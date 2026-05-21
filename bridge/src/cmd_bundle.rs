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
use crate::{cdc, config, journal};

/// Structured result of a bundle build. Returned by `build_bundle` so
/// callers (lab mode, future scripted flows) get a typed answer without
/// scraping the CLI's `println!` summary.
#[allow(dead_code)] // fields read by cmd_lab in a later commit
#[derive(Clone, Debug)]
pub struct BundleSummary {
    pub zip_path: PathBuf,
    pub manifest_json: String,
    pub pico_diag_captured: bool,
    pub pico_diag_outcome: String,
    pub pico_diag_source: Option<String>,
    pub crash_file_count: usize,
    pub setup_transcript_count: usize,
    pub pico_usb_enumerated: bool,
}

/// Build the support bundle zip. Captures diag, doctor, usb topology,
/// logs, and writes them to `out_path`. Returns a structured summary.
///
/// CLI-side prompts (open-issues-in-browser, summary printing) live in
/// `run`, not here -- this function is silent on stdout/stderr.
pub async fn build_bundle(out_path: PathBuf) -> Result<BundleSummary> {
    journal!("bundle", "run started");
    let diag = capture_pico_diag().await;
    journal!(
        "bundle",
        "diag capture outcome: {}",
        diag.discriminant_str()
    );
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

    let usb_events = capture_windows_usb_events().await;
    let usb_events_captured = usb_events.is_some();

    // Classify current Pico USB enumeration state from the pnputil output.
    // Used both in manifest.json and in the VendorNotFound stub text.
    let pico_enum_state = usb_devices
        .as_ref()
        .filter(|(_, m)| *m == "pnputil")
        .map(|(text, _)| classify_pico_enum(text))
        .unwrap_or(PicoEnumState::NotEnumerated);
    let pico_usb_enumerated = !matches!(pico_enum_state, PicoEnumState::NotEnumerated);
    let pico_usb_mode = match &pico_enum_state {
        PicoEnumState::NotEnumerated => None,
        PicoEnumState::EnumeratedSetupMode
        | PicoEnumState::EnumeratedButInterfaceUnclaimable { .. } => Some("setup".to_string()),
        PicoEnumState::EnumeratedRunMode => Some("run".to_string()),
    };

    let system_info = build_system_info().await;

    let manifest = build_manifest(
        pico_diag_captured,
        pico_diag_lost_bytes,
        &pico_diag_outcome,
        pico_diag_source.as_deref(),
        usb_devices_captured,
        &usb_capture_method,
        usb_events_captured,
        pico_usb_enumerated,
        pico_usb_mode.as_deref(),
        &crash_files,
        &setup_transcripts,
    )
    .await?;
    let manifest_json = serde_json::to_string_pretty(&manifest)?;

    let doctor_text = run_doctor_silently().await;

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
    // VendorNotFound is special: its stub text depends on pico_enum_state.
    let pico_diag_body = if matches!(diag, DiagOutcome::VendorNotFound) {
        vendor_not_found_stub_text(&pico_enum_state)
    } else {
        diag.stub_text()
    };
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

    // usb-events.txt: recent OS-level USB events from the Windows event
    // log. Catches the class of failure that pnputil can't show -- driver
    // bind failures, descriptor request timeouts, surprise removals --
    // because those events surface in the System log via the usbhub /
    // usbser / Kernel-PnP providers rather than in the pnputil snapshot.
    if let Some(text) = usb_events.as_ref() {
        zip.start_file("usb-events.txt", opts)?;
        zip.write_all(b"# Windows event log entries from the last 15 minutes\n")?;
        zip.write_all(b"# filtered to USB / usbhub / usbser / Kernel-PnP providers\n\n")?;
        zip.write_all(text.as_bytes())?;
    } else {
        zip.start_file("usb-events.txt", opts)?;
        zip.write_all(
            b"(Get-WinEvent returned no output -- either no recent USB events were \
              recorded, the Windows PowerShell event log cmdlet timed out, or the \
              capture script returned an error. This is not necessarily a problem; \
              uneventful enumeration leaves no trace.)",
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

    // State journal: short append-only timeline of bridge events. The
    // rotating log has full detail; the journal has the headlines.
    if let Some(jp) = journal::path() {
        if jp.is_file() {
            match std::fs::read(&jp) {
                Ok(bytes) => {
                    zip.start_file("state-journal.log", opts)?;
                    zip.write_all(&bytes)?;
                }
                Err(e) => {
                    tracing::debug!("bundle: could not read state journal: {e}");
                }
            }
        }
    }

    zip.finish()?;

    Ok(BundleSummary {
        zip_path: out_path,
        manifest_json,
        pico_diag_captured,
        pico_diag_outcome,
        pico_diag_source,
        crash_file_count: crash_files.len(),
        setup_transcript_count: setup_transcripts.len(),
        pico_usb_enumerated,
    })
}

pub async fn run(output: Option<PathBuf>) -> Result<()> {
    let out_path = output.unwrap_or_else(|| {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        PathBuf::from(format!("couchlink-bundle-{stamp}.zip"))
    });
    let summary = build_bundle(out_path).await?;

    let issue_url = crate::support::issue_url();
    println!("Wrote {}", summary.zip_path.display());
    println!("  manifest.json + doctor.txt + bridge logs");
    if summary.pico_diag_captured {
        match summary.pico_diag_source.as_deref() {
            Some(src) => println!("  pico-diag.txt: captured via {src}"),
            None => println!("  pico-diag.txt: captured"),
        }
    } else {
        println!(
            "  pico-diag.txt: not captured ({}) -- see the file for details",
            summary.pico_diag_outcome
        );
    }
    if summary.crash_file_count == 0 {
        println!("  crashes/: none");
    } else {
        println!("  crashes/: {} files", summary.crash_file_count);
    }
    if summary.setup_transcript_count == 0 {
        println!("  setup transcripts: none");
    } else {
        println!(
            "  setup transcripts: {} files",
            summary.setup_transcript_count
        );
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
    usb_events_captured: bool,
    pico_usb_enumerated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pico_usb_mode: Option<String>,
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
    usb_events_captured: bool,
    pico_usb_enumerated: bool,
    pico_usb_mode: Option<&str>,
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
        usb_events_captured,
        pico_usb_enumerated,
        pico_usb_mode: pico_usb_mode.map(|s| s.to_string()),
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
    VendorControl,
    RunUdp { peer: SocketAddr },
}

impl DiagSource {
    /// Short discriminant for the manifest's `pico_diag_source` field.
    fn as_str(&self) -> &'static str {
        match self {
            DiagSource::SetupCdc => "setup-cdc",
            DiagSource::VendorControl => "vendor-control",
            DiagSource::RunUdp { .. } => "run-udp",
        }
    }

    /// Human-readable description for the pico-diag.txt stub header,
    /// including the peer address when known.
    fn describe(&self) -> String {
        match self {
            DiagSource::SetupCdc => "setup-mode USB-CDC".to_string(),
            DiagSource::VendorControl => "USB vendor control transfer".to_string(),
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
    VendorNotFound,
    VendorOpenFailed {
        error: String,
    },
    VendorTransferFailed {
        step: &'static str,
        bytes_received: usize,
        error: String,
    },
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
            DiagOutcome::VendorNotFound => "vendor_not_found",
            DiagOutcome::VendorOpenFailed { .. } => "vendor_open_failed",
            DiagOutcome::VendorTransferFailed { .. } => "vendor_transfer_failed",
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

    /// Body of pico-diag.txt for this outcome.
    ///
    /// On the Captured path the body is the firmware diag log itself.
    /// On every failure path the body has a two-section layout: a
    /// `Suggested next step` block leading with the most likely cause
    /// and an ordered list of things to try, followed by a `Diagnostic
    /// details` block with the raw captured fields. The order is
    /// intentional -- an operator reading the file top-down hits an
    /// action before they hit jargon.
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
            DiagOutcome::Empty { source } => stub_failure(
                "Pico answered, but its diag ring was empty.",
                &[
                    "Re-run the failing command and immediately run bundle while \
                     the Pico is still in the same state. If the failure was at \
                     boot, the reboot between the failure and bundle wiped the \
                     in-RAM ring.",
                    "If this is reproducible, attach a bug report -- an \
                     answering-but-empty Pico is unusual.",
                ],
                &[("source", &source.describe())],
            ),
            DiagOutcome::NoSetupPort => stub_failure(
                "No setup-mode Pico found, and no last-known run-mode Pico to \
                 fall back to.",
                &[
                    "Unplug the Pico. Hold BOOTSEL while plugging it back in. \
                     Wait until Windows shows a RPI-RP2 or RP2350 drive in File \
                     Explorer.",
                    "Run `flash.ps1` (or `couchlink.exe flash`) to copy the \
                     matching UF2 onto the drive.",
                    "Once the Pico reboots into setup mode (it should appear as \
                     a new COM port within ~5 seconds), re-run this bundle.",
                    "If no COM port shows up at all, try a different micro-USB \
                     DATA cable (charge-only cables fail) or a different USB \
                     port on the PC.",
                ],
                &[("looking_for_vid_pid", "0x2E8A:0xCAF0")],
            ),
            DiagOutcome::SetupOpenFailed { error } => stub_failure(
                "Pico is enumerated, but the bridge could not open its COM port.",
                &[
                    "Another application is probably holding the port. Close any \
                     open serial terminals (PuTTY, Tera Term, Arduino Serial \
                     Monitor, screen, minicom) and re-run.",
                    "If no app is open, unplug + replug the Pico and re-run.",
                    "If the error mentions ACCESS_DENIED specifically, a Windows \
                     driver may have failed to bind; check Device Manager for a \
                     yellow exclamation mark on the Pico's entry.",
                ],
                &[("error", error)],
            ),
            DiagOutcome::SetupProbeFailed {
                port,
                step,
                elapsed_ms,
                bytes_received,
                rx_first_32_hex,
                error,
            } => {
                let (root, steps) = setup_probe_failed_diagnosis(step, *bytes_received);
                stub_failure(
                    root,
                    steps,
                    &[
                        ("port", port),
                        ("step", step),
                        ("elapsed_ms", &elapsed_ms.to_string()),
                        ("bytes_received", &bytes_received.to_string()),
                        ("rx_first_32_hex", rx_first_32_hex),
                        ("error", error),
                    ],
                )
            }
            DiagOutcome::NoLastPicoInConfig => stub_failure(
                "No setup-mode Pico found, and no run-mode Pico has ever been \
                 seen by this bridge installation.",
                &[
                    "Run `couchlink setup` to provision a Pico (flash + Wi-Fi).",
                    "Or, if a Pico is already running on your LAN, run \
                     `couchlink doctor` -- if discovery succeeds it will be \
                     recorded in config and the next bundle can probe it.",
                ],
                &[],
            ),
            DiagOutcome::VendorNotFound => stub_failure(
                "No Pico with a diag-vendor interface (WinUSB-bound) is \
                 currently enumerated. Either the Pico is in run mode (no \
                 diag interface present) or its firmware predates the \
                 WinUSB diag channel.",
                &[
                    "If the Pico is in run mode, retrieval falls through to \
                     UDP automatically; this stub means UDP also did not \
                     succeed -- see the UDP entries for diagnostics.",
                    "If the Pico is in setup mode but no diag interface is \
                     visible, the firmware predates the diag-vendor \
                     interface. Reflash with the matching couchlink-*.uf2.",
                ],
                &[("looking_for_vid_pid", "0x2E8A:0xCAF0 + vendor interface")],
            ),
            DiagOutcome::VendorOpenFailed { error } => stub_failure(
                "Found a Pico with a diag-vendor interface but could not \
                 claim it via WinUSB.",
                &[
                    "Another process may be holding the diag interface. Close \
                     any running couchlink instances and re-run bundle.",
                    "If Windows shows the diag interface as 'driver not \
                     loaded' in Device Manager, the MS OS 2.0 descriptor \
                     binding may have failed. Unplug and replug the Pico; \
                     Windows re-evaluates WinUSB binding on enumeration.",
                ],
                &[("error", error)],
            ),
            DiagOutcome::VendorTransferFailed {
                step,
                bytes_received,
                error,
            } => stub_failure(
                "Vendor control transfer to retrieve the diag log failed.",
                &[
                    "Re-run bundle. Control transfers occasionally fail under \
                     bus glitches; a retry usually succeeds.",
                    "If the error mentions PIPE or STALL, the firmware did \
                     not recognise the vendor request -- the bridge and \
                     firmware may be on mismatched protocol versions. \
                     Reflash with the matching couchlink-*.uf2.",
                ],
                &[
                    ("step", step),
                    ("bytes_received", &bytes_received.to_string()),
                    ("error", error),
                ],
            ),
            DiagOutcome::UdpDiscoveryFailed { reason } => stub_failure(
                "Bundle tried a run-mode UDP probe but the Pico did not answer.",
                &[
                    "Confirm the Pico is powered on, plugged into the USB4MAPLE \
                     (or equivalent), and within Wi-Fi range of the AP it was \
                     provisioned against.",
                    "If you have changed Wi-Fi networks since setup, the saved \
                     credentials are now stale. Hold BOOTSEL for 3+ seconds \
                     during plug-in to wipe the saved creds, then re-run \
                     `couchlink setup`.",
                    "If you have multiple network adapters, make sure the bridge \
                     is allowed through Windows Firewall on the active profile. \
                     `couchlink doctor` will surface a firewall mismatch.",
                ],
                &[("error", reason)],
            ),
            DiagOutcome::UdpProbeFailed {
                peer,
                step,
                elapsed_ms,
                chunks_received,
                error,
            } => stub_failure(
                "Pico answered initial discovery but the CMD_GET_LOG exchange \
                 did not complete.",
                &[
                    "The Pico is alive on the LAN -- it answered discovery -- \
                     but either the request did not reach it or its reply did \
                     not reach us. Run `couchlink bundle` again; transient \
                     packet loss is the most likely cause.",
                    "If it persists across multiple bundle attempts, check \
                     whether anything on the network is doing aggressive packet \
                     inspection (some corporate firewalls drop unknown UDP \
                     types).",
                ],
                &[
                    ("peer", &peer.to_string()),
                    ("step", step),
                    ("elapsed_ms", &elapsed_ms.to_string()),
                    ("chunks_received", &chunks_received.to_string()),
                    ("error", error),
                ],
            ),
            DiagOutcome::UdpUnsupported { peer } => stub_failure(
                "Pico is reachable on the LAN but is running pre-LogChunk \
                 firmware.",
                &[
                    "The Pico answered discovery, but its ACK does not advertise \
                     the LogChunk capability bit. The firmware predates the \
                     run-mode diag pull.",
                    "Hold BOOTSEL while plugging the Pico into this PC, then \
                     flash with `flash.ps1`. The new firmware advertises the \
                     bit and the next bundle will UDP-pull diag automatically.",
                    "If you cannot reflash right now, the bridge log at \
                     %LOCALAPPDATA%\\ParsecCouchLink\\data\\logs has \
                     bridge-side telemetry that does not depend on the firmware.",
                ],
                &[("peer", &peer.to_string())],
            ),
        }
    }
}

/// Generate the pico-diag.txt body for `DiagOutcome::VendorNotFound`,
/// gated on what the pnputil snapshot shows for 0x2E8A:0xCAF0. The three
/// branches match `PicoEnumState` (NotEnumerated, EnumeratedRunMode, and
/// the setup-mode-shaped but unclaimable case). The generic static text
/// in the `VendorNotFound` match arm is replaced at write time by this
/// function so the instructions match what Windows actually sees.
fn vendor_not_found_stub_text(state: &PicoEnumState) -> String {
    match state {
        PicoEnumState::NotEnumerated => stub_failure(
            "No Pico (VID_2E8A:PID_CAF0) is currently enumerated on USB.",
            &[
                "Try a different micro-USB DATA cable -- charge-only cables enumerate \
                 USB power but carry no data.",
                "Try a different USB port on the PC (prefer a port on the motherboard, \
                 not a hub).",
                "Hold BOOTSEL for 5+ seconds while replugging to wipe creds and force \
                 setup mode.",
            ],
            &[("looking_for_vid_pid", "0x2E8A:0xCAF0")],
        ),
        PicoEnumState::EnumeratedRunMode => stub_failure(
            "The Pico is in run mode. Run-mode firmware does not expose a USB diag \
             interface -- the vendor interface exists only in setup mode.",
            &[
                "Wait ~30 s for the Wi-Fi association watchdog to auto-bounce the Pico \
                 back to setup mode if Wi-Fi association is failing, then run \
                 `couchlink bundle` again.",
                "Hold BOOTSEL briefly (under 2 s) while replugging to force setup mode \
                 without wiping creds.",
                "Hold BOOTSEL for 3+ s to force setup mode AND wipe creds.",
            ],
            &[(
                "looking_for_vid_pid",
                "0x2E8A:0xCAF0 (run mode, no vendor interface)",
            )],
        ),
        PicoEnumState::EnumeratedSetupMode
        | PicoEnumState::EnumeratedButInterfaceUnclaimable { .. } => stub_failure(
            "Found a Pico with a diag-vendor interface but could not claim it via WinUSB.",
            &[
                "Another process may be holding the diag interface. Close any running \
                 couchlink instances and re-run bundle.",
                "If Windows shows the diag interface as 'driver not loaded' in Device \
                 Manager, the MS OS 2.0 descriptor binding may have failed. Unplug and \
                 replug the Pico; Windows re-evaluates WinUSB binding on enumeration.",
            ],
            &[("looking_for_vid_pid", "0x2E8A:0xCAF0 + vendor interface")],
        ),
    }
}

/// Format a self-diagnosing stub body. Leads with a one-sentence root
/// cause, then a numbered "Try this" list, then a `Diagnostic details`
/// block with the captured fields verbatim.
fn stub_failure(root_cause: &str, steps: &[&str], fields: &[(&str, &str)]) -> String {
    let mut out = String::new();
    out.push_str("=== Suggested next step ===\n");
    out.push_str(root_cause);
    out.push('\n');
    out.push('\n');
    out.push_str("Try this (in order):\n");
    for (i, s) in steps.iter().enumerate() {
        out.push_str(&format!("  {}. {}\n", i + 1, soft_wrap(s, 78, "     ")));
    }
    if !fields.is_empty() {
        out.push('\n');
        out.push_str("=== Diagnostic details ===\n");
        for (k, v) in fields {
            out.push_str(&format!("  {k}: {v}\n"));
        }
    }
    out
}

/// Wraps `text` so each line stays under `width` columns; continuation
/// lines are indented by `indent`. Whitespace-only sequences in the
/// input become single spaces.
fn soft_wrap(text: &str, width: usize, indent: &str) -> String {
    let mut out = String::new();
    let mut line_len = 0;
    for word in text.split_whitespace() {
        if line_len == 0 {
            out.push_str(word);
            line_len = word.len();
            continue;
        }
        if line_len + 1 + word.len() > width {
            out.push('\n');
            out.push_str(indent);
            out.push_str(word);
            line_len = indent.len() + word.len();
        } else {
            out.push(' ');
            out.push_str(word);
            line_len += 1 + word.len();
        }
    }
    out
}

/// Map a HELLO-probe failure shape to root cause + remediation list.
/// Most of the diagnostic value of the new bundle is concentrated
/// here: when the operator opens pico-diag.txt this is what they read
/// first.
fn setup_probe_failed_diagnosis(
    step: &str,
    bytes_received: usize,
) -> (&'static str, &'static [&'static str]) {
    static WRITE: &[&str] = &[
        "Unplug the Pico and plug it back in (no BOOTSEL).",
        "If the COM port re-appears but the bridge still fails, the USB \
         serial driver (usbser.sys) may be in a bad state. Reboot Windows.",
        "If it still fails, try a different USB port -- preferably one on \
         the motherboard rather than through a hub.",
    ];
    static READ_NO_BYTES: &[&str] = &[
        "The firmware enumerated USB (Windows sees the COM port) but is not \
         responding to commands. The most common cause is a fault during \
         firmware init -- the CDC stack is up enough to enumerate, but the \
         main poll loop never started.",
        "Hold BOOTSEL while plugging the Pico in. Run `flash.ps1` to write \
         a fresh UF2. Re-run setup. The new firmware writes a `boot: \
         reset-reason=fault` line on the next boot if it crashed; the \
         bundle then captures WHY it crashed via the new fault context \
         (PC, LR, xPSR, R0-R3, R12, SP, CFSR on RP2350).",
        "If a fresh flash still fails: try a different micro-USB DATA cable \
         (charge-only cables enumerate USB but fail data transfers), or a \
         different USB port.",
        "If still failing after cable + port + reflash, this is worth a bug \
         report. Attach this bundle.",
    ];
    static READ_SOME_BYTES: &[&str] = &[
        "The firmware is alive and writing bytes on the wire, but those \
         bytes are not a valid HELLO_ACK frame. The hex preview above \
         shows what it actually said. The most common cause is a version \
         mismatch between the bridge and the firmware UF2.",
        "Reflash the Pico with the couchlink-*.uf2 from the SAME release \
         as the couchlink.exe you are running. Mixing v2026.5.16.x \
         firmware with v2026.5.16.y bridge is the canonical cause of this \
         exact shape.",
        "If the hex preview looks like ASCII text (e.g. starts with `54 \
         75` = \"Tu...\"), the firmware may be writing diag log lines \
         directly to CDC instead of framed responses. That is a firmware \
         bug; attach this bundle.",
    ];
    static DECODE: &[&str] = &[
        "Bytes arrived but the frame header is malformed -- wrong magic, \
         wrong CRC, or wrong opcode. Almost always a protocol-version \
         mismatch.",
        "Make sure the bridge .exe and the firmware .uf2 came from the \
         same release zip. Re-download the release if unsure.",
    ];
    static GET_LOG: &[&str] = &[
        "HELLO succeeded but the follow-up GET_LOG_BUFFER call failed. The \
         firmware is responding to commands but not to this one \
         specifically.",
        "Most likely the Pico rebooted between HELLO and GET_LOG_BUFFER. \
         Re-run bundle; it should retry against the post-reboot state.",
        "If it persists, this is worth a bug report.",
    ];
    if step == "write" {
        (
            "Bridge could not write the HELLO frame to the firmware.",
            WRITE,
        )
    } else if step == "read" && bytes_received == 0 {
        (
            "Firmware enumerated USB but did not write a single byte back \
             during the 10-second probe.",
            READ_NO_BYTES,
        )
    } else if step == "read" {
        (
            "Firmware is alive on the wire but its bytes did not parse as a \
             HELLO_ACK frame.",
            READ_SOME_BYTES,
        )
    } else if step == "decode" {
        (
            "Bytes arrived but did not decode as a valid framed response.",
            DECODE,
        )
    } else if step == "get_log_buffer" {
        (
            "HELLO succeeded but the diag-log fetch did not complete.",
            GET_LOG,
        )
    } else {
        (
            "HELLO probe failed at an unexpected step.",
            &["This shape was not anticipated by the self-diagnosis. \
                 Attach this bundle to a bug report; the captured fields \
                 below carry enough detail to diagnose offline."],
        )
    }
}

/// Try setup-mode USB-CDC first, then WinUSB vendor control transfer
/// (works even when CDC bulk endpoints are wedged), then run-mode UDP.
/// First successful capture wins; on total failure, return the CDC
/// outcome because it carries the richest diagnostic detail.
async fn capture_pico_diag() -> DiagOutcome {
    let cdc_result = try_capture_setup_cdc().await;
    if matches!(
        cdc_result,
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. }
    ) {
        return cdc_result;
    }

    tracing::info!(
        "bundle: CDC diag path returned {}, trying vendor control transfer",
        cdc_result.discriminant_str()
    );
    let vendor_result = try_capture_vendor_control().await;
    if matches!(
        vendor_result,
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. }
    ) {
        tracing::info!("bundle: diag captured via USB vendor control transfer");
        return vendor_result;
    }

    tracing::info!(
        "bundle: vendor control path returned {}, trying run-mode UDP",
        vendor_result.discriminant_str()
    );
    let udp_result = try_capture_run_udp().await;
    if matches!(
        udp_result,
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. }
    ) {
        tracing::info!("bundle: diag captured via UDP TYPE_GET_LOG");
        return udp_result;
    }

    // All three paths failed. Prefer the CDC outcome for diagnostic
    // specificity (it has step / elapsed / rx_bytes detail); the vendor
    // and UDP outcomes get logged but not surfaced in the manifest.
    tracing::warn!(
        "bundle: all three diag paths failed (cdc={}, vendor={}, udp={})",
        cdc_result.discriminant_str(),
        vendor_result.discriminant_str(),
        udp_result.discriminant_str()
    );
    cdc_result
}

/// WinUSB vendor-control diag retrieval. Wraps the blocking nusb
/// implementation in `spawn_blocking` (matches `try_capture_setup_cdc`'s
/// shape), translates `VendorDiagOutcome` to `DiagOutcome`.
async fn try_capture_vendor_control() -> DiagOutcome {
    use crate::diag_usb::{capture_diag_blocking, VendorDiagOutcome};
    let outcome = match tokio::task::spawn_blocking(capture_diag_blocking).await {
        Ok(o) => o,
        Err(join_err) => {
            return DiagOutcome::VendorTransferFailed {
                step: "spawn",
                bytes_received: 0,
                error: format!("blocking task panicked: {join_err}"),
            };
        }
    };

    match outcome {
        VendorDiagOutcome::Captured { text, lost } => DiagOutcome::Captured {
            source: DiagSource::VendorControl,
            text,
            lost,
        },
        VendorDiagOutcome::Empty => DiagOutcome::Empty {
            source: DiagSource::VendorControl,
        },
        VendorDiagOutcome::NotFound => DiagOutcome::VendorNotFound,
        VendorDiagOutcome::OpenFailed { error } => DiagOutcome::VendorOpenFailed { error },
        VendorDiagOutcome::TransferFailed {
            step,
            bytes_received,
            error,
        } => DiagOutcome::VendorTransferFailed {
            step,
            bytes_received,
            error,
        },
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

        // 10-second deadline (vs. the wizard's 3 s): the bundle is the
        // "something failed; gather everything" path, so we'd rather
        // wait for late-arriving bytes from a slow-booting firmware
        // than declare timeout fast. If the wizard already saw a 3 s
        // timeout, an extra 7 s here often surfaces a delayed RSP that
        // would otherwise be invisible.
        let probe = pico.hello_probe_with_timeout(Duration::from_secs(10));
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

/// Run-mode UDP path. Tries broadcast first so a stale last_ip does not
/// prevent diag capture; falls back to unicast against last_ip only when
/// broadcast finds nothing. Two-second timeout on each leg keeps the
/// bundle fast in the common failure case.
#[allow(dead_code)] // available for the bundle flow; cmd_lab uses its own inline UDP path
pub(crate) async fn pull_pico_log_via_udp() -> Result<String, String> {
    match try_capture_run_udp().await {
        DiagOutcome::Captured { text, .. } => Ok(text),
        DiagOutcome::Empty { .. } => Ok(String::new()),
        other => Err(format!("{other:?}")),
    }
}

async fn try_capture_run_udp() -> DiagOutcome {
    let cfg = config::load().unwrap_or_default();
    let last_ip = cfg.last_pico.as_ref().and_then(|p| p.last_ip.clone());
    if last_ip.is_none() && cfg.last_pico.is_none() {
        tracing::info!("bundle: no last_pico in config; UDP probe not attempted");
        return DiagOutcome::NoLastPicoInConfig;
    }

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("bind: {e}"),
            };
        }
    };
    if let Err(e) = socket.set_broadcast(true) {
        tracing::warn!("bundle: set_broadcast failed: {e} -- broadcast leg skipped");
    }

    // Step 1: short broadcast discovery (2 s).
    let peer_addr = match broadcast_for_ack(&socket, Duration::from_secs(2)).await {
        Ok(addr) => {
            tracing::info!("bundle: broadcast discovery found Pico at {addr}");
            addr
        }
        Err(broadcast_err) => {
            // Broadcast found nothing. Try unicast against last_ip if we have one.
            let Some(last_ip) = last_ip else {
                tracing::info!("bundle: broadcast found nothing and no last_ip; UDP probe done");
                return DiagOutcome::NoLastPicoInConfig;
            };
            let peer: SocketAddr = match format!("{last_ip}:{}", protocol::PORT).parse() {
                Ok(a) => a,
                Err(e) => {
                    return DiagOutcome::UdpDiscoveryFailed {
                        reason: format!("config last_ip `{last_ip}` did not parse: {e}"),
                    };
                }
            };
            tracing::info!(
                "bundle: broadcast found nothing ({broadcast_err}); \
                 trying unicast to last known IP {peer}"
            );
            match unicast_for_ack(&socket, peer, Duration::from_secs(2)).await {
                Ok(pkt) => {
                    tracing::info!(
                        "bundle: broadcast found nothing; reaching last known IP {peer} \
                         flags=0x{:02X}",
                        pkt.flags,
                    );
                    peer
                }
                Err(e) => {
                    return DiagOutcome::UdpDiscoveryFailed {
                        reason: format!("broadcast: {broadcast_err}; unicast to {peer}: {e}"),
                    };
                }
            }
        }
    };

    // Step 2: read the capability flag from the peer we found.
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

/// Broadcast a Discover and return the address of the first Pico that answers.
/// Returns `Err(reason)` if no ack arrives within `timeout`.
async fn broadcast_for_ack(socket: &UdpSocket, timeout: Duration) -> Result<SocketAddr, String> {
    let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", protocol::PORT)
        .parse()
        .expect("broadcast addr is constant");
    let req = Packet::discover(0).encode();
    socket
        .send_to(&req, broadcast_addr)
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
            Ok(Ok((n, from))) => match Packet::decode(&buf[..n]) {
                Ok(pkt) if matches!(pkt.kind, PacketKind::Ack(_)) => return Ok(from),
                Ok(_) => continue,
                Err(e) => {
                    tracing::debug!("bundle: discarded non-ack from {from}: {e}");
                }
            },
            Ok(Err(e)) => return Err(format!("recv: {e}")),
            Err(_) => return Err(format!("no ack within {} ms", timeout.as_millis())),
        }
    }
}

/// Classification of the Pico's current USB enumeration state, derived
/// from pnputil output. Used to gate `VendorNotFound` stub text on what
/// Windows actually sees on the bus.
#[derive(Debug, PartialEq, Eq)]
enum PicoEnumState {
    /// No entry with VID_2E8A&PID_CAF0 in pnputil output.
    NotEnumerated,
    /// VID_2E8A&PID_CAF0 present with both MI_00 (CDC) and MI_02 (vendor).
    /// The vendor interface should be WinUSB-bound in setup mode.
    EnumeratedSetupMode,
    /// VID_2E8A&PID_CAF0 present but no MI_02 / vendor interface found.
    /// Run-mode firmware presents only the XInput composite without a
    /// vendor interface; xusb22.sys being bound is a secondary indicator.
    EnumeratedRunMode,
    /// Setup-mode-shaped device found but the diag_usb open call failed.
    #[allow(dead_code)]
    EnumeratedButInterfaceUnclaimable { reason: String },
}

/// Parse pnputil /enum-devices text to determine how the Pico is
/// currently enumerated. The function does not require all blocks to be
/// present; it looks for specific Instance ID patterns.
///
/// Setup mode: the composite parent VID_2E8A&PID_CAF0\... plus a child
/// with &MI_02 (the WinUSB vendor interface). Run mode: the composite
/// parent is present but no MI_02 child exists (the firmware presents
/// XInput only). The absence of both parent and children means
/// NotEnumerated.
fn classify_pico_enum(pnputil_text: &str) -> PicoEnumState {
    // Check for the parent device.
    let has_parent = pnputil_text
        .lines()
        .any(|l| l.contains("VID_2E8A") && l.contains("PID_CAF0") && !l.contains("&MI_"));

    if !has_parent {
        return PicoEnumState::NotEnumerated;
    }

    // Check for the MI_02 vendor interface (setup mode only).
    let has_vendor_itf = pnputil_text
        .lines()
        .any(|l| l.contains("VID_2E8A") && l.contains("PID_CAF0") && l.contains("&MI_02"));

    if has_vendor_itf {
        PicoEnumState::EnumeratedSetupMode
    } else {
        // Parent present but no vendor interface -- run mode firmware shape.
        PicoEnumState::EnumeratedRunMode
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

/// Last 15 minutes of OS-level USB events from the Windows event log.
/// Catches driver bind failures, surprise removals, and descriptor
/// request timeouts -- none of which surface in pnputil's snapshot.
/// Best-effort: a long-running event log query, a missing PowerShell,
/// or a permissions denial all return `None` and the bundle records
/// that the capture failed in the manifest.
#[cfg(windows)]
async fn capture_windows_usb_events() -> Option<String> {
    // Get-WinEvent's `-FilterHashtable` is documented to fail with an
    // unhelpful "No events were found" message when the filter matches
    // nothing -- which is normal on a quiet system. Catch that branch
    // and return an empty string rather than `None` so the bundle
    // header makes the "uneventful" case obvious to the operator.
    //
    // The query is split: System log gets the usbhub / usbser drivers,
    // and the Kernel-PnP/Configuration log catches the higher-level
    // bind events. PS 5.1 syntax: no `&&` chaining, no ternary.
    let ps_cmd = r#"
$ErrorActionPreference = 'SilentlyContinue'
$start = (Get-Date).AddMinutes(-15)
$events = @()
$sys = Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=$start} -MaxEvents 200 2>$null
if ($sys) {
    $events += $sys | Where-Object { $_.ProviderName -match '(?i)usb|usbser|usbhub|pnp' }
}
$pnp = Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Kernel-PnP/Configuration'; StartTime=$start} -MaxEvents 100 2>$null
if ($pnp) { $events += $pnp }
if (-not $events -or $events.Count -eq 0) {
    Write-Output '(no matching events in the last 15 minutes)'
    exit 0
}
$events |
    Sort-Object TimeCreated |
    ForEach-Object {
        $msg = if ($_.Message) { $_.Message.Trim() } else { '' }
        Write-Output ('[' + $_.TimeCreated.ToString('yyyy-MM-ddTHH:mm:ss.fff') + '] ' + $_.LevelDisplayName + ' ' + $_.ProviderName + '/' + $_.Id)
        Write-Output ('  ' + ($msg -replace "`r?`n", "`n  "))
        Write-Output ''
    }
"#;
    // 30-second cap: Get-WinEvent against System on a busy machine can
    // be slow, and we'd rather skip than hang the bundle indefinitely.
    let fut = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_cmd])
        .output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            tracing::debug!("bundle: powershell spawn for usb events failed: {e}");
            return None;
        }
        Err(_) => {
            tracing::debug!("bundle: usb events query timed out after 30 s");
            return None;
        }
    };
    if !out.status.success() {
        tracing::debug!(
            "bundle: powershell exit {} for usb events",
            out.status.code().unwrap_or(-1)
        );
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
async fn capture_windows_usb_events() -> Option<String> {
    None
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
        // Lead section is the operator-facing "Suggested next step";
        // the captured fields appear in the trailing Diagnostic block.
        assert!(
            stub.contains("=== Suggested next step ==="),
            "no header: {stub}"
        );
        assert!(stub.contains("Try this (in order):"), "no try list: {stub}");
        assert!(
            stub.contains("=== Diagnostic details ==="),
            "no detail block: {stub}"
        );
        assert!(stub.contains("port: COM3"), "missing port field: {stub}");
        assert!(stub.contains("step: read"), "missing step field: {stub}");
        assert!(
            stub.contains("elapsed_ms: 3012"),
            "missing elapsed field: {stub}"
        );
        assert!(
            stub.contains("bytes_received: 0"),
            "missing bytes_received field: {stub}"
        );
        // The read+0 case is the most common reproduction shape and should
        // lead with a fault-during-init story.
        assert!(
            stub.contains("did not write a single byte"),
            "missing read+0 lead: {stub}"
        );
    }

    #[test]
    fn udp_unsupported_names_peer() {
        let out = DiagOutcome::UdpUnsupported { peer: make_peer() };
        let stub = out.stub_text();
        assert!(stub.contains("10.0.0.24:4242"));
        // soft_wrap can break "LogChunk capability bit" across a line,
        // so check for the unbreakable token only.
        assert!(
            stub.contains("LogChunk"),
            "missing capability mention: {stub}"
        );
        assert!(stub.contains("peer: 10.0.0.24:4242"));
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
        assert!(stub.contains("step: recv_chunk"));
        assert!(stub.contains("chunks_received: 3"));
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

    // Canonical pnputil snippet for a Pico in setup mode (VID_2E8A:PID_CAF0
    // composite parent + MI_00 CDC child + MI_02 vendor child). Derived from
    // a real bundle capture where the Pico was in setup mode and WinUSB was
    // bound to the vendor interface.
    const PNPUTIL_SETUP_MODE: &str = "\
Instance ID:                USB\\VID_2E8A&PID_CAF0\\E0C9125B0D9B
Device Description:         USB Composite Device
Class Name:                 USB
Status:                     Started
Driver Name:                usb.inf

Instance ID:                USB\\VID_2E8A&PID_CAF0&MI_00\\8&22cf742d&0&0000
Device Description:         USB Serial Device
Class Name:                 Ports
Status:                     Started
Driver Name:                usbser.inf

Instance ID:                USB\\VID_2E8A&PID_CAF0&MI_02\\8&22cf742d&0&0002
Device Description:         Pico Diag
Class Name:                 USBDevice
Status:                     Started
Driver Name:                winusb.inf
";

    // Canonical pnputil snippet for a Pico in run mode (VID_2E8A:PID_CAF0
    // composite parent + XInput child only, no MI_02). The run-mode firmware
    // presents only the XInput HID interface.
    const PNPUTIL_RUN_MODE: &str = "\
Instance ID:                USB\\VID_2E8A&PID_CAF0\\E0C9125B0D9B
Device Description:         USB Composite Device
Class Name:                 USB
Status:                     Started
Driver Name:                usb.inf

Instance ID:                USB\\VID_2E8A&PID_CAF0&MI_00\\8&33aa123&0&0000
Device Description:         Xbox 360 Controller
Class Name:                 XboxController
Status:                     Started
Driver Name:                xusb22.inf
";

    #[test]
    fn classify_pico_enum_not_enumerated() {
        // Bundle from the first customer (Pico 2 W in run mode with Wi-Fi
        // failed): no 2E8A:CAF0 entries at all.
        let text = "Instance ID: USB\\VID_28DE&PID_2102\\07F8359478\nStatus: Started\n";
        assert_eq!(classify_pico_enum(text), PicoEnumState::NotEnumerated);
    }

    #[test]
    fn classify_pico_enum_setup_mode() {
        assert_eq!(
            classify_pico_enum(PNPUTIL_SETUP_MODE),
            PicoEnumState::EnumeratedSetupMode,
        );
    }

    #[test]
    fn classify_pico_enum_run_mode() {
        assert_eq!(
            classify_pico_enum(PNPUTIL_RUN_MODE),
            PicoEnumState::EnumeratedRunMode,
        );
    }

    #[test]
    fn classify_pico_enum_parent_only_is_run_mode() {
        // Parent with no children at all (e.g. driver not yet bound) should
        // classify as run mode (no vendor interface visible) rather than
        // NotEnumerated.
        let text = "Instance ID: USB\\VID_2E8A&PID_CAF0\\E0C9125B0D9B\nStatus: Started\n";
        assert_eq!(classify_pico_enum(text), PicoEnumState::EnumeratedRunMode);
    }

    #[test]
    fn vendor_not_found_stub_not_enumerated_names_cable() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::NotEnumerated);
        assert!(stub.contains("0x2E8A:0xCAF0"), "missing VID/PID: {stub}");
        assert!(stub.contains("DATA cable"), "missing cable tip: {stub}");
    }

    #[test]
    fn vendor_not_found_stub_run_mode_names_watchdog() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::EnumeratedRunMode);
        assert!(
            stub.contains("association watchdog"),
            "missing watchdog tip: {stub}"
        );
        assert!(stub.contains("BOOTSEL"), "missing BOOTSEL tip: {stub}");
    }

    #[test]
    fn vendor_not_found_stub_setup_mode_names_winusb() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::EnumeratedSetupMode);
        assert!(stub.contains("WinUSB"), "missing WinUSB tip: {stub}");
    }
}
