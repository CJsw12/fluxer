//! Local Fluxer fixture instance for exercising the Rust client without a
//! real account: discovery, the REST routes the client uses, and a gateway
//! with READY/RESUME/heartbeat and live MESSAGE_CREATE. Dev/test only; it
//! binds 127.0.0.1 and holds everything in memory.

mod fixture;
mod gateway;
mod http;
mod rest;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;

pub use fixture::{DEV_TOKEN, EMAIL_CAPTCHA, EMAIL_MFA, EMAIL_USER, MFA_CODE, PASSWORD};
pub use gateway::HEARTBEAT_INTERVAL_MS;

const EVENT_BUFFER: usize = 256;

/// One gateway dispatch fanned out to every identified session.
#[derive(Debug, Clone)]
pub struct Event {
    pub t: String,
    pub d: Value,
}

pub struct Shared {
    state: Mutex<fixture::State>,
    events: broadcast::Sender<Arc<Event>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, fixture::State> {
        // A panic while holding the lock leaves plain data behind; keep serving.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn dispatch(&self, t: &str, d: Value) {
        // No receivers just means no gateway is connected.
        let _ = self.events.send(Arc::new(Event { t: t.to_owned(), d }));
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// 0 picks a free port.
    pub port: u16,
    /// Post a message from another user every interval.
    pub chatter: Option<Duration>,
}

pub struct RunningServer {
    pub addr: SocketAddr,
    tasks: Vec<JoinHandle<()>>,
}

impl RunningServer {
    /// `http://127.0.0.1:<port>`, the value for the client's `--instance`.
    pub fn instance_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Runs until the accept loop ends (it only ends on a listener error).
    pub async fn wait(mut self) -> Result<()> {
        if let Some(accept) = self.tasks.first_mut() {
            accept.await.context("accept loop panicked")?;
        }
        Ok(())
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Binds `127.0.0.1:<port>` and serves in background tasks.
pub async fn start(config: Config) -> Result<RunningServer> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], config.port)))
        .await
        .with_context(|| format!("bind 127.0.0.1:{}", config.port))?;
    let addr = listener.local_addr().context("local addr")?;
    let (events, _) = broadcast::channel(EVENT_BUFFER);
    let shared = Arc::new(Shared { state: Mutex::new(fixture::State::new()), events });
    let mut tasks = Vec::new();
    let accept_shared = shared.clone();
    tasks.push(tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let shared = accept_shared.clone();
                    tokio::spawn(async move {
                        if let Err(err) = serve_connection(stream, shared).await {
                            eprintln!("connection ended: {err:#}");
                        }
                    });
                }
                Err(err) => {
                    eprintln!("accept failed: {err}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }));
    if let Some(every) = config.chatter.filter(|d| !d.is_zero()) {
        tasks.push(tokio::spawn(chatter(shared, every)));
    }
    Ok(RunningServer { addr, tasks })
}

async fn serve_connection(mut stream: TcpStream, shared: Arc<Shared>) -> Result<()> {
    let mut buf = Vec::new();
    loop {
        let Some(request) = http::read_request(&mut stream, &mut buf).await? else {
            return Ok(());
        };
        if request.is_websocket_upgrade() {
            if request.path != rest::GATEWAY_PATH {
                http::write_response(&mut stream, &request, &http::Response::error(404, "NOT_FOUND", "404: Not Found")).await?;
                continue;
            }
            let Some(key) = request.header("sec-websocket-key") else {
                http::write_response(&mut stream, &request, &http::Response::error(400, "BAD_REQUEST", "missing Sec-WebSocket-Key")).await?;
                continue;
            };
            let accept = derive_accept_key(key.as_bytes());
            let zstd = request.query_param("compress") == Some("zstd-stream");
            http::write_raw(
                &mut stream,
                &format!(
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
                ),
            )
            .await?;
            let ws = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
            return gateway::run(ws, zstd, shared).await;
        }
        let response = rest::handle(&request, &shared);
        eprintln!("{} {} -> {}", request.method, request.path, response.status);
        if !http::write_response(&mut stream, &request, &response).await? {
            return Ok(());
        }
    }
}

/// `--chatter`: another user posts into a rotating channel; every fourth
/// message mentions the current user.
async fn chatter(shared: Arc<Shared>, every: Duration) {
    let mut ticker = tokio::time::interval(every);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let event = {
            let mut state = shared.lock();
            state.chatter_count += 1;
            let n = state.chatter_count;
            let index = usize::try_from(n).unwrap_or(0);
            let channels = state.chatter_channels.clone();
            let Some(&channel_id) = channels.get(index % channels.len().max(1)) else {
                continue;
            };
            let is_dm = state.dm_ids.contains(&channel_id);
            let author = if is_dm {
                state.channels.get(&channel_id).map(|c| c["recipients"][0].clone()).unwrap_or(Value::Null)
            } else {
                state.others[index % 3].clone()
            };
            let mention = n % 4 == 0;
            let (content, mentions) = if mention {
                (format!("<@{}> chatter #{n}: are you there? 확인 부탁해요", state.me_id), vec![state.me.clone()])
            } else {
                (format!("chatter #{n} at {}", fixture::iso8601(fixture::now_ms())), Vec::new())
            };
            let (_, event) = state.post_message(channel_id, &author, &content, mentions, None);
            event
        };
        shared.dispatch("MESSAGE_CREATE", event);
    }
}

/// Startup banner for the binary.
pub fn banner(server: &RunningServer) -> String {
    let instance = server.instance_url();
    [
        format!("fluxer_devserver listening on {instance} (127.0.0.1 only)"),
        format!("run the client: fluxer_client --allow-insecure-loopback --instance {instance}"),
        format!("accounts (password '{PASSWORD}'):"),
        format!("  {EMAIL_USER}     direct login"),
        format!("  {EMAIL_MFA}      MFA ticket, TOTP code {MFA_CODE}"),
        format!("  {EMAIL_CAPTCHA}  400 CAPTCHA_REQUIRED unless X-Captcha-Token is sent (not verified)"),
        format!("fixed token (for --token-env): {DEV_TOKEN}"),
    ]
    .join("\n")
}
