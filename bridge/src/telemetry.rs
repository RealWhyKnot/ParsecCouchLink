//! Remote-debug tunnel client.
//!
//! Connects to the tunnel server over WSS, holds the connection open with
//! reconnect+backoff, publishes outbound events (heartbeat, journal, exec
//! output, file chunks), and dispatches inbound commands to whatever channel
//! the run-mode loop wires up.
//!
//! Everything in here is opt-in: if `config.telemetry` is `None`, the bridge
//! does not even start the task.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

use crate::config::TelemetryConfig;
use crate::journal;

/// Cheap-to-clone publisher used by anything that wants to emit a tunnel
/// event. Dropping the last clone shuts down the WS task.
#[derive(Clone)]
pub struct TelemetryHandle {
    tx: mpsc::Sender<OutEvent>,
}

impl TelemetryHandle {
    /// Fire-and-forget publish. Returns immediately. Drops the event if the
    /// channel is full (slow tunnel) so the caller never blocks the run loop.
    pub fn publish(&self, ev: OutEvent) {
        let _ = self.tx.try_send(ev);
    }

    /// Shortcut for the common case of a journal-style line.
    pub fn note(&self, message: impl Into<String>) {
        self.publish(OutEvent::System(SystemBody {
            message: message.into(),
        }));
    }
}

// ---------- outbound event shapes ----------

/// One event the bridge wants to publish. Serializes to the wire shape
/// `{ ts, kind, payload }` via `OutFrame`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum OutEvent {
    Hello(HelloBody),
    Heartbeat(HeartbeatBody),
    Journal(JournalBody),
    ExecStarted(ExecStartedBody),
    ExecStdout(ExecLineBody),
    ExecStderr(ExecLineBody),
    ExecExit(ExecExitBody),
    FileChunk(FileChunkBody),
    FileEof(FileEofBody),
    System(SystemBody),
    Error(ErrorBody),
}

#[derive(Clone, Debug, Serialize)]
pub struct HelloBody {
    pub bridge_version: String,
    pub host_os: String,
    pub host_arch: String,
    pub board_type: Option<String>,
    pub firmware_version: Option<String>,
    pub device_uid: Option<String>,
    pub started_at_ms: i64,
    pub exec_allowlist: Vec<String>,
    pub file_keys: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct HeartbeatBody {
    pub uptime_s: u64,
    pub peer: Option<String>,
    pub tx: u64,
    pub rx: u64,
    pub wifi: Option<String>,
    pub rssi: Option<i32>,
    pub parsec: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct JournalBody {
    pub category: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecStartedBody {
    pub id: String,
    pub argv: Vec<String>,
    pub resolved_exe: String,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecLineBody {
    pub id: String,
    pub line: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecExitBody {
    pub id: String,
    pub code: i32,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileChunkBody {
    pub id: String,
    pub seq: u32,
    pub b64: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileEofBody {
    pub id: String,
    pub total_chunks: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct SystemBody {
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorBody {
    pub id: Option<String>,
    pub message: String,
}

/// Wire envelope. `ts` is unix ms.
#[derive(Serialize)]
struct OutFrame<'a> {
    ts: i64,
    #[serde(flatten)]
    body: &'a OutEvent,
}

// ---------- inbound command shapes ----------

/// Commands dispatched in from the tunnel. The run loop is responsible for
/// actually carrying these out and emitting matching `OutEvent`s.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteCmd {
    Exec {
        #[serde(default = "default_cmd_id")]
        id: String,
        argv: Vec<String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    ReadFile {
        #[serde(default = "default_cmd_id")]
        id: String,
        key: String,
    },
    Kill {
        #[serde(default = "default_cmd_id")]
        id: String,
        target_id: String,
    },
    PullLog {
        #[serde(default = "default_cmd_id")]
        id: String,
    },
    DropPeer {
        #[serde(default = "default_cmd_id")]
        id: String,
    },
    SetLogFilter {
        #[serde(default = "default_cmd_id")]
        id: String,
        directive: String,
    },
}

impl RemoteCmd {
    #[allow(dead_code)] // exposed for future dispatchers that index by id
    pub fn id(&self) -> &str {
        match self {
            Self::Exec { id, .. }
            | Self::ReadFile { id, .. }
            | Self::Kill { id, .. }
            | Self::PullLog { id }
            | Self::DropPeer { id }
            | Self::SetLogFilter { id, .. } => id,
        }
    }
}

fn default_cmd_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("c_local_{n}")
}

// ---------- session mint ----------

#[derive(Deserialize)]
pub struct MintResponse {
    pub write_token: String,
    pub view_token: String,
    pub view_url: String,
}

/// One-shot HTTPS POST to mint a new session pair on the tunnel server. Used
/// by `couchlink tunnel start` the first time, or `tunnel reset`.
pub async fn mint_session(server: &str) -> Result<MintResponse> {
    let url = format!("{}/api/sessions", server.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("couchlink/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build http client")?;
    let resp = client
        .post(&url)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("tunnel mint returned {status}: {body}");
    }
    let parsed: MintResponse = resp.json().await.context("decode mint response")?;
    Ok(parsed)
}

// ---------- runtime ----------

/// Metadata the bridge knows about itself at start time. Sent as the first
/// `hello` frame on every successful WS connect.
#[derive(Clone, Debug)]
pub struct HostMetadata {
    pub board_type: Option<String>,
    pub firmware_version: Option<String>,
    pub device_uid: Option<String>,
    pub exec_allowlist: Vec<String>,
    pub file_keys: Vec<String>,
}

/// Boot the telemetry task. Returns a handle for publishing events plus a
/// receiver the caller drains for inbound commands.
///
/// If `cfg.write_token` is empty, returns a stub handle that drops everything
/// (so the rest of the bridge can publish unconditionally without checking).
pub fn spawn(
    cfg: TelemetryConfig,
    meta: HostMetadata,
    shutdown: Arc<Notify>,
) -> (TelemetryHandle, mpsc::Receiver<RemoteCmd>) {
    let (out_tx, out_rx) = mpsc::channel::<OutEvent>(512);
    let (cmd_tx, cmd_rx) = mpsc::channel::<RemoteCmd>(64);
    let handle = TelemetryHandle { tx: out_tx };

    if cfg.write_token.is_empty() || cfg.server.is_empty() {
        tracing::info!("telemetry: no session in config; tunnel inactive");
        return (handle, cmd_rx);
    }

    // Hello frame for every reconnect cycle. Captured by value so reconnects
    // don't need to re-collect metadata.
    let hello = OutEvent::Hello(HelloBody {
        bridge_version: env!("CARGO_PKG_VERSION").to_string(),
        host_os: std::env::consts::OS.to_string(),
        host_arch: std::env::consts::ARCH.to_string(),
        board_type: meta.board_type.clone(),
        firmware_version: meta.firmware_version.clone(),
        device_uid: meta.device_uid.clone(),
        started_at_ms: chrono::Utc::now().timestamp_millis(),
        exec_allowlist: meta.exec_allowlist.clone(),
        file_keys: meta.file_keys.clone(),
    });

    let url = cfg.ws_url();
    let token = cfg.write_token.clone();
    let view_url = cfg.view_url();

    tracing::info!("tunnel: configured ({}), view URL is {}", url, view_url);

    // Mirror commands the host should know about into local stdout/log so the
    // host can watch what's happening in their terminal.
    let cmd_tx_for_dispatch = cmd_tx.clone();
    let (raw_cmd_tx, mut raw_cmd_rx) = mpsc::channel::<RemoteCmd>(64);
    tokio::spawn(async move {
        while let Some(cmd) = raw_cmd_rx.recv().await {
            log_remote_cmd_for_host(&cmd);
            let _ = cmd_tx_for_dispatch.send(cmd).await;
        }
    });

    tokio::spawn(connection_loop(
        url, token, view_url, hello, out_rx, raw_cmd_tx, shutdown,
    ));

    (handle, cmd_rx)
}

async fn connection_loop(
    url: String,
    write_token: String,
    view_url: String,
    hello: OutEvent,
    mut out_rx: mpsc::Receiver<OutEvent>,
    cmd_tx: mpsc::Sender<RemoteCmd>,
    shutdown: Arc<Notify>,
) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);
    let mut consec_failures = 0u32;

    loop {
        tracing::info!("tunnel: connecting to {url}");
        match connect_once(
            &url,
            &write_token,
            &hello,
            &mut out_rx,
            &cmd_tx,
            shutdown.clone(),
        )
        .await
        {
            Ok(reason) => {
                tracing::info!("tunnel: session ended ({reason}); reconnecting");
                backoff = Duration::from_secs(1);
                consec_failures = 0;
            }
            Err(e) => {
                consec_failures = consec_failures.saturating_add(1);
                if consec_failures == 1 || consec_failures.is_multiple_of(10) {
                    tracing::warn!(
                        "tunnel: connect failed (attempt {consec_failures}): {e}. View URL: {view_url}"
                    );
                }
            }
        }

        tokio::select! {
            _ = shutdown.notified() => {
                tracing::info!("tunnel: shutdown requested");
                return;
            }
            _ = tokio::time::sleep(backoff) => { }
        }
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn connect_once(
    url: &str,
    write_token: &str,
    hello: &OutEvent,
    out_rx: &mut mpsc::Receiver<OutEvent>,
    cmd_tx: &mpsc::Sender<RemoteCmd>,
    shutdown: Arc<Notify>,
) -> Result<&'static str> {
    let mut request = url
        .into_client_request()
        .with_context(|| format!("invalid tunnel url '{url}'"))?;
    request.headers_mut().insert(
        "X-Write-Token",
        HeaderValue::from_str(write_token).context("write token contains invalid bytes")?,
    );

    let (ws, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("ws connect")?;
    tracing::info!("tunnel: connected");

    let (mut sink, mut stream) = ws.split();

    // Send hello first.
    let hello_msg = encode(hello).context("encode hello")?;
    sink.send(Message::Text(hello_msg))
        .await
        .context("send hello")?;

    loop {
        tokio::select! {
            biased;

            _ = shutdown.notified() => {
                let _ = sink.send(Message::Close(None)).await;
                return Ok("shutdown");
            }

            msg = stream.next() => {
                match msg {
                    None => return Ok("server eof"),
                    Some(Err(e)) => return Err(e.into()),
                    Some(Ok(Message::Text(t))) => {
                        if let Some(cmd) = parse_command(&t) {
                            if cmd_tx.send(cmd).await.is_err() {
                                return Ok("cmd consumer dropped");
                            }
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        if let Ok(s) = std::str::from_utf8(&b) {
                            if let Some(cmd) = parse_command(s) {
                                if cmd_tx.send(cmd).await.is_err() {
                                    return Ok("cmd consumer dropped");
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) => return Ok("server close"),
                    Some(Ok(_)) => { /* ignore pong/frame */ }
                }
            }

            ev = out_rx.recv() => {
                let Some(ev) = ev else { return Ok("publisher dropped"); };
                let msg = match encode(&ev) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("tunnel: encode event failed: {e}");
                        continue;
                    }
                };
                if let Err(e) = sink.send(Message::Text(msg)).await {
                    return Err(e.into());
                }
            }
        }
    }
}

fn encode(ev: &OutEvent) -> Result<String> {
    let frame = OutFrame {
        ts: chrono::Utc::now().timestamp_millis(),
        body: ev,
    };
    Ok(serde_json::to_string(&frame)?)
}

fn parse_command(s: &str) -> Option<RemoteCmd> {
    match serde_json::from_str::<RemoteCmd>(s) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!("tunnel: unrecognized command frame ({e}): {s}");
            None
        }
    }
}

/// Log every remote command to the bridge's own stderr/log so the host can
/// see what's being run on their machine. This is the "user can watch as the
/// helper does stuff" requirement -- the bridge console is the audit trail.
fn log_remote_cmd_for_host(cmd: &RemoteCmd) {
    match cmd {
        RemoteCmd::Exec { id, argv, cwd } => {
            tracing::info!(
                "tunnel cmd [{id}] exec {}{}",
                argv.join(" "),
                cwd.as_ref()
                    .map(|p| format!(" (cwd={})", p.display()))
                    .unwrap_or_default()
            );
            crate::journal!("tunnel", "exec [{id}] {}", argv.join(" "));
        }
        RemoteCmd::ReadFile { id, key } => {
            tracing::info!("tunnel cmd [{id}] read_file {key}");
            crate::journal!("tunnel", "read_file [{id}] {key}");
        }
        RemoteCmd::Kill { id, target_id } => {
            tracing::info!("tunnel cmd [{id}] kill target={target_id}");
            crate::journal!("tunnel", "kill [{id}] target={target_id}");
        }
        RemoteCmd::PullLog { id } => {
            tracing::info!("tunnel cmd [{id}] pull_log");
            crate::journal!("tunnel", "pull_log [{id}]");
        }
        RemoteCmd::DropPeer { id } => {
            tracing::info!("tunnel cmd [{id}] drop_peer");
            crate::journal!("tunnel", "drop_peer [{id}]");
        }
        RemoteCmd::SetLogFilter { id, directive } => {
            tracing::info!("tunnel cmd [{id}] set_log_filter '{directive}'");
            crate::journal!("tunnel", "set_log_filter [{id}] '{directive}'");
        }
    }
}

// ---------- journal forwarder ----------

/// Subscribe to the global journal bus and forward each entry to telemetry.
/// One spawn per session is enough; dies when the handle is dropped.
pub fn forward_journal(handle: TelemetryHandle) {
    tokio::spawn(async move {
        let mut rx = journal::subscribe();
        loop {
            match rx.recv().await {
                Ok(entry) => {
                    handle.publish(OutEvent::Journal(JournalBody {
                        category: entry.category,
                        message: entry.message,
                    }));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Some events were dropped; keep going.
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}
