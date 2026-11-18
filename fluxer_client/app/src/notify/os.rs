//! The real [`Toaster`] (notify-rust) and [`SoundPlayer`] (rodio). Neither
//! is used by tests.

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, Source as _};
use tokio::sync::mpsc;

use super::{Sound, SoundPlayer, Toast, Toaster};
use crate::backend::UiEvent;

const MESSAGE_SOUND: &[u8] = include_bytes!("../../assets/sounds/message.mp3");
const SAME_CHANNEL_SOUND: &[u8] = include_bytes!("../../assets/sounds/in-channel-notification.ogg");
/// Played sounds are cut off after this long (both are under 2 s).
const MAX_SOUND: Duration = Duration::from_secs(3);

/// Toasts shown (and their threads alive) at once; more are dropped (the
/// sound still plays). A burst, e.g. messages replayed after a reconnect,
/// then cannot start one thread per message.
const MAX_TOASTS_IN_FLIGHT: usize = 4;

/// Toasts via notify-rust. Each toast is shown from its own short-lived
/// thread, which waits for the click or dismissal (Windows reports both)
/// and turns a click into `UiEvent::NotificationClicked`.
pub struct OsToaster {
    events: mpsc::UnboundedSender<UiEvent>,
    in_flight: Arc<AtomicUsize>,
}

impl OsToaster {
    pub fn new(events: mpsc::UnboundedSender<UiEvent>) -> Self {
        Self { events, in_flight: Arc::default() }
    }
}

/// Frees a toast slot when its thread ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Toaster for OsToaster {
    fn show(&self, toast: Toast) {
        let taken = self
            .in_flight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_TOASTS_IN_FLIGHT).then_some(n + 1));
        if taken.is_err() {
            tracing::debug!("too many notifications on screen; toast skipped");
            return;
        }
        let slot = Slot(self.in_flight.clone());
        let events = self.events.clone();
        let spawned = std::thread::Builder::new().name("fluxer-toast".to_owned()).spawn(move || {
            let _slot = slot;
            let mut notification = notify_rust::Notification::new();
            notification.appname("Fluxer");
            // Windows attributes the toast to this AUMID; the process AUMID and
            // the Start menu shortcut carry the same value (see platform.rs).
            #[cfg(target_os = "windows")]
            notification.app_id(crate::platform::APP_USER_MODEL_ID);
            let shown = notification.summary(&toast.title).body(&toast.body).show();
            let handle = match shown {
                Ok(handle) => handle,
                Err(err) => {
                    tracing::warn!(%err, "could not show the notification");
                    return;
                }
            };
            show_wait(handle, events, toast);
        });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not start the notification thread");
        }
    }
}

#[cfg(target_os = "windows")]
fn show_wait(handle: notify_rust::NotificationHandle, events: mpsc::UnboundedSender<UiEvent>, toast: Toast) {
    use notify_rust::NotificationResponse;
    let result = handle.wait_for_response(move |response: &NotificationResponse| {
        if matches!(response, NotificationResponse::Default | NotificationResponse::Action(_)) {
            let _ = events.send(UiEvent::NotificationClicked { guild_id: toast.guild_id, channel_id: toast.channel_id });
        }
    });
    if let Err(err) = result {
        tracing::debug!(%err, "notification response not received");
    }
}

/// Click activation is only wired on Windows.
#[cfg(not(target_os = "windows"))]
fn show_wait(_: notify_rust::NotificationHandle, _: mpsc::UnboundedSender<UiEvent>, _: Toast) {}

/// Sounds on one lazily started thread. The output device is opened per
/// sound and closed when it ends, so no audio stream stays open while idle.
#[derive(Default)]
pub struct RodioPlayer {
    queue: Mutex<Option<std_mpsc::Sender<Sound>>>,
}

impl SoundPlayer for RodioPlayer {
    fn play(&self, sound: Sound) {
        let mut queue = self.queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(sender) = queue.as_ref()
            && sender.send(sound).is_ok()
        {
            return;
        }
        let (sender, receiver) = std_mpsc::channel();
        let spawned = std::thread::Builder::new().name("fluxer-sound".to_owned()).spawn(move || sound_loop(receiver));
        match spawned {
            Ok(_) => {
                let _ = sender.send(sound);
                *queue = Some(sender);
            }
            Err(err) => tracing::warn!(%err, "could not start the sound thread"),
        }
    }
}

/// Plays sounds until the player is dropped. Sounds queued while one is
/// playing collapse into the newest one.
fn sound_loop(receiver: std_mpsc::Receiver<Sound>) {
    while let Ok(mut sound) = receiver.recv() {
        while let Ok(newer) = receiver.try_recv() {
            sound = newer;
        }
        if let Err(err) = play_once(sound) {
            tracing::warn!(%err, ?sound, "could not play the notification sound");
        }
    }
}

fn play_once(sound: Sound) -> anyhow::Result<()> {
    let bytes = match sound {
        Sound::Message => MESSAGE_SOUND,
        Sound::SameChannel => SAME_CHANNEL_SOUND,
    };
    let source = Decoder::try_from(Cursor::new(bytes))?;
    let length = source.total_duration().unwrap_or(MAX_SOUND).min(MAX_SOUND);
    let mut sink = DeviceSinkBuilder::open_default_sink()?;
    sink.log_on_drop(false);
    sink.mixer().add(source);
    std::thread::sleep(length + Duration::from_millis(150));
    Ok(())
}
