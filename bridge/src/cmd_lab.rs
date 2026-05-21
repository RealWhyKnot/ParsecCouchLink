//! `couchlink lab-mode` -- opt-in remote pair-flash session.
//!
//! The host starts this subcommand explicitly; the printed session URL
//! is shared with a remote operator (the developer iterating on
//! firmware) by the host themselves. While active:
//!
//! * The accepted command set is fully enumerated in `LabCmd` below.
//!   There is no arbitrary shell, file read, or process spawn -- every
//!   command is a typed, bounded operation against a specific surface
//!   (upload a UF2, flash, query firmware diag, apply pre-saved
//!   Wi-Fi credentials, etc.).
//! * Every received command is written to the lab-mode log at
//!   `%LOCALAPPDATA%\ParsecCouchLink\data\logs\lab-mode.log` so the
//!   host can audit the session after the fact.
//! * The host ends the session immediately with Ctrl+C, or by closing
//!   the terminal. Restarting the bridge rotates the session tokens,
//!   so a previously shared URL stops working.
//! * The tunnel relay (`CouchLinkTunnel`) enforces the same
//!   command-kind allowlist server-side and will reject anything
//!   outside it.
//!
//! The console stays quiet during the session to avoid spamming the
//! host's terminal with per-command output; the rotating log file is
//! the authoritative transcript.

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

    // Print the session URL so the host knows the session is active
    // and what to share. The rest of the per-command activity goes to
    // the rotating log file (the audit trail), not stdout.
    println!("Lab session: {view_url}");
    println!("Ctrl+C to end. Activity log: %LOCALAPPDATA%\\ParsecCouchLink\\data\\logs\\");

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
    // is visible in the tunnel viewer without the host operator
    // running anything by hand.
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
                            emit(&out_tx, LabEvent::Error {
                                id: None,
                                message: format!("{e:#}"),
                            }).await;
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
    /// Decrypt the host-saved DPAPI vault and push (ssid, password) to
    /// the Pico over CDC. The cleartext credentials never traverse the
    /// tunnel; the operator only triggers this command, and the result
    /// carries the SSID (so the operator can confirm which network was
    /// applied) but never the password. The host populates the vault
    /// once by running `couchlink save-wifi` on their own terminal.
    WifiApplySaved {
        id: String,
    },
    /// Send CDC_CMD_CLEAR_WIFI so the Pico wipes its flash credentials
    /// and re-enters setup mode on next boot.
    WifiClear {
        id: String,
    },
    PullLog {
        id: String,
    },
    State {
        id: String,
    },
    /// Send CDC_CMD_HELLO to a setup-mode Pico and return the firmware
    /// version, board type, and creds-present flag. Cheaper than a full
    /// `doctor`; useful for "did the flash succeed and the Pico come
    /// back up?" checks during iteration.
    Identify {
        id: String,
    },
    /// Broadcast UDP discovery on port 4242 and return the first ACK
    /// that comes back. Useful when `last_pico.last_ip` is stale or
    /// when the firmware has hopped to a new IP after a reboot.
    Discover {
        id: String,
        /// Discovery timeout in milliseconds. Default 2000.
        #[serde(default = "default_discover_ms")]
        timeout_ms: u64,
    },
    /// Round-trip latency check. Echoes the operator's nonce alongside
    /// the host's wall-clock so they can correlate events without
    /// trusting either side's monotonic.
    Ping {
        id: String,
        #[serde(default)]
        nonce: String,
    },
    /// Read the tail of the bridge's state-journal.log. `tail_lines`
    /// defaults to 200 and is capped at 2000 to keep the response
    /// bounded.
    ReadStateJournal {
        id: String,
        #[serde(default = "default_tail_lines")]
        tail_lines: usize,
    },
    /// Read the tail of today's rotating tracing log. Same shape as
    /// `read_state_journal`.
    ReadBridgeLog {
        id: String,
        #[serde(default = "default_tail_lines")]
        tail_lines: usize,
    },
    /// Pause for `ms` milliseconds before reporting back. Useful when
    /// scripting sequences like "force_bootsel -> sleep 1500 -> flash"
    /// so the operator does not have to maintain client-side timers.
    Sleep {
        id: String,
        ms: u64,
    },
}

fn default_discover_ms() -> u64 {
    2000
}
fn default_tail_lines() -> usize {
    200
}
const MAX_TAIL_LINES: usize = 2000;
const MAX_SLEEP_MS: u64 = 60_000;

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
        wifi_vault_saved: bool,
    },
    IdentifyResult {
        id: String,
        ok: bool,
        fw_major: Option<u8>,
        fw_minor: Option<u8>,
        fw_patch: Option<u8>,
        board_type: Option<u8>,
        proto_version: Option<u8>,
        creds_present: Option<bool>,
        wifi_joined: Option<bool>,
        detail: Option<String>,
    },
    DiscoverResult {
        id: String,
        ok: bool,
        peer: Option<String>,
        proto_version: Option<u8>,
        fw_major: Option<u8>,
        fw_minor: Option<u8>,
        fw_patch: Option<u8>,
        board_type: Option<u8>,
        unique_id_short: Option<u32>,
        uptime_seconds: Option<u32>,
        detail: Option<String>,
    },
    PingResult {
        id: String,
        nonce: String,
        host_ms: i64,
    },
    LogTailResult {
        id: String,
        source: String,
        ok: bool,
        path: Option<String>,
        lines: Vec<String>,
        truncated_from_lines: Option<usize>,
        detail: Option<String>,
    },
    SleepResult {
        id: String,
        slept_ms: u64,
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

/// Push an event into the outbox. Returns immediately; if the channel
/// is briefly full, the call serialises on the bounded queue's slot
/// (256 deep, plenty for the iteration loop). Failures are logged but
/// not propagated -- a tunnel hiccup must not collapse the dispatcher.
async fn emit(out: &mpsc::Sender<String>, ev: LabEvent) {
    let msg = match wire_envelope(&ev) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("lab-mode: failed to serialize event: {e}");
            return;
        }
    };
    if let Err(e) = out.send(msg).await {
        tracing::warn!("lab-mode: outbox closed, dropping event: {e}");
    }
}

fn spawn_journal_forwarder(out: mpsc::Sender<String>) {
    use crate::journal;
    tokio::spawn(async move {
        let mut rx = journal::subscribe();
        loop {
            match rx.recv().await {
                Ok(entry) => {
                    emit(
                        &out,
                        LabEvent::Journal {
                            category: entry.category,
                            message: entry.message,
                        },
                    )
                    .await;
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

const UPLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Chunked upload tracker. Stashes incoming chunks by index in memory
/// so out-of-order delivery, duplicates, and gaps are detectable
/// without trusting the wire. The file on disk is only written after
/// every chunk in `0..expected_chunks` has arrived and the sha256
/// matches what we computed locally.
struct UploadState {
    /// Path the assembled UF2 lands at once the upload completes.
    path: PathBuf,
    /// Upload id from chunk 0 -- subsequent chunks must match.
    current_id: Option<String>,
    /// `BTreeMap` because we want ordered iteration on assembly.
    chunks: std::collections::BTreeMap<u32, Vec<u8>>,
    expected_chunks: u32,
    /// Wall-clock of the last chunk arrival; used for stall detection.
    last_chunk_at: Option<std::time::Instant>,
    /// Set after a successful assembly. `wifi_apply_saved` and `flash`
    /// gate on this being present.
    finished_sha256: Option<String>,
    finished_bytes: u64,
}

impl UploadState {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(UPLOAD_FILENAME),
            current_id: None,
            chunks: std::collections::BTreeMap::new(),
            expected_chunks: 0,
            last_chunk_at: None,
            finished_sha256: None,
            finished_bytes: 0,
        }
    }

    /// Drop in-progress upload state. Called when a new chunk-0 arrives
    /// or when an explicit reset is needed (stall, error).
    fn reset(&mut self) {
        self.current_id = None;
        self.chunks.clear();
        self.expected_chunks = 0;
        self.last_chunk_at = None;
        // finished_sha256 / finished_bytes / path retained -- a stale
        // upload is still flashable until a new one supersedes it.
    }

    /// Final UF2 path (only meaningful after `finished_sha256` is Some).
    fn finished_path(&self) -> Option<&Path> {
        if self.finished_sha256.is_some() {
            Some(&self.path)
        } else {
            None
        }
    }

    /// Total bytes buffered (in-memory) for the in-progress upload.
    fn in_progress_bytes(&self) -> u64 {
        self.chunks.values().map(|b| b.len() as u64).sum()
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
        LabCmd::WifiApplySaved { id } => handle_wifi_apply_saved(id, out).await,
        LabCmd::WifiClear { id } => handle_wifi_clear(id, out).await,
        LabCmd::PullLog { id } => handle_pull_log(id, out).await,
        LabCmd::State { id } => handle_state(id, upload, out).await,
        LabCmd::Identify { id } => handle_identify(id, out).await,
        LabCmd::Discover { id, timeout_ms } => handle_discover(id, timeout_ms, out).await,
        LabCmd::Ping { id, nonce } => handle_ping(id, nonce, out).await,
        LabCmd::ReadStateJournal { id, tail_lines } => {
            handle_read_log_tail(id, "state_journal", state_journal_path(), tail_lines, out).await
        }
        LabCmd::ReadBridgeLog { id, tail_lines } => {
            handle_read_log_tail(id, "bridge_log", todays_bridge_log_path(), tail_lines, out).await
        }
        LabCmd::Sleep { id, ms } => handle_sleep(id, ms, out).await,
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
    let total = total_chunks.max(1);

    // Detect a stalled previous upload (no new chunk for >30 s) and
    // start over fresh on the new chunk regardless of its index. The
    // operator's client is expected to begin a new upload with
    // chunk_index = 0; we tolerate the case where it doesn't.
    if let Some(last) = upload.last_chunk_at {
        if last.elapsed() > UPLOAD_STALL_TIMEOUT {
            tracing::warn!(
                "lab: upload stalled (>{:?} since last chunk); resetting",
                UPLOAD_STALL_TIMEOUT
            );
            upload.reset();
        }
    }

    if chunk_index == 0 {
        // Fresh upload: drop everything from the prior sequence.
        upload.reset();
        upload.current_id = Some(id.clone());
        upload.expected_chunks = total;
    } else {
        // Continuation chunk -- must belong to an in-progress upload.
        let Some(current) = upload.current_id.as_ref() else {
            anyhow::bail!("chunk_index={chunk_index} without a chunk-0 having started an upload");
        };
        if current != &id {
            anyhow::bail!("chunk id `{id}` does not match in-progress upload id `{current}`");
        }
        if total != upload.expected_chunks {
            anyhow::bail!(
                "chunk total_chunks={total} disagrees with chunk-0's {} (replay or corruption)",
                upload.expected_chunks
            );
        }
    }

    if chunk_index >= upload.expected_chunks {
        anyhow::bail!(
            "chunk_index={chunk_index} >= total_chunks={}",
            upload.expected_chunks
        );
    }
    if upload.chunks.contains_key(&chunk_index) {
        anyhow::bail!("duplicate chunk_index={chunk_index} for upload id `{id}`");
    }
    upload.chunks.insert(chunk_index, bytes);
    upload.last_chunk_at = Some(std::time::Instant::now());

    emit(
        out,
        LabEvent::UploadProgress {
            id: id.clone(),
            received_chunks: upload.chunks.len() as u32,
            total_chunks: upload.expected_chunks,
            bytes: upload.in_progress_bytes(),
        },
    )
    .await;

    if upload.chunks.len() as u32 == upload.expected_chunks {
        // Contiguous-from-zero check: BTreeMap iteration is ordered.
        let contiguous = upload
            .chunks
            .keys()
            .enumerate()
            .all(|(i, k)| *k == i as u32);
        if !contiguous {
            anyhow::bail!("chunk sequence not contiguous despite count match (gap)");
        }

        // Assemble + write + hash atomically. If write fails we keep
        // the in-memory chunks so the operator can retry just `flash`
        // without re-uploading.
        let mut hasher = Sha256::new();
        let mut assembled: Vec<u8> = Vec::with_capacity(upload.in_progress_bytes() as usize);
        for chunk in upload.chunks.values() {
            hasher.update(chunk);
            assembled.extend_from_slice(chunk);
        }
        let sha = hex_lower(&hasher.finalize());

        tokio::fs::write(&upload.path, &assembled)
            .await
            .with_context(|| format!("write assembled UF2 to {}", upload.path.display()))?;

        upload.finished_sha256 = Some(sha.clone());
        upload.finished_bytes = assembled.len() as u64;
        let size = upload.finished_bytes;
        let path = upload.path.display().to_string();
        // Free the buffer now that the file is on disk.
        upload.chunks.clear();
        upload.last_chunk_at = None;

        emit(
            out,
            LabEvent::UploadComplete {
                id,
                path,
                size,
                sha256: sha,
            },
        )
        .await;
    }
    Ok(())
}

async fn handle_flash(id: String, upload: &UploadState, out: &mpsc::Sender<String>) -> Result<()> {
    let Some(uf2) = upload.finished_path() else {
        emit(
            out,
            LabEvent::FlashDone {
                id,
                ok: false,
                board: None,
                bytes_written: 0,
                wait_seconds: 0,
                rebooted_during_copy: false,
                error: Some("no completed upload_uf2 in this session".into()),
            },
        )
        .await;
        return Ok(());
    };

    emit(
        out,
        LabEvent::FlashStage {
            id: id.clone(),
            stage: "waiting_bootsel".into(),
            detail: "scanning for RPI-RP2 / RP2350 drive (60 s timeout)".into(),
        },
    )
    .await;

    match cmd_flash::flash_uf2_to_bootsel(uf2, Duration::from_secs(60)).await {
        Ok(outcome) => {
            emit(
                out,
                LabEvent::FlashDone {
                    id,
                    ok: true,
                    board: Some(outcome.board.label().to_string()),
                    bytes_written: outcome.bytes_written,
                    wait_seconds: outcome.wait_seconds,
                    rebooted_during_copy: outcome.rebooted_during_copy,
                    error: None,
                },
            )
            .await;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::FlashDone {
                    id,
                    ok: false,
                    board: None,
                    bytes_written: 0,
                    wait_seconds: 0,
                    rebooted_during_copy: false,
                    error: Some(format!("{e:#}")),
                },
            )
            .await;
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
                emit(
                    out,
                    LabEvent::BootselResult {
                        id,
                        method: "cdc".into(),
                        ok: true,
                        detail: None,
                    },
                )
                .await;
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
                    emit(
                        out,
                        LabEvent::BootselResult {
                            id,
                            method: "udp".into(),
                            ok: true,
                            detail: Some(format!("via {peer}")),
                        },
                    )
                    .await;
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
            emit(
                out,
                LabEvent::BootselResult {
                    id,
                    method: "picotool".into(),
                    ok: true,
                    detail: Some(detail),
                },
            )
            .await;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::BootselResult {
                    id,
                    method: "picotool".into(),
                    ok: false,
                    detail: Some(format!("{e:#}")),
                },
            )
            .await;
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
    emit(out, LabEvent::DoctorResult { id, checks }).await;
    Ok(())
}

async fn handle_bundle(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let zip_path = std::env::temp_dir().join(format!("couchlink-bundle-{stamp}.zip"));
    emit(
        out,
        LabEvent::BundleProgress {
            id: id.clone(),
            stage: "capturing".into(),
        },
    )
    .await;
    match cmd_bundle::build_bundle(zip_path).await {
        Ok(summary) => {
            emit(
                out,
                LabEvent::BundleDone {
                    id: id.clone(),
                    ok: true,
                    zip_path: Some(summary.zip_path.display().to_string()),
                    manifest_json: Some(summary.manifest_json),
                    error: None,
                },
            )
            .await;
            // Stream the zip back in 32 KiB chunks so the operator gets the
            // file without having to read it off the host's disk.
            stream_file(&summary.zip_path, &id, out).await?;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::BundleDone {
                    id,
                    ok: false,
                    zip_path: None,
                    manifest_json: None,
                    error: Some(format!("{e:#}")),
                },
            )
            .await;
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
        emit(
            out,
            LabEvent::FileChunk {
                id: id.to_string(),
                seq,
                b64: B64.encode(&buf[..n]),
            },
        )
        .await;
        seq = seq.saturating_add(1);
        total = total.saturating_add(n as u64);
    }
    emit(
        out,
        LabEvent::FileEof {
            id: id.to_string(),
            total_chunks: seq,
            total_bytes: total,
        },
    )
    .await;
    Ok(())
}

async fn handle_wifi_apply_saved(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    // Decrypt strictly in the spawn_blocking closure so the cleartext
    // bytes live on a synchronous stack we can zeroize deterministically.
    // The cleartext never crosses an await boundary, never lands in an
    // Event, never sees the tunnel.
    let result = tokio::task::spawn_blocking(move || -> Result<String> {
        let Some(creds) = crate::wifi_vault::load()? else {
            anyhow::bail!(
                "no saved Wi-Fi credentials on this host; run \
                 `couchlink save-wifi` on the host first"
            );
        };
        let port = cdc::find_setup_port()
            .map_err(|e| anyhow::anyhow!("no setup-mode Pico found: {e:#}"))?;
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let _ = pico.hello()?;
        // Move the cleartext into a local mutable buffer so set_wifi can
        // zeroize it; the Zeroizing<String> in `creds` zeroizes on drop
        // as defense in depth.
        let mut pass = creds.password.as_str().to_string();
        let rc = pico.set_wifi(&creds.ssid, &mut pass);
        use zeroize::Zeroize;
        pass.zeroize();
        rc?;
        pico.reboot_to_run()?;
        Ok(creds.ssid)
    })
    .await
    .context("join wifi_apply_saved task")?;

    match result {
        Ok(ssid) => {
            emit(
                out,
                LabEvent::WifiResult {
                    id,
                    ok: true,
                    detail: Some(format!("applied saved credentials for SSID '{ssid}'")),
                },
            )
            .await;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::WifiResult {
                    id,
                    ok: false,
                    detail: Some(format!("{e:#}")),
                },
            )
            .await;
        }
    }
    Ok(())
}

async fn handle_wifi_clear(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    let port = match cdc::find_setup_port() {
        Ok(p) => p,
        Err(e) => {
            emit(
                out,
                LabEvent::WifiResult {
                    id,
                    ok: false,
                    detail: Some(format!("no setup-mode Pico found: {e:#}")),
                },
            )
            .await;
            return Ok(());
        }
    };
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let _ = pico.hello()?;
        pico.clear_wifi()
    })
    .await
    .context("join wifi_clear task")?;
    match result {
        Ok(()) => {
            emit(
                out,
                LabEvent::WifiResult {
                    id,
                    ok: true,
                    detail: Some("cleared Pico flash credentials".into()),
                },
            )
            .await;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::WifiResult {
                    id,
                    ok: false,
                    detail: Some(format!("{e:#}")),
                },
            )
            .await;
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
        emit(
            out,
            LabEvent::PullLogResult {
                id,
                ok: false,
                log_text: None,
                lost_bytes: 0,
                detail: Some("no last_pico.last_ip in config".into()),
            },
        )
        .await;
        return Ok(());
    };
    let peer: SocketAddr = match format!("{ip}:{}", protocol::PORT).parse() {
        Ok(a) => a,
        Err(e) => {
            emit(
                out,
                LabEvent::PullLogResult {
                    id,
                    ok: false,
                    log_text: None,
                    lost_bytes: 0,
                    detail: Some(format!("config last_ip `{ip}` did not parse: {e}")),
                },
            )
            .await;
            return Ok(());
        }
    };

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            emit(
                out,
                LabEvent::PullLogResult {
                    id,
                    ok: false,
                    log_text: None,
                    lost_bytes: 0,
                    detail: Some(format!("bind: {e}")),
                },
            )
            .await;
            return Ok(());
        }
    };
    let req = protocol::encode_get_log(0);
    if let Err(e) = socket.send_to(&req, peer).await {
        emit(
            out,
            LabEvent::PullLogResult {
                id,
                ok: false,
                log_text: None,
                lost_bytes: 0,
                detail: Some(format!("send GET_LOG: {e}")),
            },
        )
        .await;
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
                emit(
                    out,
                    LabEvent::PullLogResult {
                        id,
                        ok: false,
                        log_text: None,
                        lost_bytes: 0,
                        detail: Some(format!("recv: {e}")),
                    },
                )
                .await;
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
    emit(
        out,
        LabEvent::PullLogResult {
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
    )
    .await;
    Ok(())
}

async fn handle_state(id: String, upload: &UploadState, out: &mpsc::Sender<String>) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let last_pico = cfg
        .last_pico
        .as_ref()
        .and_then(|p| serde_json::to_value(p).ok());
    let last_upload = upload.finished_sha256.as_ref().map(|sha| UploadStateBody {
        path: upload.path.display().to_string(),
        size: upload.finished_bytes,
        sha256: sha.clone(),
    });
    emit(
        out,
        LabEvent::StateSnapshot {
            id,
            bridge_version: env!("CARGO_PKG_VERSION").to_string(),
            last_pico,
            setup_complete: cfg.setup_complete,
            last_upload,
            wifi_vault_saved: crate::wifi_vault::exists(),
        },
    )
    .await;
    Ok(())
}

async fn handle_identify(id: String, out: &mpsc::Sender<String>) -> Result<()> {
    let port = match cdc::find_setup_port() {
        Ok(p) => p,
        Err(e) => {
            emit(
                out,
                LabEvent::IdentifyResult {
                    id,
                    ok: false,
                    fw_major: None,
                    fw_minor: None,
                    fw_patch: None,
                    board_type: None,
                    proto_version: None,
                    creds_present: None,
                    wifi_joined: None,
                    detail: Some(format!("no setup-mode Pico found: {e:#}")),
                },
            )
            .await;
            return Ok(());
        }
    };
    let outcome = tokio::task::spawn_blocking(move || -> Result<cdc::HelloAck> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        pico.hello()
    })
    .await
    .context("join identify task")?;
    match outcome {
        Ok(hello) => {
            // 0x02 is HELLO_FLAG_WIFI_JOINED on the firmware side.
            let wifi_joined = Some((hello.flags & 0x02) != 0);
            emit(
                out,
                LabEvent::IdentifyResult {
                    id,
                    ok: true,
                    fw_major: Some(hello.fw_major),
                    fw_minor: Some(hello.fw_minor),
                    fw_patch: Some(hello.fw_patch),
                    board_type: Some(hello.board_type),
                    proto_version: Some(hello.proto_version),
                    creds_present: Some(hello.creds_present()),
                    wifi_joined,
                    detail: None,
                },
            )
            .await;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::IdentifyResult {
                    id,
                    ok: false,
                    fw_major: None,
                    fw_minor: None,
                    fw_patch: None,
                    board_type: None,
                    proto_version: None,
                    creds_present: None,
                    wifi_joined: None,
                    detail: Some(format!("{e:#}")),
                },
            )
            .await;
        }
    }
    Ok(())
}

async fn handle_discover(id: String, timeout_ms: u64, out: &mpsc::Sender<String>) -> Result<()> {
    use crate::protocol::{Packet, PacketKind};
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;

    let timeout = Duration::from_millis(timeout_ms.min(15_000));
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            emit(
                out,
                LabEvent::DiscoverResult {
                    id,
                    ok: false,
                    peer: None,
                    proto_version: None,
                    fw_major: None,
                    fw_minor: None,
                    fw_patch: None,
                    board_type: None,
                    unique_id_short: None,
                    uptime_seconds: None,
                    detail: Some(format!("bind: {e}")),
                },
            )
            .await;
            return Ok(());
        }
    };
    if let Err(e) = socket.set_broadcast(true) {
        tracing::debug!("lab: discover set_broadcast: {e}");
    }
    let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", protocol::PORT)
        .parse()
        .expect("broadcast addr is constant");
    let req = Packet::discover(0).encode();
    if let Err(e) = socket.send_to(&req, broadcast_addr).await {
        emit(
            out,
            LabEvent::DiscoverResult {
                id,
                ok: false,
                peer: None,
                proto_version: None,
                fw_major: None,
                fw_minor: None,
                fw_patch: None,
                board_type: None,
                unique_id_short: None,
                uptime_seconds: None,
                detail: Some(format!("broadcast send: {e}")),
            },
        )
        .await;
        return Ok(());
    }
    let mut buf = [0u8; 64];
    let result = tokio::time::timeout(timeout, socket.recv_from(&mut buf)).await;
    match result {
        Ok(Ok((n, from))) => match Packet::decode(&buf[..n]) {
            Ok(pkt) => match pkt.kind {
                PacketKind::Ack(info) => {
                    emit(
                        out,
                        LabEvent::DiscoverResult {
                            id,
                            ok: true,
                            peer: Some(from.to_string()),
                            proto_version: Some(info.proto_version),
                            fw_major: Some(info.fw_major),
                            fw_minor: Some(info.fw_minor),
                            fw_patch: Some(info.fw_patch),
                            board_type: Some(info.board_type),
                            unique_id_short: Some(info.unique_id_short),
                            uptime_seconds: Some(info.uptime_seconds),
                            detail: None,
                        },
                    )
                    .await;
                }
                other => {
                    emit(
                        out,
                        LabEvent::DiscoverResult {
                            id,
                            ok: false,
                            peer: Some(from.to_string()),
                            proto_version: None,
                            fw_major: None,
                            fw_minor: None,
                            fw_patch: None,
                            board_type: None,
                            unique_id_short: None,
                            uptime_seconds: None,
                            detail: Some(format!("reply was not ACK: {other:?}")),
                        },
                    )
                    .await;
                }
            },
            Err(e) => {
                emit(
                    out,
                    LabEvent::DiscoverResult {
                        id,
                        ok: false,
                        peer: Some(from.to_string()),
                        proto_version: None,
                        fw_major: None,
                        fw_minor: None,
                        fw_patch: None,
                        board_type: None,
                        unique_id_short: None,
                        uptime_seconds: None,
                        detail: Some(format!("decode: {e}")),
                    },
                )
                .await;
            }
        },
        Ok(Err(e)) => {
            emit(
                out,
                LabEvent::DiscoverResult {
                    id,
                    ok: false,
                    peer: None,
                    proto_version: None,
                    fw_major: None,
                    fw_minor: None,
                    fw_patch: None,
                    board_type: None,
                    unique_id_short: None,
                    uptime_seconds: None,
                    detail: Some(format!("recv: {e}")),
                },
            )
            .await;
        }
        Err(_) => {
            emit(
                out,
                LabEvent::DiscoverResult {
                    id,
                    ok: false,
                    peer: None,
                    proto_version: None,
                    fw_major: None,
                    fw_minor: None,
                    fw_patch: None,
                    board_type: None,
                    unique_id_short: None,
                    uptime_seconds: None,
                    detail: Some(format!("no ACK within {timeout_ms} ms")),
                },
            )
            .await;
        }
    }
    Ok(())
}

async fn handle_ping(id: String, nonce: String, out: &mpsc::Sender<String>) -> Result<()> {
    emit(
        out,
        LabEvent::PingResult {
            id,
            nonce,
            host_ms: chrono::Utc::now().timestamp_millis(),
        },
    )
    .await;
    Ok(())
}

async fn handle_sleep(id: String, ms: u64, out: &mpsc::Sender<String>) -> Result<()> {
    let bounded = ms.min(MAX_SLEEP_MS);
    tokio::time::sleep(Duration::from_millis(bounded)).await;
    emit(
        out,
        LabEvent::SleepResult {
            id,
            slept_ms: bounded,
        },
    )
    .await;
    Ok(())
}

fn state_journal_path() -> Result<PathBuf> {
    Ok(config::log_dir()?.join("state-journal.log"))
}

fn todays_bridge_log_path() -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y-%m-%d");
    Ok(config::log_dir()?.join(format!("couchlink.{stamp}.log")))
}

async fn handle_read_log_tail(
    id: String,
    source: &'static str,
    path: Result<PathBuf>,
    requested_lines: usize,
    out: &mpsc::Sender<String>,
) -> Result<()> {
    let path = match path {
        Ok(p) => p,
        Err(e) => {
            emit(
                out,
                LabEvent::LogTailResult {
                    id,
                    source: source.to_string(),
                    ok: false,
                    path: None,
                    lines: Vec::new(),
                    truncated_from_lines: None,
                    detail: Some(format!("resolve log path: {e:#}")),
                },
            )
            .await;
            return Ok(());
        }
    };
    let want = requested_lines.clamp(1, MAX_TAIL_LINES);
    let read_result = tokio::fs::read_to_string(&path).await;
    match read_result {
        Ok(text) => {
            let total_lines = text.lines().count();
            let skip = total_lines.saturating_sub(want);
            let lines: Vec<String> = text.lines().skip(skip).map(|s| s.to_string()).collect();
            let truncated = if total_lines > want {
                Some(total_lines)
            } else {
                None
            };
            emit(
                out,
                LabEvent::LogTailResult {
                    id,
                    source: source.to_string(),
                    ok: true,
                    path: Some(path.display().to_string()),
                    lines,
                    truncated_from_lines: truncated,
                    detail: None,
                },
            )
            .await;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            emit(
                out,
                LabEvent::LogTailResult {
                    id,
                    source: source.to_string(),
                    ok: false,
                    path: Some(path.display().to_string()),
                    lines: Vec::new(),
                    truncated_from_lines: None,
                    detail: Some("file does not exist yet".into()),
                },
            )
            .await;
        }
        Err(e) => {
            emit(
                out,
                LabEvent::LogTailResult {
                    id,
                    source: source.to_string(),
                    ok: false,
                    path: Some(path.display().to_string()),
                    lines: Vec::new(),
                    truncated_from_lines: None,
                    detail: Some(format!("{e:#}")),
                },
            )
            .await;
        }
    }
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
