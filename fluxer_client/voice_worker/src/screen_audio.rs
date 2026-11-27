//! WASAPI capture of the default render endpoint for a screen-share audio track.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource, native::NativeAudioSource};
use tokio::sync::mpsc;

pub(super) struct Capture {
    stream: Stream,
    source: NativeAudioSource,
    sid: TrackSid,
    pcm: mpsc::Receiver<Vec<i16>>,
    device_errors: mpsc::Receiver<()>,
}

impl Capture {
    pub(super) async fn start(room: &Room) -> Result<Self, &'static str> {
        let device = cpal::default_host().default_output_device().ok_or("No Windows playback device")?;
        let format = device.default_output_config().map_err(|_| "Could not read the Windows playback format")?;
        let config = format.config();
        // shortcut: capture mono/stereo render formats, add an explicit downmix for surround endpoints.
        if !matches!(config.channels, 1 | 2) || config.sample_rate < 8_000
            || config.sample_rate > 192_000 || config.sample_rate % 100 != 0
        {
            return Err("Unsupported Windows playback format for screen sharing");
        }
        let (pcm_tx, pcm) = mpsc::channel(4);
        let (error_tx, device_errors) = mpsc::channel(1);
        let stream = match format.sample_format() {
            SampleFormat::U8 => loopback_stream::<u8>(&device, config, pcm_tx, error_tx),
            SampleFormat::I16 => loopback_stream::<i16>(&device, config, pcm_tx, error_tx),
            SampleFormat::I24 => loopback_stream::<cpal::I24>(&device, config, pcm_tx, error_tx),
            SampleFormat::I32 => loopback_stream::<i32>(&device, config, pcm_tx, error_tx),
            SampleFormat::I64 => loopback_stream::<i64>(&device, config, pcm_tx, error_tx),
            SampleFormat::F32 => loopback_stream::<f32>(&device, config, pcm_tx, error_tx),
            SampleFormat::F64 => loopback_stream::<f64>(&device, config, pcm_tx, error_tx),
            _ => return Err("Unsupported Windows playback sample format"),
        }.map_err(|_| "Could not open Windows loopback capture")?;
        let source = NativeAudioSource::new(
            AudioSourceOptions::default(), config.sample_rate, config.channels.into(), 0,
        );
        let participant = room.local_participant();
        let track = LocalAudioTrack::create_audio_track(
            "screen_share_audio", RtcAudioSource::Native(source.clone()),
        );
        let sid = participant.publish_track(LocalTrack::Audio(track), TrackPublishOptions {
            source: TrackSource::ScreenshareAudio,
            ..Default::default()
        }).await.map_err(|_| "Could not publish screen share audio")?.sid();
        if stream.play().is_err() {
            let _ = participant.unpublish_track(&sid).await;
            return Err("Could not start Windows loopback capture");
        }
        Ok(Self { stream, source, sid, pcm, device_errors })
    }

    pub(super) async fn stop(self, room: &Room) {
        drop(self.stream);
        let _ = room.local_participant().unpublish_track(&self.sid).await;
    }

    pub(super) async fn forward_until_failure(&mut self) -> &'static str {
        loop {
            tokio::select! {
                biased;
                _ = self.device_errors.recv() => return "Windows loopback capture stopped",
                samples = self.pcm.recv() => {
                    let Some(samples) = samples else { return "Windows loopback capture ended"; };
                    let frame = AudioFrame {
                        data: samples.into(),
                        sample_rate: self.source.sample_rate(),
                        num_channels: self.source.num_channels(),
                        samples_per_channel: self.source.sample_rate() / 100,
                    };
                    if self.source.capture_frame(&frame).await.is_err() {
                        return "Could not send screen share audio";
                    }
                }
            }
        }
    }
}

fn loopback_stream<T>(
    device: &Device,
    config: StreamConfig,
    pcm: mpsc::Sender<Vec<i16>>,
    errors: mpsc::Sender<()>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    i16: FromSample<T>,
{
    let frame_samples = (config.sample_rate / 100) as usize * usize::from(config.channels);
    let mut frame = Vec::with_capacity(frame_samples);
    device.build_input_stream(
        config,
        move |data: &[T], _| queue_pcm(data, frame_samples, &mut frame, &pcm),
        move |_| { let _ = errors.try_send(()); },
        None,
    )
}

fn queue_pcm<T: SizedSample>(
    data: &[T],
    frame_samples: usize,
    frame: &mut Vec<i16>,
    pcm: &mpsc::Sender<Vec<i16>>,
) where
    i16: FromSample<T>,
{
    for sample in data {
        frame.push((*sample).to_sample());
        if frame.len() == frame_samples {
            if let Ok(slot) = pcm.try_reserve() {
                slot.send(std::mem::replace(frame, Vec::with_capacity(frame_samples)));
            } else {
                frame.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_packets_span_callbacks_and_drop_whole_frames_under_backpressure() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut pending = Vec::new();
        queue_pcm(&[1_i16, 2], 4, &mut pending, &tx);
        assert!(rx.try_recv().is_err());
        queue_pcm(&[3_i16, 4, 5, 6, 7, 8, 9], 4, &mut pending, &tx);
        assert_eq!(rx.try_recv().unwrap(), [1, 2, 3, 4]);
        assert_eq!(pending, [9]);
        assert!(rx.try_recv().is_err());
    }
}
