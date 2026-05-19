//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! crash files, Pico diag log, and a manifest.json with non-sensitive system
//! info. Intended to be attached to a bug report.
//!
//! NEVER include Wi-Fi credentials. The Pico stores them and the bridge
//! never reads them. SSID is also omitted by default to be safe.

use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Local;
use serde::Serialize;
use tokio::net::UdpSocket;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::protocol::{self, LogChunk, Packet, PacketKind, ACK_FLAG_LOG_CHUNK_SUPPORTED};
use crate::{cdc, config};

pub async fn run(output: Option<PathBuf>) -> Result<()> {
    let diag = capture_pico_diag().await;
    let pico_diag_captured = matches!(diag, DiagOutcome::Captured { .. });
    let pico_diag_lost_bytes = diag.lost_bytes();
    let pico_diag_outcome = diag.discriminant_str().to_string();
    let pico_diag_source = diag.source_str().map(|s| s.to_string());

    let crash_files = collect_crash_file_names();
    let setup_transcripts = collect_setup_transcript_names();

    let usb_devices = capture_usb_devices().await;
    let usb_devices_captured = usb_devices.is_some();
    let usb_capture_method = usb_devices
        .as_ref()
        .map(|(_, m)| (*m).to_string())
        .unwrap_or_else(|| "none".to_string());

    let system_info = build_system_info().await;

    let manifest = build_manifest(
        pico_diag_captured,
        pico_diag_lost_bytes,
        &pico_diag_outcome,
        pico_diag_source.as_deref(),
        usb_devices_captured,
        &usb_capture_method,
        &crash_files,
        &setup_transcripts,
    )
    .await?;
    let manifest_json = serde_json::to_string_pretty(&manifest)?;

    let doctor_text = run_doctor_silently().await;

    let out_path = output.unwrap_or_else(|| {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        PathBuf::from(format!("couchlink-bundle-{stamp}.zip"))
    });
    let f = std::fs::File::create(&out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    let mut zip = ZipWriter::new(f);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("manifest.json", opts)?;
    zip.write_all(manifest_json.as_bytes())?;

    zip.start_file("doctor.txt", opts)?;
    zip.write_all(doctor_text.as_bytes())?;

    // Always write pico-diag.txt. The body is a self-narrating stub
    // when capture failed; the per-variant message names the failing
    // step so the bundle is actionable without reading the bridge log.
    let pico_diag_body = diag.stub_text();
    zip.start_file("pico-diag.txt", opts)?;
    zip.write_all(pico_diag_body.as_bytes())?;

    // system-info.txt: always present. Captures the Windows build,
    // couchlink version, last-known Pico identity, short hostname.
    zip.start_file("system-info.txt", opts)?;
    zip.write_all(system_info.as_bytes())?;

    // usb-devices.txt: pnputil dump if available (Windows 10 1903+),
    // otherwise a SetupAPI-via-serialport fallback so the bundle always
    // has *something* describing the USB topology at bundle time.
    if let Some((text, method)) = usb_devices.as_ref() {
        zip.start_file("usb-devices.txt", opts)?;
        zip.write_all(format!("# capture method: {method}\n\n").as_bytes())?;
        zip.write_all(text.as_bytes())?;
    } else {
        zip.start_file("usb-devices.txt", opts)?;
        zip.write_all(
            b"(USB device enumeration unavailable: pnputil is missing AND the serialport \
              fallback returned an error. Run `pnputil /enum-devices /class USB /connected` \
              manually and attach the output.)",
        )?;
    }

    // Crash files from crash_dir(). Errors at each step are logged at
    // debug -- a locked-by-antivirus crash dir, a permissions change,
    // or a vanished file used to be invisible.
    if let Ok(crash_dir) = config::crash_dir() {
        if crash_dir.is_dir() {
            match std::fs::read_dir(&crash_dir) {
                Ok(entries) => {
                    for entry in entries {
                        let entry = match entry {
                            Ok(e) => e,
                            Err(e) => {
                                tracing::debug!(
                                    "bundle: could not read entry in {}: {e}",
                                    crash_dir.display()
                                );
                                continue;
                            }
                        };
                        let p = entry.path();
                        if !p.is_file() {
                            continue;
                        }
                        let Some(name) = p.file_name() else { continue };
                        match std::fs::read(&p) {
                            Ok(bytes) => {
                                zip.start_file(
                                    format!("crashes/{}", name.to_string_lossy()),
                                    opts,
                                )?;
                                zip.write_all(&bytes)?;
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "bundle: could not read crash file {}: {e}",
                                    p.display(),
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "bundle: could not enumerate crash dir {}: {e}",
                        crash_dir.display()
                    );
                }
            }
        }
    }

    // Logs: last 3 couchlink.*.log (bridge, written by tracing-appender's
    // daily rotation as couchlink.YYYY-MM-DD.log) and last 3 setup-*.log
    // (PowerShell transcripts from setup.ps1 / the wrapper scripts).
    // The bridge prefix was previously "couchlink-" which never matched
    // tracing-appender's actual filename format and silently produced
    // bundles with zero bridge logs.
    if let Ok(log_dir) = config::log_dir() {
        bundle_log_prefix(&log_dir, "couchlink.", &mut zip, opts)?;
        bundle_log_prefix(&log_dir, "setup-", &mut zip, opts)?;
    }

    zip.finish()?;

    let issue_url = crate::support::issue_url();
    println!("Wrote {}", out_path.display());
    println!("  manifest.json + doctor.txt + bridge logs");
    if pico_diag_captured {
        match pico_diag_source.as_deref() {
            Some(src) => println!("  pico-diag.txt: captured via {src}"),
            None => println!("  pico-diag.txt: captured"),
        }
    } else {
        println!("  pico-diag.txt: not captured ({pico_diag_outcome}) -- see the file for details");
    }
    if crash_files.is_empty() {
        println!("  crashes/: none");
    } else {
        println!("  crashes/: {} files", crash_files.len());
    }
    if setup_transcripts.is_empty() {
        println!("  setup transcripts: none");
    } else {
        println!("  setup transcripts: {} files", setup_transcripts.len());
    }
    println!();
    println!("Wi-Fi password and SSID are not included. Safe to share.");
    println!();
    println!("Report this bundle at: {issue_url}");

    // Offer to open the issues page if stdin is a terminal.
    if console::Term::stdout().is_term() {
        let url = issue_url.clone();
        let open_it = tokio::task::spawn_blocking(move || {
            dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Open the issues page in your browser now?")
                .default(false)
                .interact()
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);

        if open_it {
            #[cfg(windows)]
            {
                let _ = tokio::process::Command::new("cmd")
                    .args(["/C", "start", "", url.as_str()])
                    .status()
                    .await;
            }
            #[cfg(not(windows))]
            let _ = url;
        }
    }

    Ok(())
}

fn bundle_log_prefix(
    log_dir: &std::path::Path,
    prefix: &str,
    zip: &mut ZipWriter<std::fs::File>,
    opts: SimpleFileOptions,
) -> Result<()> {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "bundle: could not enumerate log dir {}: {e}",
                log_dir.display()
            );
            return Ok(());
        }
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                let p = e.path();
                let ok = p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(prefix) && n.ends_with(".log"))
                        .unwrap_or(false);
                if ok {
                    paths.push(p);
                }
            }
            Err(e) => {
                tracing::debug!("bundle: could not read entry in {}: {e}", log_dir.display());
            }
        }
    }
    paths.sort();
    let take = 3.min(paths.len());
    let recent = &paths[paths.len() - take..];
    for p in recent {
        let Some(name) = p.file_name() else { continue };
        match std::fs::read(p) {
            Ok(bytes) => {
                zip.start_file(format!("logs/{}", name.to_string_lossy()), opts)?;
                zip.write_all(&bytes)?;
            }
            Err(e) => {
                tracing::debug!("bundle: could not read log file {}: {e}", p.display());
            }
        }
    }
    Ok(())
}

fn collect_crash_file_names() -> Vec<String> {
    let Ok(crash_dir) = config::crash_dir() else {
        return vec![];
    };
    if !crash_dir.is_dir() {
        return vec![];
    }
    let entries = match std::fs::read_dir(&crash_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "bundle: could not enumerate crash dir {}: {e}",
                crash_dir.display()
            );
            return vec![];
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                if e.path().is_file() {
                    if let Ok(name) = e.file_name().into_string() {
                        out.push(name);
                    }
                }
            }
            Err(e) => tracing::debug!("bundle: skip crash entry: {e}"),
        }
    }
    out
}

fn collect_setup_transcript_names() -> Vec<String> {
    let Ok(log_dir) = config::log_dir() else {
        return vec![];
    };
    let entries = match std::fs::read_dir(&log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "bundle: could not enumerate log dir {}: {e}",
                log_dir.display()
            );
            return vec![];
        }
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                if e.path().is_file() {
                    if let Ok(name) = e.file_name().into_string() {
                        if name.starts_with("setup-") && name.ends_with(".log") {
                            names.push(name);
                        }
                    }
                }
            }
            Err(e) => tracing::debug!("bundle: skip log entry: {e}"),
        }
    }
    names.sort();
    let take = 3.min(names.len());
    names[names.len() - take..].to_vec()
}

#[derive(Serialize)]
struct Manifest {
    bridge_version: &'static str,
    protocol_version: u8,
    cdc_protocol_version: u8,
    generated_at: String,
    os: String,
    windows_version: Option<String>,
    last_pico: Option<config::PicoIdentity>,
    setup_complete: bool,
    config_path: String,
    log_dir: String,
    report_url: String,
    pico_diag_captured: bool,
    pico_diag_lost_bytes: u32,
    pico_diag_outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pico_diag_source: Option<String>,
    usb_devices_captured: bool,
    usb_capture_method: String,
    crash_files: Vec<String>,
    setup_transcripts: Vec<String>,
    notes: Vec<&'static str>,
}

#[allow(clippy::too_many_arguments)]
async fn build_manifest(
    pico_diag_captured: bool,
    pico_diag_lost_bytes: u32,
    pico_diag_outcome: &str,
    pico_diag_source: Option<&str>,
    usb_devices_captured: bool,
    usb_capture_method: &str,
    crash_files: &[String],
    setup_transcripts: &[String],
) -> Result<Manifest> {
    let cfg = config::load().unwrap_or_default();
    Ok(Manifest {
        bridge_version: env!("CARGO_PKG_VERSION"),
        protocol_version: crate::protocol::PROTO_VERSION,
        cdc_protocol_version: crate::cdc::PROTO_VERSION,
        generated_at: Local::now().to_rfc3339(),
        os: std::env::consts::OS.to_string(),
        windows_version: windows_version().await,
        last_pico: cfg.last_pico,
        setup_complete: cfg.setup_complete,
        config_path: config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        log_dir: config::log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        report_url: crate::support::issue_url(),
        pico_diag_captured,
        pico_diag_lost_bytes,
        pico_diag_outcome: pico_diag_outcome.to_string(),
        pico_diag_source: pico_diag_source.map(|s| s.to_string()),
        usb_devices_captured,
        usb_capture_method: usb_capture_method.to_string(),
        crash_files: crash_files.to_vec(),
        setup_transcripts: setup_transcripts.to_vec(),
        notes: vec![
            "Wi-Fi credentials are NOT included.",
            "SSID is NOT included.",
            "Logs are filtered to the last 3 rotated files per prefix.",
        ],
    })
}

#[cfg(windows)]
async fn windows_version() -> Option<String> {
    let out = tokio::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(not(windows))]
async fn windows_version() -> Option<String> {
    None
}

/// Source the Pico diag log came from (or would have come from, if
/// capture failed). The bundle stub names this so an operator can tell
/// at a glance whether the failure was on the USB-CDC path or the
/// run-mode UDP path.
#[derive(Clone, Debug)]
enum DiagSource {
    SetupCdc,
    RunUdp { peer: SocketAddr },
}

impl DiagSource {
    /// Short discriminant for the manifest's `pico_diag_source` field.
    fn as_str(&self) -> &'static str {
        match self {
            DiagSource::SetupCdc => "setup-cdc",
            DiagSource::RunUdp { .. } => "run-udp",
        }
    }

    /// Human-readable description for the pico-diag.txt stub header,
    /// including the peer address when known.
    fn describe(&self) -> String {
        match self {
            DiagSource::SetupCdc => "setup-mode USB-CDC".to_string(),
            DiagSource::RunUdp { peer } => format!("run-mode UDP from {peer}"),
        }
    }
}

/// The result of an attempt to capture the firmware's diag-log ring,
/// rich enough that the bundle's pico-diag.txt stub can name the
/// specific step that failed (port enum / port open / HELLO write /
/// HELLO read / GET_LOG over UDP / etc.). Replaces the previous
/// `Option<(String, u32)>` which collapsed every failure mode into
/// the same generic stub.
#[derive(Clone, Debug)]
enum DiagOutcome {
    Captured {
        source: DiagSource,
        text: String,
        lost: u32,
    },
    Empty {
        source: DiagSource,
    },
    NoSetupPort,
    SetupOpenFailed {
        error: String,
    },
    SetupProbeFailed {
        port: String,
        step: &'static str,
        elapsed_ms: u128,
        bytes_received: usize,
        rx_first_32_hex: String,
        error: String,
    },
    NoLastPicoInConfig,
    UdpDiscoveryFailed {
        reason: String,
    },
    UdpProbeFailed {
        peer: SocketAddr,
        step: &'static str,
        elapsed_ms: u128,
        chunks_received: u16,
        error: String,
    },
    UdpUnsupported {
        peer: SocketAddr,
    },
}

impl DiagOutcome {
    fn discriminant_str(&self) -> &'static str {
        match self {
            DiagOutcome::Captured { .. } => "captured",
            DiagOutcome::Empty { .. } => "empty",
            DiagOutcome::NoSetupPort => "no_setup_port",
            DiagOutcome::SetupOpenFailed { .. } => "setup_open_failed",
            DiagOutcome::SetupProbeFailed { .. } => "setup_probe_failed",
            DiagOutcome::NoLastPicoInConfig => "no_last_pico_in_config",
            DiagOutcome::UdpDiscoveryFailed { .. } => "udp_discovery_failed",
            DiagOutcome::UdpProbeFailed { .. } => "udp_probe_failed",
            DiagOutcome::UdpUnsupported { .. } => "udp_unsupported",
        }
    }

    fn source_str(&self) -> Option<&'static str> {
        match self {
            DiagOutcome::Captured { source, .. } | DiagOutcome::Empty { source } => {
                Some(source.as_str())
            }
            _ => None,
        }
    }

    fn lost_bytes(&self) -> u32 {
        match self {
            DiagOutcome::Captured { lost, .. } => *lost,
            _ => 0,
        }
    }

    /// Body of pico-diag.txt for this outcome. The Captured/Empty paths
    /// produce the operator-facing log text; every failure variant
    /// names the specific step that broke so an operator can tell a
    /// "no Pico found" from a "port opened, HELLO timed out" from a
    /// "running Pico answered but doesn't speak the new protocol".
    fn stub_text(&self) -> String {
        match self {
            DiagOutcome::Captured { text, lost, source } => {
                let prefix = format!("--- captured via {}", source.describe());
                let prefix = if *lost > 0 {
                    format!(
                        "{prefix}; {lost} byte(s) dropped from the ring before this snapshot ---\n",
                    )
                } else {
                    format!("{prefix} ---\n")
                };
                format!("{prefix}{text}")
            }
            DiagOutcome::Empty { source } => format!(
                "(firmware diag ring was empty when bundle ran -- the Pico was \
                 reachable via {} but had no log entries. Usually the Pico \
                 rebooted between the failure and bundle. If the failure was \
                 at boot, re-run the failing command and immediately run bundle \
                 while the Pico is still in the same state.)",
                source.describe(),
            ),
            DiagOutcome::NoSetupPort => "(no setup-mode Pico found over USB-CDC \
                 -- nothing enumerated with VID 0x2E8A PID 0xCAF0. \
                 Bundle also has no last-known run-mode address in config to \
                 fall back to, OR the run-mode Pico did not answer. Re-plug \
                 the Pico while holding BOOTSEL, flash with `flash.ps1`, \
                 wait for it to come back as a setup-mode serial device, then \
                 run bundle. See logs/couchlink.*.log for what the bridge tried.)"
                .to_string(),
            DiagOutcome::SetupOpenFailed { error } => format!(
                "(setup-mode Pico was visible but the bridge could not open \
                 the serial port: {error}. Another app may be holding the COM \
                 port -- close any open serial terminals and re-run bundle. \
                 See logs/couchlink.*.log for the open call details.)",
            ),
            DiagOutcome::SetupProbeFailed {
                port,
                step,
                elapsed_ms,
                bytes_received,
                rx_first_32_hex,
                error,
            } => format!(
                "(setup-mode CDC port {port} opened but the HELLO probe failed \
                 at step `{step}` after {elapsed_ms} ms with {bytes_received} \
                 bytes received (first 32 = {rx_first_32_hex}). Error: {error}. \
                 Interpretation: `write` failure = the bridge could not send the \
                 HELLO frame to the firmware. `read` failure with 0 received \
                 bytes = the firmware enumerated USB but is not running its \
                 CDC poll loop (likely an early-boot fault or stuck init). \
                 `read` failure with non-zero received bytes = the firmware is \
                 mis-framing -- the hex above is what it actually put on the \
                 wire. `decode` failure = the frame arrived but was the wrong \
                 shape, indicating a protocol version mismatch.)"
            ),
            DiagOutcome::NoLastPicoInConfig => "(no setup-mode Pico found AND \
                 no last-known Pico recorded in config -- the bridge has \
                 nowhere to look on the LAN. Run `couchlink setup` to provision \
                 a Pico, or `couchlink doctor` to do a fresh discovery, then \
                 re-run bundle.)"
                .to_string(),
            DiagOutcome::UdpDiscoveryFailed { reason } => format!(
                "(no setup-mode Pico found; bundle attempted a run-mode UDP \
                 probe against the last known address but it did not answer: \
                 {reason}. The Pico may be off, on a different network, or \
                 the host firewall may be blocking UDP/4242. Run \
                 `couchlink doctor` to diagnose discovery, then re-run bundle.)"
            ),
            DiagOutcome::UdpProbeFailed {
                peer,
                step,
                elapsed_ms,
                chunks_received,
                error,
            } => format!(
                "(run-mode UDP probe against {peer} failed at step `{step}` \
                 after {elapsed_ms} ms with {chunks_received} chunk(s) received. \
                 Error: {error}. The Pico answered the initial discovery so \
                 it is alive on the LAN, but the CMD_GET_LOG path did not \
                 complete. See logs/couchlink.*.log for per-step details.)"
            ),
            DiagOutcome::UdpUnsupported { peer } => format!(
                "(run-mode Pico at {peer} answered discovery but does NOT \
                 advertise the LogChunk capability (ACK flags bit 0 clear). \
                 This firmware is older than v2026.5.16.6 and only exposes \
                 its diag ring through setup-mode USB-CDC. To capture diag \
                 from this Pico, hold BOOTSEL, plug it into this PC, flash \
                 `couchlink-picow.uf2` or `couchlink-pico2w.uf2`, wait for \
                 setup-mode serial enumeration, then re-run bundle. To get \
                 UDP-side diag in future, flash this Pico with a newer \
                 firmware.)"
            ),
        }
    }
}

/// Try setup-mode USB-CDC first, fall back to run-mode UDP probe.
async fn capture_pico_diag() -> DiagOutcome {
    let cdc_result = try_capture_setup_cdc().await;
    match cdc_result {
        DiagOutcome::NoSetupPort => {
            tracing::info!("bundle: no setup-mode CDC port, attempting run-mode UDP probe");
            try_capture_run_udp().await
        }
        other => other,
    }
}

/// Setup-mode CDC path. Distinguishes:
///   - find_setup_port() failed -> NoSetupPort
///   - port found, open failed -> SetupOpenFailed
///   - port + open OK, HELLO probe failed -> SetupProbeFailed (with step)
///   - HELLO OK, get_log_buffer failed -> SetupProbeFailed { step: "get_log_buffer" }
///   - get_log_buffer OK, payload empty -> Empty
///   - all OK with text -> Captured
async fn try_capture_setup_cdc() -> DiagOutcome {
    tokio::task::spawn_blocking(|| {
        let port = match cdc::find_setup_port() {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("bundle: find_setup_port: {e:#}");
                return DiagOutcome::NoSetupPort;
            }
        };
        tracing::info!("bundle: setup-mode CDC port at {port}");
        let mut pico = match cdc::PicoSetup::open_named(&port) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("bundle: setup-mode CDC open on {port} failed: {e:#}");
                return DiagOutcome::SetupOpenFailed {
                    error: format!("{e:#}"),
                };
            }
        };

        let probe = pico.hello_probe();
        if let Err(err) = probe.result.clone() {
            tracing::error!(
                "bundle: HELLO probe failed at step `{}` after {} ms (rx_bytes={}): {}",
                probe.step_reached.as_str(),
                probe.elapsed_ms,
                probe.bytes_received,
                err,
            );
            return DiagOutcome::SetupProbeFailed {
                port: probe.port,
                step: probe.step_reached.as_str(),
                elapsed_ms: probe.elapsed_ms,
                bytes_received: probe.bytes_received,
                rx_first_32_hex: probe.rx_first_32_hex,
                error: err,
            };
        }

        // HELLO ok; pull the diag ring.
        let log_start = Instant::now();
        match pico.get_log_buffer() {
            Ok((text, _lost)) if text.is_empty() => DiagOutcome::Empty {
                source: DiagSource::SetupCdc,
            },
            Ok((text, lost)) => DiagOutcome::Captured {
                source: DiagSource::SetupCdc,
                text,
                lost,
            },
            Err(e) => {
                tracing::error!(
                    "bundle: GET_LOG_BUFFER on {} failed after {} ms: {e:#}",
                    probe.port,
                    log_start.elapsed().as_millis(),
                );
                DiagOutcome::SetupProbeFailed {
                    port: probe.port,
                    step: "get_log_buffer",
                    elapsed_ms: log_start.elapsed().as_millis(),
                    bytes_received: 0,
                    rx_first_32_hex: "n/a".to_string(),
                    error: format!("{e:#}"),
                }
            }
        }
    })
    .await
    .unwrap_or_else(|join_err| DiagOutcome::SetupOpenFailed {
        error: format!("spawn_blocking task failed: {join_err}"),
    })
}

/// Run-mode UDP path. Uses the config's last-known Pico address as the
/// probe target. A fresh broadcast discovery is intentionally NOT used
/// here: the bundle is for a "something's wrong" context, the user
/// already has a `doctor` command for re-discovery, and unicast against
/// a known address is fast (no 1-second tick).
async fn try_capture_run_udp() -> DiagOutcome {
    let cfg = config::load().unwrap_or_default();
    let last_ip = match cfg.last_pico.as_ref().and_then(|p| p.last_ip.clone()) {
        Some(ip) => ip,
        None => {
            tracing::info!("bundle: no last_pico.last_ip in config; UDP probe not attempted");
            return DiagOutcome::NoLastPicoInConfig;
        }
    };
    let peer_addr: SocketAddr = match format!("{last_ip}:{}", protocol::PORT).parse() {
        Ok(a) => a,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("config last_ip `{last_ip}` did not parse: {e}"),
            };
        }
    };
    tracing::info!("bundle: UDP probe target {peer_addr}");

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("bind: {e}"),
            };
        }
    };

    // Step 1: unicast Discover, wait for an Ack, read the capability flag.
    let ack_started = Instant::now();
    let ack_packet = match unicast_for_ack(&socket, peer_addr, Duration::from_secs(2)).await {
        Ok(p) => p,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("ack probe: {e}"),
            };
        }
    };
    tracing::info!(
        "bundle: UDP ack from {peer_addr} after {} ms, flags=0x{:02X}",
        ack_started.elapsed().as_millis(),
        ack_packet.flags,
    );

    if ack_packet.flags & ACK_FLAG_LOG_CHUNK_SUPPORTED == 0 {
        return DiagOutcome::UdpUnsupported { peer: peer_addr };
    }

    // Step 2: send GET_LOG, collect chunks until LAST_CHUNK or timeout.
    let started = Instant::now();
    let req = protocol::encode_get_log(0);
    if let Err(e) = socket.send_to(&req, peer_addr).await {
        return DiagOutcome::UdpProbeFailed {
            peer: peer_addr,
            step: "send_get_log",
            elapsed_ms: started.elapsed().as_millis(),
            chunks_received: 0,
            error: format!("{e}"),
        };
    }

    let mut chunks: BTreeMap<u8, LogChunk> = BTreeMap::new();
    let mut got_last = false;
    let mut buf = [0u8; 1024];
    // Overall deadline gives the firmware time to drain the ring. With 16
    // chunks of 256 bytes each, even a slow per-chunk cadence completes
    // well inside this budget.
    let deadline = started + Duration::from_millis(1500);
    while !got_last {
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(d) if !d.is_zero() => d,
            _ => break,
        };
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if from != peer_addr {
                    tracing::debug!("bundle: UDP probe dropped pkt from {from}");
                    continue;
                }
                match LogChunk::decode(&buf[..n]) {
                    Ok(chunk) => {
                        tracing::debug!(
                            "bundle: log chunk idx={} len={} last={}",
                            chunk.chunk_index,
                            chunk.payload.len(),
                            chunk.is_last(),
                        );
                        if chunk.is_last() {
                            got_last = true;
                        }
                        chunks.insert(chunk.chunk_index, chunk);
                    }
                    Err(e) => {
                        tracing::debug!("bundle: UDP probe garbled chunk from {from}: {e}");
                    }
                }
            }
            Ok(Err(e)) => {
                return DiagOutcome::UdpProbeFailed {
                    peer: peer_addr,
                    step: "recv_chunk",
                    elapsed_ms: started.elapsed().as_millis(),
                    chunks_received: chunks.len() as u16,
                    error: format!("{e}"),
                };
            }
            Err(_) => break, // overall timeout
        }
    }

    if chunks.is_empty() {
        return DiagOutcome::UdpProbeFailed {
            peer: peer_addr,
            step: "wait_for_chunks",
            elapsed_ms: started.elapsed().as_millis(),
            chunks_received: 0,
            error: "no LogChunk datagrams received before the 1500 ms deadline".to_string(),
        };
    }

    let lost = chunks.get(&0).map(|c| c.lost_bytes).unwrap_or(0);
    let mut text_bytes: Vec<u8> = Vec::new();
    for c in chunks.values() {
        text_bytes.extend_from_slice(&c.payload);
    }
    let text = String::from_utf8_lossy(&text_bytes).into_owned();
    if text.is_empty() {
        DiagOutcome::Empty {
            source: DiagSource::RunUdp { peer: peer_addr },
        }
    } else {
        DiagOutcome::Captured {
            source: DiagSource::RunUdp { peer: peer_addr },
            text,
            lost,
        }
    }
}

async fn unicast_for_ack(
    socket: &UdpSocket,
    peer: SocketAddr,
    timeout: Duration,
) -> Result<Packet, String> {
    let req = Packet::discover(0).encode();
    socket
        .send_to(&req, peer)
        .await
        .map_err(|e| format!("send: {e}"))?;
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| format!("no ack within {} ms", timeout.as_millis()))?;
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if from != peer {
                    continue;
                }
                match Packet::decode(&buf[..n]) {
                    Ok(pkt) if matches!(pkt.kind, PacketKind::Ack(_)) => return Ok(pkt),
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::debug!("bundle: discarded non-ack from {from}: {e}");
                    }
                }
            }
            Ok(Err(e)) => return Err(format!("recv: {e}")),
            Err(_) => {
                return Err(format!("no ack within {} ms", timeout.as_millis()));
            }
        }
    }
}

/// Capture a USB device enumeration. On Windows we first try
/// `pnputil /enum-devices /class USB /connected` (Win10 1903+); on
/// older Windows or non-Windows hosts we fall back to a serialport
/// list dump that at least names every USB serial device with VID,
/// PID, manufacturer, and serial number. Returns `(text, method)`
/// or `None` if both paths failed.
async fn capture_usb_devices() -> Option<(String, &'static str)> {
    #[cfg(windows)]
    {
        if let Some(text) = pnputil_enum_usb().await {
            return Some((text, "pnputil"));
        }
        tracing::debug!("bundle: pnputil enum failed, falling back to serialport list");
    }
    let text = tokio::task::spawn_blocking(serialport_list_dump)
        .await
        .ok()??;
    Some((text, "serialport-fallback"))
}

#[cfg(windows)]
async fn pnputil_enum_usb() -> Option<String> {
    let out = tokio::process::Command::new("pnputil.exe")
        .args(["/enum-devices", "/class", "USB", "/connected"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        // Some Win10 1903-1909 builds reject /connected unelevated.
        // Try the looser /enum-devices /class USB (no /connected) as a
        // last resort before declaring the path unusable.
        let fallback = tokio::process::Command::new("pnputil.exe")
            .args(["/enum-devices", "/class", "USB"])
            .output()
            .await
            .ok()?;
        if !fallback.status.success() {
            tracing::debug!(
                "bundle: pnputil returned non-zero (status={:?})",
                fallback.status.code()
            );
            return None;
        }
        return Some(String::from_utf8_lossy(&fallback.stdout).into_owned());
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn serialport_list_dump() -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    let mut out = String::new();
    out.push_str("# serialport::available_ports() fallback dump\n\n");
    if ports.is_empty() {
        out.push_str("(no serial ports found)\n");
    }
    for p in &ports {
        out.push_str(&format!("- {}\n", p.port_name));
        if let serialport::SerialPortType::UsbPort(info) = &p.port_type {
            out.push_str(&format!(
                "    VID=0x{:04X} PID=0x{:04X}\n",
                info.vid, info.pid,
            ));
            if let Some(s) = info.serial_number.as_deref() {
                out.push_str(&format!("    serial={s}\n"));
            }
            if let Some(s) = info.manufacturer.as_deref() {
                out.push_str(&format!("    manufacturer={s}\n"));
            }
            if let Some(s) = info.product.as_deref() {
                out.push_str(&format!("    product={s}\n"));
            }
        }
    }
    Some(out)
}

/// Always-present body for `bundle/system-info.txt`. Captures things
/// that can be checked without opening the Pico, so an issue reporter
/// has provenance even when the Pico is gone (lost cable, dead
/// firmware, hardware swap).
async fn build_system_info() -> String {
    let cfg = config::load().unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!("couchlink {}\n", env!("CARGO_PKG_VERSION"),));
    out.push_str(&format!("generated  {}\n", Local::now().to_rfc3339()));
    out.push_str(&format!("os         {}\n", std::env::consts::OS));
    if let Some(v) = windows_version().await {
        out.push_str(&format!("windows    {v}\n"));
    }
    out.push_str(&format!(
        "hostname   {}\n",
        hostname_short().unwrap_or_else(|| "(unknown)".into())
    ));
    out.push_str(&format!("setup-done {}\n", cfg.setup_complete));
    match &cfg.last_pico {
        Some(p) => {
            out.push_str(&format!(
                "last-pico  fw={}.{}.{} board=0x{:02X} unique-id-short=0x{:08X}\n",
                p.fw_major, p.fw_minor, p.fw_patch, p.board_type, p.unique_id_short,
            ));
            if let Some(ip) = p.last_ip.as_deref() {
                out.push_str(&format!("           last-ip={ip}\n"));
            }
            if let Some(n) = p.device_name.as_deref() {
                out.push_str(&format!("           device-name={n}\n"));
            }
        }
        None => out.push_str("last-pico  (none)\n"),
    }
    out.push_str(&format!(
        "config     {}\n",
        config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(unknown)".into())
    ));
    out.push_str(&format!(
        "logs       {}\n",
        config::log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(unknown)".into())
    ));
    out
}

fn hostname_short() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())?;
    Some(raw.split('.').next().unwrap_or(&raw).to_string())
}

async fn run_doctor_silently() -> String {
    // Doctor prints to stdout via println!; we don't capture that here.
    // For the bundle we instead run each check and format the result
    // ourselves into a plain-text report.
    use crate::cmd_doctor::*;
    let checks: Vec<(&str, BoxedCheck)> = vec![
        ("paths", Box::pin(check_paths())),
        ("xinput", Box::pin(check_xinput())),
        ("startup", Box::pin(check_startup_shortcut())),
        ("firewall", Box::pin(check_firewall())),
        ("wifi-band", Box::pin(check_24ghz_warning())),
        ("cdc", Box::pin(check_cdc())),
        ("discover", Box::pin(check_discover())),
    ];
    let mut out = String::new();
    out.push_str(&format!(
        "couchlink doctor (bundled, no terminal styling)\n  bridge v{}\n  time {}\n\n",
        env!("CARGO_PKG_VERSION"),
        chrono::Local::now().to_rfc3339(),
    ));
    for (name, fut) in checks {
        let res = fut.await;
        out.push_str(&format!("  [{:<10}] {}\n", name, format_result(&res)));
    }
    out
}

fn format_result(r: &crate::cmd_doctor::CheckResult) -> String {
    use crate::cmd_doctor::CheckResult::*;
    match r {
        Pass(m) => format!("PASS  {m}"),
        Warn(m) => format!("WARN  {m}"),
        Skip(m) => format!("SKIP  {m}"),
        Fail(m, h) => format!("FAIL  {m}\n              hint: {h}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer() -> SocketAddr {
        "10.0.0.24:4242".parse().unwrap()
    }

    /// Every variant has a distinct discriminant string. These strings
    /// ship in manifest.json and are likely to be grepped on by humans
    /// or future tooling, so stability matters.
    #[test]
    fn discriminant_strings_are_distinct() {
        let variants = [
            DiagOutcome::Captured {
                source: DiagSource::SetupCdc,
                text: "".into(),
                lost: 0,
            },
            DiagOutcome::Empty {
                source: DiagSource::SetupCdc,
            },
            DiagOutcome::NoSetupPort,
            DiagOutcome::SetupOpenFailed { error: "x".into() },
            DiagOutcome::SetupProbeFailed {
                port: "COM3".into(),
                step: "read",
                elapsed_ms: 0,
                bytes_received: 0,
                rx_first_32_hex: "none".into(),
                error: "x".into(),
            },
            DiagOutcome::NoLastPicoInConfig,
            DiagOutcome::UdpDiscoveryFailed { reason: "x".into() },
            DiagOutcome::UdpProbeFailed {
                peer: make_peer(),
                step: "send_get_log",
                elapsed_ms: 0,
                chunks_received: 0,
                error: "x".into(),
            },
            DiagOutcome::UdpUnsupported { peer: make_peer() },
        ];
        let mut seen = std::collections::HashSet::new();
        for v in &variants {
            let d = v.discriminant_str();
            assert!(seen.insert(d), "duplicate discriminant: {d}");
        }
        assert_eq!(seen.len(), variants.len());
    }

    #[test]
    fn captured_stub_includes_text_and_source() {
        let out = DiagOutcome::Captured {
            source: DiagSource::SetupCdc,
            text: "BOOT: hello".into(),
            lost: 0,
        };
        let stub = out.stub_text();
        assert!(
            stub.contains("setup-mode USB-CDC"),
            "missing source: {stub}"
        );
        assert!(stub.contains("BOOT: hello"), "missing text: {stub}");
    }

    #[test]
    fn captured_stub_flags_lost_bytes() {
        let out = DiagOutcome::Captured {
            source: DiagSource::RunUdp { peer: make_peer() },
            text: "tail-of-ring".into(),
            lost: 1234,
        };
        let stub = out.stub_text();
        assert!(
            stub.contains("1234 byte(s) dropped"),
            "missing lost: {stub}"
        );
        assert!(stub.contains("10.0.0.24:4242"), "missing peer: {stub}");
    }

    #[test]
    fn setup_probe_failed_names_step_and_bytes() {
        let out = DiagOutcome::SetupProbeFailed {
            port: "COM3".into(),
            step: "read",
            elapsed_ms: 3012,
            bytes_received: 0,
            rx_first_32_hex: "none".into(),
            error: "timed out".into(),
        };
        let stub = out.stub_text();
        assert!(stub.contains("COM3"));
        assert!(stub.contains("`read`"));
        assert!(stub.contains("3012 ms"));
        assert!(stub.contains("0 bytes received"));
        // Specifically check the operator-facing hint for the read+0 case
        // is present (this is the most common reproduction shape).
        assert!(stub.contains("not running its CDC poll loop"));
    }

    #[test]
    fn udp_unsupported_names_peer() {
        let out = DiagOutcome::UdpUnsupported { peer: make_peer() };
        let stub = out.stub_text();
        assert!(stub.contains("10.0.0.24:4242"));
        assert!(stub.contains("ACK flags bit 0 clear"));
    }

    #[test]
    fn udp_probe_failed_names_step_and_count() {
        let out = DiagOutcome::UdpProbeFailed {
            peer: make_peer(),
            step: "recv_chunk",
            elapsed_ms: 1500,
            chunks_received: 3,
            error: "lost peer".into(),
        };
        let stub = out.stub_text();
        assert!(stub.contains("10.0.0.24:4242"));
        assert!(stub.contains("`recv_chunk`"));
        assert!(stub.contains("3 chunk(s)"));
    }

    /// `source_str` returns the manifest-facing source for captured/empty
    /// outcomes only -- it is None for every failure variant.
    #[test]
    fn source_str_only_set_when_reachable() {
        assert_eq!(
            DiagOutcome::Captured {
                source: DiagSource::SetupCdc,
                text: "".into(),
                lost: 0,
            }
            .source_str(),
            Some("setup-cdc"),
        );
        assert_eq!(
            DiagOutcome::Empty {
                source: DiagSource::RunUdp { peer: make_peer() },
            }
            .source_str(),
            Some("run-udp"),
        );
        assert!(DiagOutcome::NoSetupPort.source_str().is_none());
        assert!(DiagOutcome::UdpUnsupported { peer: make_peer() }
            .source_str()
            .is_none());
    }
}
