//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! crash files, Pico diag log, and a manifest.json with non-sensitive system
//! info. Intended to be attached to a bug report.
//!
//! NEVER include Wi-Fi credentials. The Pico stores them and the bridge
//! never reads them. SSID is also omitted by default to be safe.

mod collect;
mod manifest;
mod pico_diag;
mod sysinfo;
mod usb_enum;

use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::{cmd_run, cmd_usb_diag, config, journal};

use collect::{bundle_log_prefix, collect_crash_file_names, collect_setup_transcript_names};
use manifest::build_manifest;
use pico_diag::{capture_pico_diag, DiagOutcome};
use sysinfo::{build_system_info, run_doctor_silently};
use usb_enum::{
    capture_usb_devices, capture_windows_usb_events, classify_pico_enum, parent_only_stub_text,
    vendor_not_found_stub_text, PicoEnumState,
};

/// Structured result of a bundle build. Returned by `build_bundle` so
/// callers get a typed answer without
/// scraping the CLI's `println!` summary.
#[allow(dead_code)] // returned for tests and future local automation
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
    pub usb_diag_captured: bool,
    pub usb_diag_target_count: usize,
}

#[derive(Clone, Debug)]
struct UsbDiagBundle {
    text: String,
    captured: bool,
    target_count: usize,
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

    let usb_diag = capture_usb_diag_text().await;

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
        PicoEnumState::EnumeratedParentOnly => Some("parent_only".to_string()),
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
        usb_diag.captured,
        usb_diag.target_count,
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
    // VendorNotFound and parent-only VendorOpenFailed are special: their
    // stub text depends on the USB topology captured in pico_enum_state.
    let pico_diag_body = match (&diag, &pico_enum_state) {
        (DiagOutcome::VendorNotFound, _) => vendor_not_found_stub_text(&pico_enum_state),
        (DiagOutcome::VendorOpenFailed { .. }, PicoEnumState::EnumeratedParentOnly) => {
            parent_only_stub_text()
        }
        _ => diag.stub_text(),
    };
    zip.start_file("pico-diag.txt", opts)?;
    zip.write_all(pico_diag_body.as_bytes())?;

    // usb-diag.txt: structured run-mode USB counters from the Pico. This
    // complements pico-diag.txt's firmware log ring with the current USB
    // mount, descriptor, input-report, and host OUT counters.
    zip.start_file("usb-diag.txt", opts)?;
    zip.write_all(usb_diag.text.as_bytes())?;

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
    // (PowerShell transcripts from setup.ps1).
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
        usb_diag_captured: usb_diag.captured,
        usb_diag_target_count: usb_diag.target_count,
    })
}

async fn capture_usb_diag_text() -> UsbDiagBundle {
    let (targets, source) = match resolve_usb_diag_targets().await {
        Ok(found) => found,
        Err(e) => {
            return UsbDiagBundle {
                text: format!(
                    "Structured Pico USB diagnostics were not captured.\n\n\
                     Suggested next step:\n\
                     - Make sure the Pico is powered, has joined Wi-Fi, and is still plugged into the console adapter.\n\
                     - Run `couchlink.exe bundle` again immediately after the failure.\n\
                     - If the Pico is on Wi-Fi but broadcast discovery is blocked, run `couchlink.exe doctor` once so the last-known IP is saved.\n\n\
                     Diagnostic details:\n\
                     error={e:#}\n"
                ),
                captured: false,
                target_count: 0,
            };
        }
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Structured Pico USB diagnostics\n# target source: {source}\n"
    );

    let mut captured = false;
    for pico in &targets {
        let _ = writeln!(out, "{}", pico.detail_label());
        match cmd_usb_diag::query_usb_diag(pico, Duration::from_secs(3)).await {
            Ok(diag) => {
                captured = true;
                out.push_str(&cmd_usb_diag::format_usb_diag(&diag, pico.persona));
            }
            Err(e) => {
                let _ = writeln!(
                    out,
                    "  FAIL  USB diagnostic did not reply: {e:#}\n  Update Pico firmware, then run this bundle again."
                );
            }
        }
        out.push('\n');
    }

    UsbDiagBundle {
        text: out,
        captured,
        target_count: targets.len(),
    }
}

async fn resolve_usb_diag_targets() -> Result<(Vec<cmd_run::PicoTarget>, String)> {
    match cmd_run::discover_picos(Duration::from_secs(2)).await {
        Ok(picos) if !picos.is_empty() => {
            return Ok((picos, "broadcast discovery".to_string()));
        }
        Ok(_) => {}
        Err(e) => {
            tracing::debug!("bundle: USB diag broadcast discovery failed: {e:#}");
        }
    }

    let cfg = config::load().unwrap_or_default();
    let last_ip = cfg
        .last_pico
        .as_ref()
        .and_then(|p| p.last_ip.as_deref())
        .ok_or_else(|| {
            anyhow!("no running Pico answered discovery and no last-known Pico IP is saved")
        })?;
    let ip = cmd_run::parse_ip_selector(last_ip)
        .ok_or_else(|| anyhow!("last-known Pico IP `{last_ip}` is not a valid IP address"))?;
    let pico = cmd_run::probe_pico_ip(ip, Duration::from_secs(3))
        .await
        .with_context(|| format!("probing last-known Pico IP {ip}"))?;
    Ok((vec![pico], format!("last-known IP {ip}")))
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
    if summary.usb_diag_captured {
        println!(
            "  usb-diag.txt: captured for {} Pico board(s)",
            summary.usb_diag_target_count
        );
    } else {
        println!("  usb-diag.txt: not captured -- see the file for details");
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
