//! Authenticated loopback IPC for single-instance window activation and
//! `fluxer://` deep link handoff (`instance.ipc` next to `sessions.lock`).

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use gpui_kit::{AnyWindowHandle, App, AsyncApp, WeakEntity};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::deep_links::{MAX_DEEP_LINK_LEN, has_blocked_chars, is_app_protocol_url};
use crate::root::AppRoot;

const IPC_FILE_NAME: &str = "instance.ipc";
const MAX_FILE_BYTES: u64 = 1024;
const MAX_LINE_BYTES: usize = 8192;
const MAX_ACTIVE_CLIENTS: usize = 4;
const IO_TIMEOUT: Duration = Duration::from_millis(500);
const RETRY_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingleInstanceMessage {
    Activate,
    DeepLink(String),
}

fn random_token_hex() -> String {
    let mut bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut bytes);
    let mut hex = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

fn write_ipc_file(path: &Path, port: u16, token: &str) -> anyhow::Result<()> {
    let dir = path.parent().context("ipc path missing parent directory")?;
    let tmp = dir.join(format!(
        "{IPC_FILE_NAME}.tmp-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let payload = serde_json::json!({
        "port": port,
        "token": token,
        "pid": std::process::id(),
    })
    .to_string();

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let result = (|| -> anyhow::Result<()> {
        let mut file = opts.open(&tmp).context("create temp ipc file")?;
        file.write_all(payload.as_bytes()).context("write temp ipc file")?;
        file.sync_all().context("sync temp ipc file")?;
        drop(file);
        fs::rename(&tmp, path).context("install ipc file")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn read_bounded_line(stream: &mut TcpStream, deadline: Instant) -> Option<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let _ = stream.set_read_timeout(Some(remaining));
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => {
                if buf.len() >= MAX_LINE_BYTES {
                    return None;
                }
                buf.push(byte[0]);
                if byte[0] == b'\n' {
                    return String::from_utf8(buf).ok();
                }
            }
            Err(_) => return None,
        }
    }
}

fn handle_client(
    mut stream: TcpStream,
    expected_token: &str,
    tx: &mpsc::UnboundedSender<SingleInstanceMessage>,
) {
    let deadline = Instant::now() + IO_TIMEOUT;
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Some(line) = read_bounded_line(&mut stream, deadline) else {
        return;
    };
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return;
    };
    let Some(token) = val.get("token").and_then(Value::as_str) else {
        return;
    };
    if !ct_eq(token.as_bytes(), expected_token.as_bytes()) {
        return;
    }
    let msg = match val.get("action").and_then(Value::as_str) {
        Some("activate") => SingleInstanceMessage::Activate,
        Some("deep_link") => {
            let Some(url) = val.get("url").and_then(Value::as_str) else {
                return;
            };
            if has_blocked_chars(url) {
                return;
            }
            let trimmed = url.trim();
            if trimmed.is_empty() || trimmed.len() > MAX_DEEP_LINK_LEN || !is_app_protocol_url(trimmed) {
                return;
            }
            SingleInstanceMessage::DeepLink(trimmed.to_owned())
        }
        _ => return,
    };
    if tx.send(msg).is_ok() {
        let _ = stream.write_all(b"ok\n");
        let _ = stream.flush();
    }
}

pub struct SingleInstanceServer {
    ipc_path: PathBuf,
    port: u16,
    token: String,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct ClientSlot(Arc<AtomicUsize>);

impl Drop for ClientSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn try_acquire_slot(counter: &Arc<AtomicUsize>) -> Option<ClientSlot> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_ACTIVE_CLIENTS).then_some(n + 1)
        })
        .ok()
        .map(|_| ClientSlot(counter.clone()))
}

impl SingleInstanceServer {
    pub fn bind(dir: &Path) -> anyhow::Result<(Self, mpsc::UnboundedReceiver<SingleInstanceMessage>)> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind loopback ipc listener")?;
        let port = listener.local_addr().context("read ipc listener port")?.port();
        let token = random_token_hex();
        let ipc_path = dir.join(IPC_FILE_NAME);
        write_ipc_file(&ipc_path, port, &token)?;

        let (tx, rx) = mpsc::unbounded_channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_flag = stopped.clone();
        let thread_token = token.clone();
        let active_clients = Arc::new(AtomicUsize::new(0));
        let spawn = std::thread::Builder::new()
            .name("fluxer-ipc".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stop_flag.load(Ordering::Acquire) {
                        break;
                    }
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let Some(slot) = try_acquire_slot(&active_clients) else {
                        drop(stream);
                        continue;
                    };
                    let tok = thread_token.clone();
                    let tx = tx.clone();
                    if std::thread::Builder::new()
                        .name("fluxer-ipc-req".to_owned())
                        .spawn(move || {
                            let _slot = slot;
                            handle_client(stream, &tok, &tx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });

        let thread = match spawn {
            Ok(thread) => thread,
            Err(err) => {
                remove_ipc_if_owned(&ipc_path, &token);
                return Err(err).context("spawn single-instance ipc thread");
            }
        };

        Ok((
            Self {
                ipc_path,
                port,
                token,
                stopped,
                thread: Some(thread),
            },
            rx,
        ))
    }
}

impl Drop for SingleInstanceServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(100));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        remove_ipc_if_owned(&self.ipc_path, &self.token);
    }
}

fn read_ipc_path(path: &Path) -> Option<(u16, String)> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return None;
    }
    let val: Value = serde_json::from_slice(&bytes).ok()?;
    let port_u64 = val.get("port").and_then(Value::as_u64)?;
    let port = u16::try_from(port_u64).ok().filter(|&p| p > 0)?;
    let token = val.get("token").and_then(Value::as_str)?.trim();
    if token.is_empty() {
        return None;
    }
    Some((port, token.to_owned()))
}

fn remove_ipc_if_owned(path: &Path, expected_token: &str) {
    if let Some((_, on_disk_token)) = read_ipc_path(path)
        && ct_eq(on_disk_token.as_bytes(), expected_token.as_bytes())
    {
        let _ = fs::remove_file(path);
    }
}

/// Forwards activation (or a deep link URL) to the running instance holding `{dir}/sessions.lock`,
/// retrying briefly if the primary instance is still completing cold-start IPC binding.
pub fn try_forward_with_retry(dir: &Path, deep_link: Option<&str>, max_wait: Duration) -> bool {
    let deadline = Instant::now() + max_wait;
    loop {
        if try_forward_to_running(dir, deep_link) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(remaining.min(RETRY_POLL_INTERVAL));
    }
}

/// Forwards activation (or a deep link URL) to the running instance holding `{dir}/sessions.lock`.
pub fn try_forward_to_running(dir: &Path, deep_link: Option<&str>) -> bool {
    let Some((port, token)) = read_ipc_path(&dir.join(IPC_FILE_NAME)) else {
        return false;
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, IO_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    let validated = deep_link.and_then(|raw| {
        if has_blocked_chars(raw) {
            return None;
        }
        let trimmed = raw.trim();
        (!trimmed.is_empty() && trimmed.len() <= MAX_DEEP_LINK_LEN && is_app_protocol_url(trimmed))
            .then_some(trimmed)
    });
    let payload = match validated {
        Some(url) => serde_json::json!({ "token": token, "action": "deep_link", "url": url }),
        None => serde_json::json!({ "token": token, "action": "activate" }),
    };
    let Ok(mut wire) = serde_json::to_vec(&payload) else {
        return false;
    };
    wire.push(b'\n');
    if wire.len() > MAX_LINE_BYTES {
        return false;
    }
    if stream.write_all(&wire).is_err() || stream.flush().is_err() {
        return false;
    }
    let mut reader = BufReader::new((&mut stream).take(64));
    let mut ack = String::new();
    reader.read_line(&mut ack).is_ok() && ack.trim() == "ok"
}

/// Pumps authenticated IPC messages onto the GPUI window to focus or navigate.
pub fn install(
    mut rx: mpsc::UnboundedReceiver<SingleInstanceMessage>,
    window_handle: AnyWindowHandle,
    root: WeakEntity<AppRoot>,
    cx: &mut App,
) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(msg) = rx.recv().await {
            match msg {
                SingleInstanceMessage::Activate => {
                    let _ = window_handle.update(cx, |_, window, _| window.activate_window());
                }
                SingleInstanceMessage::DeepLink(url) => {
                    let _ = window_handle.update(cx, |_, window, cx| {
                        window.activate_window();
                        let _ = root.update(cx, |r, cx| r.handle_deep_link(&url, window, cx));
                    });
                }
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!("fluxer-ipc-test-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn authenticated_handoff_delivers_activate_and_deep_link() {
        let dir = TempDir::new("handoff");
        let (server, mut rx) = SingleInstanceServer::bind(&dir.0).expect("bind ipc");
        assert!(dir.0.join(IPC_FILE_NAME).is_file());

        assert!(try_forward_to_running(&dir.0, None));
        assert_eq!(rx.try_recv(), Ok(SingleInstanceMessage::Activate));

        assert!(try_forward_to_running(&dir.0, Some("fluxer://channels/1/2/3")));
        assert_eq!(
            rx.try_recv(),
            Ok(SingleInstanceMessage::DeepLink("fluxer://channels/1/2/3".to_owned()))
        );

        drop(server);
        assert!(!dir.0.join(IPC_FILE_NAME).exists());
    }

    #[test]
    fn drop_does_not_delete_newer_instances_ipc_file() {
        let dir = TempDir::new("ownership");
        let (first, _rx1) = SingleInstanceServer::bind(&dir.0).expect("bind first");
        let (second, mut rx2) = SingleInstanceServer::bind(&dir.0).expect("bind second");
        let (_, second_token) = read_ipc_path(&dir.0.join(IPC_FILE_NAME)).expect("ipc present");
        assert_eq!(second_token, second.token);

        drop(first);
        assert!(dir.0.join(IPC_FILE_NAME).is_file(), "old instance must not delete newer instance.ipc");
        assert!(try_forward_to_running(&dir.0, None));
        assert_eq!(rx2.try_recv(), Ok(SingleInstanceMessage::Activate));

        drop(second);
        assert!(!dir.0.join(IPC_FILE_NAME).exists());
    }

    #[test]
    fn rejects_bad_token_slowloris_oversized_payload_and_missing_file() {
        let dir = TempDir::new("reject");
        assert!(!try_forward_to_running(&dir.0, None));

        let (server, mut rx) = SingleInstanceServer::bind(&dir.0).expect("bind ipc");
        let addr = SocketAddr::from(([127, 0, 0, 1], server.port));

        let mut bad_token = TcpStream::connect_timeout(&addr, IO_TIMEOUT).expect("connect");
        let _ = bad_token.write_all(br#"{"token":"wrong","action":"activate"}"#);
        let _ = bad_token.write_all(b"\n");
        let mut resp = String::new();
        let _ = BufReader::new(bad_token).read_line(&mut resp);
        assert!(resp.is_empty());

        // Held-open slow connection does not block a legitimate request
        let stalled = TcpStream::connect_timeout(&addr, IO_TIMEOUT).expect("connect stalled");
        assert!(try_forward_with_retry(&dir.0, None, Duration::from_millis(300)));
        assert_eq!(rx.try_recv(), Ok(SingleInstanceMessage::Activate));
        drop(stalled);

        let mut huge = TcpStream::connect_timeout(&addr, IO_TIMEOUT).expect("connect");
        let _ = huge.write_all(&vec![b'a'; MAX_LINE_BYTES + 16]);
        let _ = huge.write_all(b"\n");
        drop(huge);

        assert!(try_forward_to_running(&dir.0, None));
        assert_eq!(rx.try_recv(), Ok(SingleInstanceMessage::Activate));

        drop(server);
    }
}
