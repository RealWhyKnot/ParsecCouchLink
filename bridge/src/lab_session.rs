//! WSS session plumbing for `couchlink lab-mode`.
//!
//! Holds a single tunnel-server connection open with reconnect+backoff,
//! pushes outbound events from an `mpsc::Receiver`, and forwards inbound
//! commands as raw JSON strings to a callback for the command surface to
//! parse. The command-shape JSON is intentionally not typed here; that
//! lives in `cmd_lab` alongside the dispatch.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

use crate::config::{self, Config, TelemetryConfig};

/// Tunnel-server response from `POST /api/sessions`.
#[derive(Deserialize)]
struct MintResponse {
    write_token: String,
    view_token: String,
    #[allow(dead_code)] // server echoes; we derive our own from config
    view_url: Option<String>,
}

/// Either load saved tokens or mint a fresh session against `server`. The
/// resulting config is persisted to `config.toml` under `[lab]`.
///
/// `force_new` skips the cache and always mints.
pub async fn ensure_session(server: &str, force_new: bool) -> Result<TelemetryConfig> {
    let mut cfg = config::load().unwrap_or_default();
    if !force_new {
        if let Some(existing) = cfg.lab.as_ref() {
            if !existing.write_token.is_empty() && !existing.view_token.is_empty() {
                return Ok(existing.clone());
            }
        }
    }

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

    let new_cfg = TelemetryConfig {
        server: server.to_string(),
        write_token: parsed.write_token,
        view_token: parsed.view_token,
    };
    cfg.lab = Some(new_cfg.clone());
    save_lab_only(&cfg)?;
    Ok(new_cfg)
}

/// Re-write `config.toml` with the lab section, preserving everything
/// else. Wraps `config::save` so callers don't have to remember which
/// fields are session-local.
fn save_lab_only(cfg: &Config) -> Result<()> {
    config::save(cfg)
}

/// Run the WSS connection loop. Returns when `shutdown` is notified.
///
/// `hello_json` is the first message sent on every successful connect.
/// Outbound `outbox` items are serialized JSON strings -- the caller
/// (cmd_lab) builds the wire envelope. Inbound text frames are pushed
/// to `inbox_tx` as raw JSON; the caller parses them as `LabCmd`.
pub async fn run_loop(
    cfg: TelemetryConfig,
    hello_json: String,
    mut outbox: mpsc::Receiver<String>,
    inbox_tx: mpsc::Sender<String>,
    shutdown: Arc<Notify>,
) {
    let url = cfg.ws_url();
    let write_token = cfg.write_token.clone();
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);
    let mut consec_failures = 0u32;

    loop {
        tracing::info!("lab-mode: connecting to {url}");
        match connect_once(
            &url,
            &write_token,
            &hello_json,
            &mut outbox,
            &inbox_tx,
            shutdown.clone(),
        )
        .await
        {
            Ok(reason) => {
                tracing::info!("lab-mode: session ended ({reason}); reconnecting");
                backoff = Duration::from_secs(1);
                consec_failures = 0;
            }
            Err(e) => {
                consec_failures = consec_failures.saturating_add(1);
                if consec_failures == 1 || consec_failures.is_multiple_of(10) {
                    tracing::warn!("lab-mode: connect failed (attempt {consec_failures}): {e}");
                }
            }
        }

        tokio::select! {
            _ = shutdown.notified() => {
                tracing::info!("lab-mode: shutdown requested");
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
    hello_json: &str,
    outbox: &mut mpsc::Receiver<String>,
    inbox_tx: &mpsc::Sender<String>,
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
    tracing::info!("lab-mode: connected");

    let (mut sink, mut stream) = ws.split();

    sink.send(Message::Text(hello_json.to_string()))
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
                        if inbox_tx.send(t).await.is_err() {
                            return Ok("dispatcher dropped");
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        if let Ok(s) = std::str::from_utf8(&b) {
                            if inbox_tx.send(s.to_string()).await.is_err() {
                                return Ok("dispatcher dropped");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) => return Ok("server close"),
                    Some(Ok(_)) => { /* ignore pong/raw */ }
                }
            }

            out = outbox.recv() => {
                let Some(msg) = out else { return Ok("publisher dropped"); };
                if let Err(e) = sink.send(Message::Text(msg)).await {
                    return Err(e.into());
                }
            }
        }
    }
}
