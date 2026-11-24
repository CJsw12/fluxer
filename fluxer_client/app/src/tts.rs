//! Text-to-speech playback (`VoiceTtsUtils.tsx`) using Windows WinRT
//! `SpeechSynthesizer` + `rodio` in production and a recording stub in tests.

#[cfg(test)]
use std::sync::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAX_TTS_CHARS: usize = 2000;

/// Strips common message markup and collapses whitespace for spoken playback.
pub fn format_tts_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Angle-bracket tokens: <@123>, <@!123>, <#123>, <@&123>, <:name:123>, <a:name:123>
        if chars[i] == '<'
            && let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '>')
        {
            let token: String = chars[i + 1..i + 1 + rel_end].iter().collect();
            if let Some(rest) = token.strip_prefix(':').or_else(|| token.strip_prefix("a:"))
                && let Some((name, id)) = rest.split_once(':')
                && !name.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
            {
                out.push_str(name);
                i += rel_end + 2;
                continue;
            }
            if (token.starts_with('@') || token.starts_with('#'))
                && token.trim_start_matches(['@', '!', '&', '#']).bytes().all(|b| b.is_ascii_digit())
            {
                out.push(' ');
                i += rel_end + 2;
                continue;
            }
        }
        // Fenced code blocks ```...``` -> inner text without fence/lang line
        if i + 2 < chars.len() && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            i += 3;
            continue;
        }
        // Spoiler ||, bold **, underline __, strikethrough ~~
        if i + 1 < chars.len()
            && ((chars[i] == '|' && chars[i + 1] == '|')
                || (chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_')
                || (chars[i] == '~' && chars[i + 1] == '~'))
        {
            i += 2;
            continue;
        }
        if matches!(chars[i], '`' | '*') {
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }

    let mut collapsed = String::with_capacity(out.len());
    for word in out.split_whitespace() {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
    }
    if collapsed.chars().count() > MAX_TTS_CHARS {
        collapsed.chars().take(MAX_TTS_CHARS).collect()
    } else {
        collapsed
    }
}

/// Parses a standard RIFF/WAVE PCM buffer (8-bit unsigned or 16-bit signed LE)
/// into `(channels, sample_rate, normalized_f32_samples)`.
pub fn parse_pcm_wav(bytes: &[u8]) -> Result<(u16, u32, Vec<f32>), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Invalid WAV header".to_owned());
    }
    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut bits_per_sample: Option<u16> = None;
    let mut data_slice: Option<&[u8]> = None;

    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let chunk_len = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| "Truncated WAV chunk header".to_owned())?,
        ) as usize;
        let payload_start = offset + 8;
        let payload_end = payload_start.checked_add(chunk_len).ok_or_else(|| "WAV chunk overflow".to_owned())?;
        if payload_end > bytes.len() {
            return Err("Truncated WAV chunk".to_owned());
        }
        let payload = &bytes[payload_start..payload_end];
        if id == b"fmt " {
            if payload.len() < 16 {
                return Err("Truncated WAV fmt chunk".to_owned());
            }
            let audio_format = u16::from_le_bytes([payload[0], payload[1]]);
            if audio_format != 1 {
                return Err(format!("Unsupported WAV format: {audio_format}"));
            }
            let ch = u16::from_le_bytes([payload[2], payload[3]]);
            let sr = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let bps = u16::from_le_bytes([payload[14], payload[15]]);
            if ch == 0 || sr == 0 || !matches!(bps, 8 | 16) {
                return Err(format!("Unsupported WAV PCM parameters: {ch}ch {sr}Hz {bps}bit"));
            }
            channels = Some(ch);
            sample_rate = Some(sr);
            bits_per_sample = Some(bps);
        } else if id == b"data" {
            data_slice = Some(payload);
        }
        // RIFF chunks are padded to even byte boundaries.
        offset = payload_end + (chunk_len % 2);
    }

    let (Some(ch), Some(sr), Some(bps), Some(data)) = (channels, sample_rate, bits_per_sample, data_slice) else {
        return Err("Missing WAV fmt or data chunk".to_owned());
    };
    let samples = match bps {
        8 => data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect(),
        16 => data
            .chunks_exact(2)
            .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
            .collect(),
        _ => return Err("Unsupported WAV bit depth".to_owned()),
    };
    Ok((ch, sr, samples))
}

/// Non-blocking single-utterance text-to-speech player.
#[derive(Clone, Default)]
pub struct TtsPlayer {
    speaking: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    #[cfg(test)]
    spoken_log: Arc<Mutex<Vec<String>>>,
}

impl TtsPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.speaking.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub fn spoken(&self) -> Vec<String> {
        self.spoken_log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn speak(&self, raw_text: String) -> Result<(), String> {
        let clean = format_tts_text(&raw_text);
        if clean.is_empty() {
            return Ok(());
        }
        let epoch = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.speaking.store(true, Ordering::Release);

        #[cfg(test)]
        {
            let _ = epoch;
            self.spoken_log
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(clean);
            Ok(())
        }

        #[cfg(all(windows, not(test)))]
        {
            let speaking = self.speaking.clone();
            let gen_ref = self.generation.clone();
            std::thread::Builder::new()
                .name("fluxer-tts".to_owned())
                .spawn(move || {
                    if let Err(err) = play_windows_tts(&clean, epoch, &gen_ref) {
                        tracing::warn!(%err, "text-to-speech playback failed");
                    }
                    if gen_ref.load(Ordering::Acquire) == epoch {
                        speaking.store(false, Ordering::Release);
                    }
                })
                .map_err(|err| {
                    if self.generation.load(Ordering::Acquire) == epoch {
                        self.speaking.store(false, Ordering::Release);
                    }
                    format!("Could not start text-to-speech thread: {err}")
                })?;
            Ok(())
        }

        #[cfg(all(not(windows), not(test)))]
        {
            let _ = epoch;
            self.speaking.store(false, Ordering::Release);
            Err("Text-to-speech is not supported on this platform".to_owned())
        }
    }
}

#[cfg(all(windows, not(test)))]
fn play_windows_tts(text: &str, my_gen: u64, gen_ref: &AtomicU64) -> Result<(), String> {
    use std::num::NonZero;
    use std::time::Duration;

    use rodio::buffer::SamplesBuffer;
    use rodio::{DeviceSinkBuilder, Player};
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::DataReader;
    use windows_core::HSTRING;

    let synth = SpeechSynthesizer::new().map_err(|e| format!("SpeechSynthesizer init failed: {e}"))?;
    let stream = synth
        .SynthesizeTextToStreamAsync(&HSTRING::from(text))
        .map_err(|e| format!("SynthesizeTextToStreamAsync failed: {e}"))?
        .join()
        .map_err(|e| format!("Speech synthesis failed: {e}"))?;
    if gen_ref.load(Ordering::Acquire) != my_gen {
        return Ok(());
    }
    let total_size = u32::try_from(stream.Size().map_err(|e| format!("Stream Size failed: {e}"))?)
        .map_err(|_| "Synthesized audio stream too large".to_owned())?;
    let input = stream
        .GetInputStreamAt(0)
        .map_err(|e| format!("GetInputStreamAt failed: {e}"))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| format!("CreateDataReader failed: {e}"))?;

    let mut bytes = Vec::with_capacity(total_size as usize);
    while bytes.len() < total_size as usize {
        if gen_ref.load(Ordering::Acquire) != my_gen {
            return Ok(());
        }
        let remaining = total_size - bytes.len() as u32;
        let loaded = reader
            .LoadAsync(remaining)
            .map_err(|e| format!("DataReader LoadAsync failed: {e}"))?
            .join()
            .map_err(|e| format!("DataReader load failed: {e}"))?;
        if loaded == 0 {
            break;
        }
        let available = reader
            .UnconsumedBufferLength()
            .map_err(|e| format!("UnconsumedBufferLength failed: {e}"))?;
        if available == 0 {
            break;
        }
        let mut chunk = vec![0u8; available as usize];
        reader
            .ReadBytes(&mut chunk)
            .map_err(|e| format!("ReadBytes failed: {e}"))?;
        bytes.extend_from_slice(&chunk);
    }

    if gen_ref.load(Ordering::Acquire) != my_gen {
        return Ok(());
    }
    let (ch, sr, samples) = parse_pcm_wav(&bytes)?;
    let ch_nz = NonZero::new(ch).ok_or_else(|| "Invalid WAV channel count".to_owned())?;
    let sr_nz = NonZero::new(sr).ok_or_else(|| "Invalid WAV sample rate".to_owned())?;
    let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| format!("Audio output failed: {e}"))?;
    sink.log_on_drop(false);
    let player = Player::connect_new(sink.mixer());
    player.append(SamplesBuffer::new(ch_nz, sr_nz, samples));

    while !player.empty() {
        if gen_ref.load(Ordering::Acquire) != my_gen {
            player.stop();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm16_wav(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let riff_len = 36 + data_len;
        let block_align = channels * 2;
        let byte_rate = sample_rate * u32::from(block_align);
        let mut out = Vec::with_capacity((riff_len + 8) as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_len.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    #[test]
    fn format_tts_text_strips_markup_and_collapses_whitespace() {
        assert_eq!(
            format_tts_text("hello **world** ||secret|| <:wave:123> <@456>  `code`"),
            "hello world secret wave code"
        );
        assert_eq!(format_tts_text("   \n\t  "), "");
    }

    #[test]
    fn parse_pcm_wav_accepts_valid_pcm_and_rejects_bad_headers() {
        let wav = pcm16_wav(2, 22050, &[0, 16384, -32768, 8192]);
        let (ch, sr, samples) = parse_pcm_wav(&wav).expect("valid wav");
        assert_eq!((ch, sr), (2, 22050));
        assert_eq!(samples.len(), 4);
        assert!((samples[1] - 0.5).abs() < 0.001);
        assert!((samples[2] + 1.0).abs() < 0.001);

        assert!(parse_pcm_wav(b"NOTWAV").is_err());
        let mut non_pcm = pcm16_wav(1, 16000, &[0]);
        non_pcm[20] = 3; // IEEE float format instead of PCM (1)
        assert!(parse_pcm_wav(&non_pcm).is_err());
    }

    #[test]
    fn tts_player_speaks_and_stops() {
        let player = TtsPlayer::new();
        assert!(!player.is_speaking());
        player.speak("**hi** there".to_owned()).unwrap();
        assert!(player.is_speaking());
        assert_eq!(player.spoken(), vec!["hi there"]);
        player.stop();
        assert!(!player.is_speaking());
    }
}
