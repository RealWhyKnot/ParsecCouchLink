//! `couchlink lab-mode` -- silent remote-flash session.
//!
//! Replaces the broad `couchlink tunnel` flow with a tight command
//! surface focused on the deploy/test iteration loop: receive a UF2
//! over the tunnel, drop the Pico into BOOTSEL (via firmware command or
//! `picotool` fallback), flash, run health checks, ship the bundle
//! back. Nothing visible on the friend's console besides a one-time
//! banner.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Notify};

use crate::cdc;
use crate::cmd_bundle;
use crate::cmd_doctor;
use crate::cmd_flash;
use crate::config;
use crate::lab_session;
use crate::network;
use crate::protocol;

const DEFAULT_TUNNEL_SERVER: &str = "https://couchlink.whyknot.dev";
const UPLOAD_FILENAME: &str = "couchlink-lab.uf2";

pub async fn run(server: Option<String>, reset: bool) -> Result<()> {
    let server = server.unwrap_or_else(|| DEFAULT_TUNNEL_SERVER.to_string());
    let cfg = lab_session::ensure_session(&server, reset)
        .await
        .context("provisioning lab-mode session with tunnel server")?;
    let view_url = cfg.view_url();

    // Silent everywhere except this once. The two-line banner is the entire
    // friend-visible surface of lab-mode.
    println!("Lab session: {view_url}");
    println!("Ctrl+C to end.");

    let shutdown = Arc::new(Notify::new());
    let shutdown_signal = shutdown.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown_signal.notify_waiters();
        }
    });

    let hello = LabEvent::Hello {
        bridge_version: env!("CARGO_PKG_VERSION").to_string(),
        host_os: std::env::consts::OS.to_string(),
        host_arch: std::env::consts::ARCH.to_string(),
        started_at_ms: chrono::Utc::now().timestamp_millis(),
    };
    let hello_json = wire_envelope(&hello)?;

    let (out_tx, out_rx) = mpsc::channel::<String>(256);
    let (in_tx, mut in_rx) = mpsc::channel::<String>(64);

    let session_shutdown = shutdown.clone();
    let session_task = tokio::spawn(async move {
        lab_session::run_loop(cfg, hello_json, out_rx, in_tx, session_shutdown).await;
    });

    // Forward journal events to the operator so the live state-journal
    // is visible in the tunnel viewer without the friend running
    // anything.
    spawn_journal_forwarder(out_tx.clone());

    let mut upload = UploadState::new();
    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            msg = in_rx.recv() => {
                let Some(raw) = msg else { break };
                match serde_json::from_str::<LabCmd>(&raw) {
                    Ok(cmd) => {
                        if let Err(e) = dispatch(cmd, &mut upload, &out_tx).await {
                            let _ = send(&out_tx, &LabEvent::Error {
                                id: None,
                                message: format!("{e:#}"),
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!("lab-mode: ignoring unrecognized frame ({e}): {raw}");
                    }
                }
            }
        }
    }

    drop(out_tx);
    let _ = session_task.await;
    Ok(())
}

// ---------- inbound command shapes ----------

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LabCmd {
    UploadUf2 {
        id: String,
        b64: String,
        #[serde(default)]
        chunk_index: u32,
        #[serde(default = "default_total_chunks")]
        total_chunks: u32,
    },
    Flash {
        id: String,
    },
    ForceBootsel {
        id: String,
    },
    Doctor {
        id: String,
    },
    Bundle {
        id: String,
    },
    WifiSet {
        id: String,
        ssid: String,
        password: String,
    },
    PullLog {
        id: String,
    },
    State {
        id: String,
    },
}

fn default_total_chunks() -> u32 {
    1
}

// ---------- outbound event shapes ----------

#[derive(Serialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
enum LabEvent {
    Hello {
        bridge_version: String,
        host_os: String,
        host_arch: String,
        started_at_ms: i64,
    },
    Journal {
        category: String,
        message: String,
    },
    UploadProgress {
        id: String,
        received_chunks: u32,
        total_chunks: u32,
        bytes: u64,
    },
    UploadComplete {
        id: String,
        path: String,
        size: u64,
        sha256: String,
    },
    FlashStage {
        id: String,
        stage: String,
        detail: String,
    },
    FlashDone {
        id: String,
        ok: bool,
        board: Option<String>,
        bytes_written: usize,
        wait_seconds: u64,
        rebooted_during_copy: bool,
        error: Option<String>,
    },
    BootselResult {
        id: String,
        method: String,
        ok: bool,
        detail: Option<String>,
    },
    DoctorResult {
        id: String,
        checks: Vec<DoctorCheckBody>,
    },
    BundleProgress {
        id: String,
        stage: String,
    },
    BundleDone {
        id: String,
        ok: bool,
        zip_path: Option<String>,
        manifest_json: Option<String>,
        error: Option<String>,
    },
    FileChunk {
        id: String,
        seq: u32,
        b64: String,
    },
    FileEof {
        id: String,
        total_chunks: u32,
        total_bytes: u64,
    },
    WifiResult {
        id: String,
        ok: bool,
        detail: Option<String>,
    },
    PullLogResult {
        id: String,
        ok: bool,
        log_text: Option<String>,
        lost_bytes: u32,
        detail: Option<String>,
    },
    StateSnapshot {
        id: String,
        bridge_version: String,
        last_pico: Option<serde_json::Value>,
        setup_complete: bool,
        last_upload: Option<UploadStateBody>,
    },
    Error {
        id: Option<String>,
        message: String,
    },
}

#[derive(Serialize)]
struct DoctorCheckBody {
    name: String,
    status: String,
    message: String,
    hint: Option<String>,
    elapsed_ms: u128,
}

#[derive(Serialize)]
struct UploadStateBody {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize)]
struct WireFrame<'a> {
    ts: i64,
    #[serde(flatten)]
    body: &'a LabEvent,
}

fn wire_envelope(ev: &LabEvent) -> Result<String> {
    let frame = WireFrame {
        ts: chrono::Utc::now().timestamp_millis(),
        body: ev,
    };
    Ok(serde_json::to_string(&frame)?)
}

fn send(out: &mpsc::Sender<String>, ev: &LabEvent) -> Result<()> {
    let msg = wire_envelope(ev)?;
    out.try_send(msg)
        .map_err(|e| anyhow::anyhow!("outbox send failed: {e}"))?;
    Ok(())
}

fn spawn_journal_forwarder(out: mpsc::Sender<String>) {
    use crate::journal;
    tokio::spawn(async move {
        let mut rx = journal::subscribe();
        loop {
            match rx.recv().await {
                Ok(entry) => {
                    let _ = send(
                        &out,
                        &LabEvent::Journal {
                            category: entry.category,
                            message: entry.message,
                        },
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Some events dropped; keep going.
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

// ---------- upload state ----------

struct UploadState {
    /// Path the next chunk will append to. Stays the same across an
    /// upload sequence; cleared (and the file truncated) when chunk
    /// `chunk_index == 0` arrives.
    path: PathBuf,
    received_chunks: u32,
    expected_chunks: u32,
    bytes: u64,
    hasher: Sha256,
    complete_sha256: Option<String>,
}

impl UploadState {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(UPLOAD_FILENAME),
            received_chunks: 0,
            expected_chunks: 0,
            bytes: 0,
            hasher: Sha256::new(),
            complete_sha256: None,
        }
    }

    /// Final UF2 path (only meaningful after `complete_sha256` is Some).
    fn finished_path(&self) -> Option<&Path> {
        if self.complete_sha256.is_some() {
            Some(&self.path)
        } else {
            None
        }
    }
}

// ---------- dispatch ----------

async fn dispatch(cmd: LabCmd, upload: &mut UploadState, out: &mpsc::Sender<String>) -> Result<()> {
    match cmd {
        LabCmd::UploadUf2 {
            id,
            b64,
            chunk_index,
            total_chunks,
        } => handle_upload(id, b64, chunk_index, total_chunks, upload, out).await,
        LabCmd::Flash { id } => handle_flash(id, upload, out).await,
        LabCmd::ForceBootsel { id } => handle_force_bootsel(id, out).await,
        LabCmd::Doctor { id } => handle_doctor(id, out).await,
        LabCmd::Bundle { id } => handle_bundle(id, out).await,
        LabCmd::WifiSet { id, ssid, password } => handle_wifi_set(id, ssid, password, out).await,
        LabCmd::PullLog { id } => handle_pull_log(id, out).await,
        LabCmd::State { id } => handle_state(id, upload, out).await,
    }
}

async fn handle_upload(
    id: String,
    b64: String,
    chunk_index: u32,
    total_chunks: u32,
    upload: &mut UploadState,
    out: &mpsc::Sender<String>,
) -> Result<()> {
    let bytes = B64
        .decode(b64.as_bytes())
        .context("decode upload_uf2 base64 payload")?;

    if chunk_index == 0 {
        // Fresh upload: truncate any prior file and reset hasher.
        upload.received_chunks = 0;
        upload.expected_chunks = total_chunks.max(1);
        upload.bytes = 0;
        upload.hasher = Sha256::new();
        upload.complete_sha256 = None;
        tokio::fs::write(&upload.path, &bytes)
            .await
            .with_context(|| format!("write {}", upload.path.display()))?;
    } else {
        use tokio::io::AsyncWriteExt;
        let mut f = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&upload.path)
            .await
            .with_context(|| format!("append to {}", upload.path.display()))?;
        f.write_all(&bytes).await.context("append chunk bytes")?;
    }
    upload.hasher.update(&bytes);
    upload.received_chunks = upload.received_chunks.saturating_add(1);
    upload.bytes = upload.bytes.saturating_add(bytes.len() as u64);

    let _ = send(
        out,
        &LabEvent::UploadProgress {
            id: id.clone(),
            received_chunks: upload.received_chunks,
            total_chunks: upload.expected_chunks,
            bytes: upload.bytes,
        },
    );

    if upload.received_chunks >= upload.expected_chunks {
        let digest = upload.hasher.clone().finalize();
        let sha = hex_lower(&digest);
        upload.complete_sha256 = Some(sha.clone());
        let _ = send(
            out,
            &LabEvent::UploadComplete {
                id,
                path: upload.path.display().to_string(),
                size: upload.bytes,
                sha256: sha,
            },
        );
    }
    Ok(())
}

async fn handle_flash(id: String, upload: &UploadState, out: &mpsc::Sender<String>) -> Result<()> {
    let Some(uf2) = upload.finished_path() else {
        let _ = send(
            out,
            &LabEvent::FlashDone {
                id,
                ok: false,
                board: None,
                bytes_written: 0,
                wait_seconds: 0,
                rebooted_during_copy: false,
                error: Some("no completed upload_uf2 in this session".into()),
            },
        );
        return Ok(());
    };

    let _ = send(
        out,
        &LabEvent::FlashStage {
            id: id.clone(),
            stage: "waiting_bootsel".into(),
            detail: "scanning for RPI-RP2 / RP2350 drive (60 s timeout)".into(),
        },
    );

    match cmd_flash::flash_uf2_to_bootsel(uf2, Duration::from_secs(60)).await {
        Ok(outcome) => {
            let _ = send(
                out,
                &LabEvent::FlashDone {
                    id,
                    ok: true,
                    board: Some(outcome.board.label().to_string()),
                    bytes_written: outcome.bytes_written,
                    wait_seconds: outcome.wait_seconds,
                    rebooted_during_copy: outcome.rebooted_during_copy,
                    error: None,
                },
            );
        }
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::FlashDone {
                    id,
                    ok: false,
                    board: None,
                    bytes_written: 0,
                    wait_seconds: 0,
                    rebooted_during_copy: false,
                    error: Some(format!("{e:#}")),
                },
            );
        }
    }
    Ok(())
}

async fn handle_force_bootsel(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    // Try CDC first if a setup-mode port is enumerated.
    if let Ok(port) = cdc::find_setup_port() {
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let mut p = cdc::PicoSetup::open_named(&port)?;
            p.reboot_to_bootsel()
        })
        .await
        .context("join CDC reboot task")?;
        match result {
            Ok(()) => {
                let _ = send(
                    out,
                    &LabEvent::BootselResult {
                        id,
                        method: "cdc".into(),
                        ok: true,
                        detail: None,
                    },
                );
                return Ok(());
            }
            Err(e) => {
                tracing::info!("lab: CDC reboot_to_bootsel failed: {e:#} -- trying UDP");
            }
        }
    }

    // Then UDP against the last known peer.
    if let Ok(cfg) = config::load() {
        if let Some(ip) = cfg.last_pico.as_ref().and_then(|p| p.last_ip.clone()) {
            let peer: std::net::SocketAddr = format!("{ip}:{}", protocol::PORT)
                .parse()
                .with_context(|| format!("config last_pico.last_ip `{ip}` did not parse as IP"))?;
            match network::send_reboot_to_bootsel(peer, Duration::from_secs(2)).await {
                Ok(()) => {
                    let _ = send(
                        out,
                        &LabEvent::BootselResult {
                            id,
                            method: "udp".into(),
                            ok: true,
                            detail: Some(format!("via {peer}")),
                        },
                    );
                    return Ok(());
                }
                Err(e) => {
                    tracing::info!("lab: UDP reboot_to_bootsel failed: {e:#} -- trying picotool");
                }
            }
        }
    }

    // Final fallback: picotool. Works as long as the USB controller is
    // alive, even if firmware is wedged.
    match picotool_force_reboot().await {
        Ok(detail) => {
            let _ = send(
                out,
                &LabEvent::BootselResult {
                    id,
                    method: "picotool".into(),
                    ok: true,
                    detail: Some(detail),
                },
            );
        }
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::BootselResult {
                    id,
                    method: "picotool".into(),
                    ok: false,
                    detail: Some(format!("{e:#}")),
                },
            );
        }
    }
    Ok(())
}

async fn picotool_force_reboot() -> Result<String> {
    let bin = locate_picotool().context("locate picotool.exe")?;
    let output = tokio::process::Command::new(&bin)
        .args(["reboot", "-u", "-f"])
        .output()
        .await
        .with_context(|| format!("spawn {}", bin.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        anyhow::bail!(
            "picotool exited {}: stdout=`{stdout}` stderr=`{stderr}`",
            output.status,
        );
    }
    Ok(format!("picotool reboot -u -f: {stdout}"))
}

fn locate_picotool() -> Result<PathBuf> {
    // Side-by-side with the bridge executable, then in the system PATH.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(if cfg!(windows) {
                "picotool.exe"
            } else {
                "picotool"
            });
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    Ok(PathBuf::from(if cfg!(windows) {
        "picotool.exe"
    } else {
        "picotool"
    }))
}

async fn handle_doctor(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    let outcomes = cmd_doctor::run_all_checks().await;
    let checks = outcomes
        .into_iter()
        .map(|o| DoctorCheckBody {
            name: o.name.to_string(),
            status: o.result.status().to_string(),
            message: o.result.message().to_string(),
            hint: o.result.hint().map(|s| s.to_string()),
            elapsed_ms: o.elapsed_ms,
        })
        .collect();
    let _ = send(out, &LabEvent::DoctorResult { id, checks });
    Ok(())
}

async fn handle_bundle(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let zip_path = std::env::temp_dir().join(format!("couchlink-bundle-{stamp}.zip"));
    let _ = send(
        out,
        &LabEvent::BundleProgress {
            id: id.clone(),
            stage: "capturing".into(),
        },
    );
    match cmd_bundle::build_bundle(zip_path).await {
        Ok(summary) => {
            let _ = send(
                out,
                &LabEvent::BundleDone {
                    id: id.clone(),
                    ok: true,
                    zip_path: Some(summary.zip_path.display().to_string()),
                    manifest_json: Some(summary.manifest_json),
                    error: None,
                },
            );
            // Stream the zip back in 32 KiB chunks so the operator gets the
            // file without having to read it off the host's disk.
            stream_file(&summary.zip_path, &id, out).await?;
        }
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::BundleDone {
                    id,
                    ok: false,
                    zip_path: None,
                    manifest_json: None,
                    error: Some(format!("{e:#}")),
                },
            );
        }
    }
    Ok(())
}

async fn stream_file(path: &Path, id: &str, out: &mpsc::Sender<String>) -> Result<()> {
    use tokio::io::AsyncReadExt;
    const CHUNK: usize = 32 * 1024;
    let mut f = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open {} for streaming", path.display()))?;
    let mut buf = vec![0u8; CHUNK];
    let mut seq: u32 = 0;
    let mut total: u64 = 0;
    loop {
        let n = f.read(&mut buf).await.context("read chunk")?;
        if n == 0 {
            break;
        }
        let _ = send(
            out,
            &LabEvent::FileChunk {
                id: id.to_string(),
                seq,
                b64: B64.encode(&buf[..n]),
            },
        );
        seq = seq.saturating_add(1);
        total = total.saturating_add(n as u64);
    }
    let _ = send(
        out,
        &LabEvent::FileEof {
            id: id.to_string(),
            total_chunks: seq,
            total_bytes: total,
        },
    );
    Ok(())
}

async fn handle_wifi_set(
    id: String,
    ssid: String,
    password: String,
    out: &mpsc::Sender<String>,
) -> Result<()> {
    let port = match cdc::find_setup_port() {
        Ok(p) => p,
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::WifiResult {
                    id,
                    ok: false,
                    detail: Some(format!("no setup-mode Pico found: {e:#}")),
                },
            );
            return Ok(());
        }
    };

    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let _ = pico.hello()?;
        let mut pass_buf = password;
        let rc = pico.set_wifi(&ssid, &mut pass_buf);
        // pass_buf is zeroized by set_wifi on success; defense-in-depth
        // here for the error path.
        use zeroize::Zeroize;
        pass_buf.zeroize();
        rc?;
        pico.reboot_to_run()
    })
    .await
    .context("join wifi_set task")?;

    match result {
        Ok(()) => {
            let _ = send(
                out,
                &LabEvent::WifiResult {
                    id,
                    ok: true,
                    detail: None,
                },
            );
        }
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::WifiResult {
                    id,
                    ok: false,
                    detail: Some(format!("{e:#}")),
                },
            );
        }
    }
    Ok(())
}

async fn handle_pull_log(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    use crate::protocol::LogChunk;
    use std::collections::BTreeMap;
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;

    let Some(ip) = config::load()
        .ok()
        .and_then(|c| c.last_pico.and_then(|p| p.last_ip))
    else {
        let _ = send(
            out,
            &LabEvent::PullLogResult {
                id,
                ok: false,
                log_text: None,
                lost_bytes: 0,
                detail: Some("no last_pico.last_ip in config".into()),
            },
        );
        return Ok(());
    };
    let peer: SocketAddr = match format!("{ip}:{}", protocol::PORT).parse() {
        Ok(a) => a,
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::PullLogResult {
                    id,
                    ok: false,
                    log_text: None,
                    lost_bytes: 0,
                    detail: Some(format!("config last_ip `{ip}` did not parse: {e}")),
                },
            );
            return Ok(());
        }
    };

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            let _ = send(
                out,
                &LabEvent::PullLogResult {
                    id,
                    ok: false,
                    log_text: None,
                    lost_bytes: 0,
                    detail: Some(format!("bind: {e}")),
                },
            );
            return Ok(());
        }
    };
    let req = protocol::encode_get_log(0);
    if let Err(e) = socket.send_to(&req, peer).await {
        let _ = send(
            out,
            &LabEvent::PullLogResult {
                id,
                ok: false,
                log_text: None,
                lost_bytes: 0,
                detail: Some(format!("send GET_LOG: {e}")),
            },
        );
        return Ok(());
    }

    let mut chunks: BTreeMap<u8, LogChunk> = BTreeMap::new();
    let mut got_last = false;
    let mut buf = [0u8; 1024];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
    while !got_last {
        let remaining = match deadline.checked_duration_since(tokio::time::Instant::now()) {
            Some(d) => d,
            None => break,
        };
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if from != peer {
                    continue;
                }
                match LogChunk::decode(&buf[..n]) {
                    Ok(c) => {
                        if c.is_last() {
                            got_last = true;
                        }
                        chunks.insert(c.chunk_index, c);
                    }
                    Err(e) => {
                        tracing::debug!("lab: discarded malformed log chunk: {e}");
                    }
                }
            }
            Ok(Err(e)) => {
                let _ = send(
                    out,
                    &LabEvent::PullLogResult {
                        id,
                        ok: false,
                        log_text: None,
                        lost_bytes: 0,
                        detail: Some(format!("recv: {e}")),
                    },
                );
                return Ok(());
            }
            Err(_) => break,
        }
    }

    let lost_bytes = chunks.values().next().map(|c| c.lost_bytes).unwrap_or(0);
    let mut body: Vec<u8> = Vec::new();
    for c in chunks.values() {
        body.extend_from_slice(&c.payload);
    }
    let log_text = String::from_utf8_lossy(&body).into_owned();
    let _ = send(
        out,
        &LabEvent::PullLogResult {
            id,
            ok: !chunks.is_empty(),
            log_text: Some(log_text),
            lost_bytes,
            detail: if got_last {
                None
            } else {
                Some("incomplete (no LAST_CHUNK flag seen)".into())
            },
        },
    );
    Ok(())
}

async fn handle_state(id: String, upload: &UploadState, out: &mpsc::Sender<String>) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let last_pico = cfg
        .last_pico
        .as_ref()
        .and_then(|p| serde_json::to_value(p).ok());
    let last_upload = upload.complete_sha256.as_ref().map(|sha| UploadStateBody {
        path: upload.path.display().to_string(),
        size: upload.bytes,
        sha256: sha.clone(),
    });
    let _ = send(
        out,
        &LabEvent::StateSnapshot {
            id,
            bridge_version: env!("CARGO_PKG_VERSION").to_string(),
            last_pico,
            setup_complete: cfg.setup_complete,
            last_upload,
        },
    );
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[((b >> 4) & 0xF) as usize] as char);
        s.push(HEX[(b & 0xF) as usize] as char);
    }
    s
}
const HEX: &[u8; 16] = b"0123456789abcdef";
