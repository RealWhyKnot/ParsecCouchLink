//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! crash files, Pico diag log, and a manifest.json with non-sensitive system
//! info. Intended to be attached to a bug report.
//!
//! NEVER include Wi-Fi credentials. The Pico stores them and the bridge
//! never reads them. SSID is also omitted by default to be safe.

mod collect;
mod host_snapshot;
mod manifest;
mod pico_diag;
mod redact;
mod sysinfo;
mod usb_enum;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::{cmd_run, cmd_usb_diag, config, debug_packets, journal, pico_cache, pico_state};

use collect::{bundle_log_prefix, collect_crash_file_names, collect_setup_transcript_names};
use host_snapshot::capture_host_snapshots;
use manifest::{build_manifest, ManifestHostSnapshot, ManifestPicoCapture};
use pico_diag::{capture_pico_diag, DiagOutcome};
use redact::redact_bundle_text;
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
    pub usb_packet_dump_count: usize,
    pub retained_debug_packet_log_count: usize,
    pub retained_debug_packet_count: usize,
    pub per_pico_capture_count: usize,
    pub host_snapshot_count: usize,
    pub diagnostic_cache_included: bool,
}

#[derive(Clone, Debug)]
struct UsbDiagBundle {
    text: String,
    captured: bool,
    target_count: usize,
}

#[derive(Clone, Debug)]
struct PicoBundleCapture {
    manifest: ManifestPicoCapture,
    state_json: String,
    pico_diag_text: String,
    usb_diag_text: String,
    usb_packets_text: String,
}

#[derive(Clone, Debug)]
struct PicoCaptureSeed {
    uid: String,
    target: Option<cmd_run::PicoTarget>,
    saved: Option<config::PicoIdentity>,
    source: String,
    cached_state_json: Option<String>,
}

#[derive(Clone, Debug)]
struct RetainedDebugPacketLog {
    name: String,
    text: String,
}

#[derive(Default)]
struct CaptureLog {
    lines: Vec<String>,
}

impl CaptureLog {
    fn record_duration(
        &mut self,
        step: impl AsRef<str>,
        duration_ms: u64,
        status: impl AsRef<str>,
        bytes: usize,
        reason: impl AsRef<str>,
    ) {
        let step = sanitize_log_field(step.as_ref());
        let status = sanitize_log_field(status.as_ref());
        let reason = sanitize_log_field(reason.as_ref());
        tracing::debug!(
            "bundle-capture: step={} duration_ms={} status={} bytes={} reason={}",
            step,
            duration_ms,
            status,
            bytes,
            reason
        );
        self.lines.push(format!(
            "{}\t{step}\t{duration_ms}\t{status}\t{bytes}\t{reason}",
            Local::now().to_rfc3339()
        ));
    }

    fn record(
        &mut self,
        step: impl AsRef<str>,
        started: Instant,
        status: impl AsRef<str>,
        bytes: usize,
        reason: impl AsRef<str>,
    ) {
        let duration_ms = pico_cache::duration_ms(started.elapsed());
        self.record_duration(step, duration_ms, status, bytes, reason);
    }

    fn text(&self) -> String {
        let mut out = String::from("captured_at\tstep\tduration_ms\tstatus\tbytes\treason\n");
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

fn sanitize_log_field(value: &str) -> String {
    value.replace(['\r', '\n', '\t'], " ").trim().to_string()
}

/// Build the support bundle zip. Captures diag, doctor, usb topology,
/// logs, and writes them to `out_path`. Returns a structured summary.
///
/// CLI-side prompts (open-issues-in-browser, summary printing) live in
/// `run`, not here -- this function is silent on stdout/stderr.
pub async fn build_bundle(out_path: PathBuf) -> Result<BundleSummary> {
    journal!("bundle", "run started");
    tracing::info!("bundle: run started out_path={}", out_path.display());
    let mut capture_log = CaptureLog::default();

    let started = Instant::now();
    let diag = capture_pico_diag().await;
    capture_log.record(
        "top_level_pico_diag",
        started,
        diag.discriminant_str(),
        diag.stub_text().len(),
        diag.source_str().unwrap_or(""),
    );
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

    let started = Instant::now();
    let usb_diag = capture_usb_diag_text().await;
    capture_log.record(
        "top_level_usb_diag",
        started,
        if usb_diag.captured {
            "captured"
        } else {
            "not_captured"
        },
        usb_diag.text.len(),
        format!("targets={}", usb_diag.target_count),
    );

    let per_pico_captures = capture_per_pico(&mut capture_log).await;
    let usb_packet_dump_count: usize = per_pico_captures
        .iter()
        .map(|capture| capture.manifest.usb_packet_dump_count)
        .sum();
    let per_pico_manifest: Vec<ManifestPicoCapture> = per_pico_captures
        .iter()
        .map(|capture| capture.manifest.clone())
        .collect();
    let retained_debug_packet_logs = collect_retained_debug_packet_logs(&mut capture_log);
    let retained_debug_packet_log_names: Vec<String> = retained_debug_packet_logs
        .iter()
        .map(|log| log.name.clone())
        .collect();
    let retained_debug_packet_count =
        count_retained_debug_packet_lines(&retained_debug_packet_logs);

    let host_snapshots = capture_host_snapshots().await;
    for snapshot in &host_snapshots {
        capture_log.record_duration(
            format!("host_snapshot.{}", snapshot.manifest.name),
            snapshot.duration_ms,
            &snapshot.manifest.status,
            snapshot.text.len(),
            if snapshot.manifest.captured {
                "captured"
            } else {
                "not_captured"
            },
        );
    }
    let host_snapshot_manifest: Vec<ManifestHostSnapshot> = host_snapshots
        .iter()
        .map(|snapshot| snapshot.manifest.clone())
        .collect();

    let started = Instant::now();
    let cache_current = pico_cache::read_current();
    let cache_history = pico_cache::read_history();
    let diagnostic_cache_included = cache_current.is_some() || cache_history.is_some();
    capture_log.record(
        "diagnostic_cache",
        started,
        if diagnostic_cache_included {
            "included"
        } else {
            "not_present"
        },
        cache_current.as_ref().map(|s| s.len()).unwrap_or(0)
            + cache_history.as_ref().map(|s| s.len()).unwrap_or(0),
        "",
    );

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
        &retained_debug_packet_log_names,
        retained_debug_packet_count,
        diagnostic_cache_included,
        &per_pico_manifest,
        &host_snapshot_manifest,
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

    zip.start_file("bundle-capture.txt", opts)?;
    zip.write_all(redact_bundle_text(&capture_log.text()).as_bytes())?;

    zip.start_file("doctor.txt", opts)?;
    zip.write_all(redact_bundle_text(&doctor_text).as_bytes())?;

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
    zip.write_all(redact_bundle_text(&pico_diag_body).as_bytes())?;

    // usb-diag.txt: structured run-mode USB counters from the Pico. This
    // complements pico-diag.txt's firmware log ring with the current USB
    // mount, descriptor, input-report, and host OUT counters.
    zip.start_file("usb-diag.txt", opts)?;
    zip.write_all(redact_bundle_text(&usb_diag.text).as_bytes())?;

    for pico in &per_pico_captures {
        let base = pico.manifest.path.trim_end_matches('/');
        zip.start_file(format!("{base}/state.json"), opts)?;
        zip.write_all(redact_bundle_text(&pico.state_json).as_bytes())?;

        zip.start_file(format!("{base}/pico-diag.txt"), opts)?;
        zip.write_all(redact_bundle_text(&pico.pico_diag_text).as_bytes())?;

        zip.start_file(format!("{base}/usb-diag.txt"), opts)?;
        zip.write_all(redact_bundle_text(&pico.usb_diag_text).as_bytes())?;

        zip.start_file(format!("{base}/usb-packets.txt"), opts)?;
        zip.write_all(redact_bundle_text(&pico.usb_packets_text).as_bytes())?;
    }

    zip.start_file("usb-packets.txt", opts)?;
    zip.write_all(
        redact_bundle_text(&aggregate_usb_packets(
            &per_pico_captures,
            &retained_debug_packet_logs,
        ))
        .as_bytes(),
    )?;

    for log in &retained_debug_packet_logs {
        zip.start_file(format!("debug-packets/{}", log.name), opts)?;
        zip.write_all(redact_bundle_text(&log.text).as_bytes())?;
    }

    if let Some(text) = cache_current.as_ref() {
        zip.start_file("diagnostics/pico-state-current.json", opts)?;
        zip.write_all(redact_bundle_text(text).as_bytes())?;
    }
    if let Some(text) = cache_history.as_ref() {
        zip.start_file("diagnostics/pico-state-history.jsonl", opts)?;
        zip.write_all(redact_bundle_text(text).as_bytes())?;
    }

    for snapshot in &host_snapshots {
        zip.start_file(snapshot.manifest.path.as_str(), opts)?;
        zip.write_all(redact_bundle_text(&snapshot.text).as_bytes())?;
    }

    // system-info.txt: always present. Captures the Windows build,
    // couchlink version, last-known Pico identity, short hostname.
    zip.start_file("system-info.txt", opts)?;
    zip.write_all(redact_bundle_text(&system_info).as_bytes())?;

    // usb-devices.txt: pnputil dump if available (Windows 10 1903+),
    // otherwise a SetupAPI-via-serialport fallback so the bundle always
    // has *something* describing the USB topology at bundle time.
    if let Some((text, method)) = usb_devices.as_ref() {
        zip.start_file("usb-devices.txt", opts)?;
        zip.write_all(format!("# capture method: {method}\n\n").as_bytes())?;
        zip.write_all(redact_bundle_text(text).as_bytes())?;
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
        zip.write_all(redact_bundle_text(text).as_bytes())?;
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

    // Logs: last 5 couchlink.*.log (bridge, written by tracing-appender's
    // daily rotation as couchlink.YYYY-MM-DD.log) and last 5 setup-*.log
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
    tracing::info!(
        "bundle: run finished out_path={} per_pico={} usb_packets={} retained_debug_packets={} retained_debug_packet_logs={} host_snapshots={} cache_included={}",
        out_path.display(),
        per_pico_captures.len(),
        usb_packet_dump_count,
        retained_debug_packet_count,
        retained_debug_packet_logs.len(),
        host_snapshots.len(),
        diagnostic_cache_included,
    );

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
        usb_packet_dump_count,
        retained_debug_packet_log_count: retained_debug_packet_logs.len(),
        retained_debug_packet_count,
        per_pico_capture_count: per_pico_captures.len(),
        host_snapshot_count: host_snapshots.len(),
        diagnostic_cache_included,
    })
}

fn collect_retained_debug_packet_logs(capture_log: &mut CaptureLog) -> Vec<RetainedDebugPacketLog> {
    let mut out = Vec::new();
    for path in debug_packets::recent_packet_files(debug_packets::DEBUG_PACKET_FILE_RETENTION) {
        let started = Instant::now();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
        else {
            continue;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                capture_log.record(
                    format!("retained_debug_packet_log.{name}"),
                    started,
                    "included",
                    text.len(),
                    "",
                );
                out.push(RetainedDebugPacketLog { name, text });
            }
            Err(e) => {
                capture_log.record(
                    format!("retained_debug_packet_log.{name}"),
                    started,
                    "not_included",
                    0,
                    format!("{e:#}"),
                );
            }
        }
    }
    out
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

async fn capture_per_pico(capture_log: &mut CaptureLog) -> Vec<PicoBundleCapture> {
    let mut seeds: BTreeMap<String, PicoCaptureSeed> = BTreeMap::new();

    let started = Instant::now();
    match cmd_run::discover_picos(Duration::from_secs(2)).await {
        Ok(picos) => {
            capture_log.record(
                "per_pico.broadcast_discovery",
                started,
                "ok",
                picos.len(),
                format!("targets={}", picos.len()),
            );
            for target in picos {
                let uid = target.uid_hex();
                seeds
                    .entry(uid.clone())
                    .and_modify(|seed| {
                        seed.target = Some(target.clone());
                        seed.source = "broadcast discovery".to_string();
                    })
                    .or_insert(PicoCaptureSeed {
                        uid,
                        target: Some(target),
                        saved: None,
                        source: "broadcast discovery".to_string(),
                        cached_state_json: None,
                    });
            }
        }
        Err(e) => {
            capture_log.record(
                "per_pico.broadcast_discovery",
                started,
                "error",
                0,
                format!("{e:#}"),
            );
        }
    }

    let cfg = config::load().unwrap_or_default();
    for saved in saved_picos_from_config(&cfg) {
        let uid = saved.uid_hex();
        seeds
            .entry(uid.clone())
            .and_modify(|seed| {
                seed.saved = Some(saved.clone());
            })
            .or_insert(PicoCaptureSeed {
                uid: uid.clone(),
                target: None,
                saved: Some(saved.clone()),
                source: "saved config".to_string(),
                cached_state_json: None,
            });

        let already_live = seeds
            .get(&uid)
            .and_then(|seed| seed.target.as_ref())
            .is_some();
        if already_live {
            continue;
        }
        let Some(last_ip) = saved.last_ip.as_deref() else {
            continue;
        };
        let Some(ip) = cmd_run::parse_ip_selector(last_ip) else {
            capture_log.record(
                format!("per_pico.{uid}.last_known_ip_probe"),
                Instant::now(),
                "invalid_saved_ip",
                0,
                last_ip,
            );
            continue;
        };

        let started = Instant::now();
        match cmd_run::probe_pico_ip(ip, Duration::from_secs(2)).await {
            Ok(target) => {
                capture_log.record(
                    format!("per_pico.{uid}.last_known_ip_probe"),
                    started,
                    "ok",
                    1,
                    target.peer.to_string(),
                );
                seeds
                    .entry(uid.clone())
                    .and_modify(|seed| {
                        seed.target = Some(target.clone());
                        seed.source = "last-known IP probe".to_string();
                    })
                    .or_insert(PicoCaptureSeed {
                        uid,
                        target: Some(target),
                        saved: Some(saved),
                        source: "last-known IP probe".to_string(),
                        cached_state_json: None,
                    });
            }
            Err(e) => {
                capture_log.record(
                    format!("per_pico.{uid}.last_known_ip_probe"),
                    started,
                    "not_reachable",
                    0,
                    format!("{e:#}"),
                );
            }
        }
    }

    if let Some(cache) = pico_cache::read_current() {
        if let Some(uid) = uid_from_cache_json(&cache) {
            seeds
                .entry(uid.clone())
                .and_modify(|seed| {
                    seed.cached_state_json = Some(cache.clone());
                })
                .or_insert(PicoCaptureSeed {
                    uid,
                    target: None,
                    saved: None,
                    source: "diagnostic cache".to_string(),
                    cached_state_json: Some(cache),
                });
        }
    }

    let mut captures = Vec::new();
    for seed in seeds.into_values() {
        captures.push(capture_one_pico(seed, capture_log).await);
    }
    captures
}

async fn capture_one_pico(
    seed: PicoCaptureSeed,
    capture_log: &mut CaptureLog,
) -> PicoBundleCapture {
    let path = format!("picos/{}", sanitize_path_component(&seed.uid));
    let state_captured: bool;
    let pico_diag_status: String;
    let usb_diag_status: String;
    let pico_state_status: String;
    let pico_diag_text: String;
    let usb_diag_text: String;
    let usb_packets_text: String;

    let state_json = if let Some(target) = seed.target.as_ref() {
        state_captured = true;
        let mut snapshot = pico_cache::PicoStateSnapshot::from_target("bundle", target);
        let target_pico_state_status;
        let target_usb_diag_status;

        let started = Instant::now();
        match pico_state::query_pico_state(target, Duration::from_millis(900)).await {
            Ok(state) => {
                target_pico_state_status = "captured".to_string();
                snapshot = snapshot.with_pico_state(&state);
                capture_log.record(
                    format!("per_pico.{}.pico_state", seed.uid),
                    started,
                    "captured",
                    crate::protocol::PICO_STATE_WIRE_SIZE,
                    "",
                );
            }
            Err(e) => {
                target_pico_state_status = "timeout_or_unsupported".to_string();
                capture_log.record(
                    format!("per_pico.{}.pico_state", seed.uid),
                    started,
                    "timeout_or_unsupported",
                    0,
                    format!("{e:#}"),
                );
            }
        }

        let started = Instant::now();
        let diag = pico_diag::capture_run_udp_for_target(target).await;
        let target_pico_diag_status = diag.discriminant_str().to_string();
        let target_pico_diag_text = diag.stub_text();
        capture_log.record(
            format!("per_pico.{}.pico_diag", seed.uid),
            started,
            &target_pico_diag_status,
            target_pico_diag_text.len(),
            diag.source_str().unwrap_or(""),
        );

        let started = Instant::now();
        let target_usb_diag_text =
            match cmd_usb_diag::query_usb_diag(target, Duration::from_secs(3)).await {
                Ok(diag) => {
                    target_usb_diag_status = "captured".to_string();
                    let text = cmd_usb_diag::format_usb_diag(&diag, target.persona);
                    snapshot = snapshot.with_usb_diag(&diag, target.persona);
                    capture_log.record(
                        format!("per_pico.{}.usb_diag", seed.uid),
                        started,
                        "captured",
                        text.len(),
                        "",
                    );
                    text
                }
                Err(e) => {
                    target_usb_diag_status = "not_captured".to_string();
                    let text = format!(
                    "Structured Pico USB diagnostics were not captured for {}.\n\nerror={e:#}\n",
                    target.detail_label()
                );
                    capture_log.record(
                        format!("per_pico.{}.usb_diag", seed.uid),
                        started,
                        "not_captured",
                        text.len(),
                        format!("{e:#}"),
                    );
                    text
                }
            };

        snapshot = snapshot.with_outcome(format!(
            "bundle: pico_state={target_pico_state_status}; pico_diag={target_pico_diag_status}; usb_diag={target_usb_diag_status}"
        ));
        pico_cache::record(snapshot.clone());
        pico_state_status = target_pico_state_status;
        pico_diag_status = target_pico_diag_status;
        pico_diag_text = target_pico_diag_text;
        usb_packets_text = usb_packets_text_from_diag(&seed.uid, &pico_diag_text);
        usb_diag_status = target_usb_diag_status;
        usb_diag_text = target_usb_diag_text;
        state_json_from_snapshot(&snapshot)
    } else if let Some(saved) = seed.saved.as_ref() {
        state_captured = true;
        let snapshot = pico_cache::PicoStateSnapshot::offline_from_config("bundle-offline", saved);
        pico_cache::record(snapshot.clone());
        pico_diag_status = "offline_not_attempted".to_string();
        usb_diag_status = "offline_not_attempted".to_string();
        pico_state_status = "offline_not_attempted".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        state_json_from_snapshot(&snapshot)
    } else if let Some(cached) = seed.cached_state_json.as_ref() {
        state_captured = true;
        pico_diag_status = "cache_only_not_attempted".to_string();
        usb_diag_status = "cache_only_not_attempted".to_string();
        pico_state_status = "cache_only".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        cached.clone()
    } else {
        state_captured = false;
        pico_diag_status = "no_state".to_string();
        usb_diag_status = "no_state".to_string();
        pico_state_status = "no_state".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        "{}\n".to_string()
    };
    let usb_packet_count = count_usb_packet_lines(&usb_packets_text);
    let usb_packet_status = if usb_packet_count > 0 {
        "captured"
    } else if seed.target.is_some() {
        "no_packets"
    } else {
        "not_attempted"
    };
    capture_log.record_duration(
        format!("per_pico.{}.usb_packets", seed.uid),
        0,
        usb_packet_status,
        usb_packets_text.len(),
        format!("count={usb_packet_count}"),
    );

    PicoBundleCapture {
        manifest: ManifestPicoCapture {
            uid: seed.uid,
            path,
            peer: seed.target.as_ref().map(|target| target.peer.to_string()),
            live: seed.target.is_some(),
            source: seed.source,
            state_captured,
            pico_diag_status,
            usb_diag_status,
            pico_state_status,
            usb_packet_dump_status: usb_packet_status.to_string(),
            usb_packet_dump_count: usb_packet_count,
            cached_state_included: seed.cached_state_json.is_some(),
        },
        state_json,
        pico_diag_text,
        usb_diag_text,
        usb_packets_text,
    }
}

fn saved_picos_from_config(cfg: &config::Config) -> Vec<config::PicoIdentity> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for pico in &cfg.picos {
        if seen.insert(pico.unique_id_short) {
            out.push(pico.clone());
        }
    }
    if let Some(pico) = cfg.last_pico.as_ref() {
        if seen.insert(pico.unique_id_short) {
            out.push(pico.clone());
        }
    }
    out
}

fn uid_from_cache_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(uid) = value.get("uid").and_then(|v| v.as_str()) {
        return Some(sanitize_path_component(uid));
    }
    let uid = value.get("unique_id_short").and_then(|v| v.as_u64())?;
    Some(format!("{:08X}", uid as u32))
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn state_json_from_snapshot(snapshot: &pico_cache::PicoStateSnapshot) -> String {
    serde_json::to_string_pretty(snapshot).unwrap_or_else(|e| {
        format!(
            "{{\"capture_outcome\":\"state_serialization_failed\",\"error\":\"{}\"}}\n",
            e
        )
    })
}

fn offline_pico_text(seed: &PicoCaptureSeed, artifact: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{artifact} was not captured because this Pico was not reachable during bundle capture."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "uid={}", seed.uid);
    let _ = writeln!(out, "source={}", seed.source);
    if let Some(saved) = seed.saved.as_ref() {
        let _ = writeln!(out, "saved_board={}", saved.board_label());
        let _ = writeln!(out, "saved_firmware={}", saved.firmware_version());
        if let Some(ip) = saved.last_ip.as_deref() {
            let _ = writeln!(out, "last_known_ip={ip}");
        }
    }
    if seed.cached_state_json.is_some() {
        let _ = writeln!(out, "cached_state_available=true");
    }
    out
}

fn usb_packets_text_from_diag(uid: &str, diag_text: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Raw USB OUT packet dump extracted from firmware diagnostics"
    );
    let _ = writeln!(out, "# uid={uid}");
    let _ = writeln!(
        out,
        "# These lines are present only when the Pico is in debug input mode."
    );
    let _ = writeln!(out);
    let mut count = 0usize;
    for line in diag_text.lines() {
        if let Some(idx) = line.find("usb-packet ") {
            out.push_str(&line[idx..]);
            out.push('\n');
            count += 1;
        }
    }
    if count == 0 {
        let _ = writeln!(
            out,
            "No usb-packet lines were present. Switch the Pico to debug input mode, reproduce the adapter traffic, then run bundle again."
        );
    }
    out
}

fn count_usb_packet_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("usb-packet "))
        .count()
}

fn count_retained_debug_packet_lines(logs: &[RetainedDebugPacketLog]) -> usize {
    logs.iter()
        .flat_map(|log| log.text.lines())
        .filter(|line| line.starts_with("usb-packet "))
        .count()
}

fn aggregate_usb_packets(
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
) -> String {
    let mut out = String::from("# Aggregate raw USB OUT packet dump\n\n");
    let mut total = 0usize;
    for capture in captures {
        let count = capture.manifest.usb_packet_dump_count;
        let _ = writeln!(
            out,
            "## {} packets={} path={}/usb-packets.txt",
            capture.manifest.uid, count, capture.manifest.path
        );
        if count > 0 {
            for line in capture.usb_packets_text.lines() {
                if line.starts_with("usb-packet ") {
                    out.push_str(line);
                    out.push('\n');
                    total += 1;
                }
            }
        }
        out.push('\n');
    }
    if !retained_logs.is_empty() {
        out.push_str("## retained host debug packet logs\n");
        for log in retained_logs {
            let _ = writeln!(out, "### debug-packets/{}", log.name);
            for line in log.text.lines() {
                if line.starts_with("usb-packet ") {
                    out.push_str(line);
                    out.push('\n');
                    total += 1;
                }
            }
            out.push('\n');
        }
    }
    if total == 0 {
        out.push_str("No raw USB OUT packets were captured in this bundle.\n");
    }
    out
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
    let total_packet_count = summary.usb_packet_dump_count + summary.retained_debug_packet_count;
    if total_packet_count > 0 {
        println!(
            "  usb-packets.txt: captured {} raw USB OUT packet(s)",
            total_packet_count
        );
    } else {
        println!("  usb-packets.txt: no raw packets captured (debug input mode only)");
    }
    if summary.retained_debug_packet_log_count > 0 {
        println!(
            "  debug-packets/: {} retained packet log(s)",
            summary.retained_debug_packet_log_count
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

#[cfg(test)]
mod tests {
    use super::{
        aggregate_usb_packets, count_usb_packet_lines, sanitize_path_component,
        usb_packets_text_from_diag, RetainedDebugPacketLog,
    };

    #[test]
    fn pico_bundle_path_component_is_sanitized() {
        assert_eq!(sanitize_path_component("02E22DA9"), "02E22DA9");
        assert_eq!(sanitize_path_component("../02:E2\\2D/A9"), "02E22DA9");
        assert_eq!(sanitize_path_component(""), "unknown");
    }

    #[test]
    fn extracts_usb_packet_lines_from_diag_log() {
        let diag = "[      10] boot\n[      11] usb-packet seq=0 dir=out len=3 data=010203\n";
        let out = usb_packets_text_from_diag("02E22DA9", diag);
        assert!(out.contains("usb-packet seq=0 dir=out len=3 data=010203"));
        assert_eq!(count_usb_packet_lines(&out), 1);
    }

    #[test]
    fn aggregate_usb_packets_includes_retained_host_logs() {
        let retained = vec![RetainedDebugPacketLog {
            name: "usb-packets-20260615-214000-02E22DA9.log".to_string(),
            text: "# header\nusb-packet seq=4 dir=out data=010203\n".to_string(),
        }];
        let out = aggregate_usb_packets(&[], &retained);
        assert!(out.contains("debug-packets/usb-packets-20260615-214000-02E22DA9.log"));
        assert!(out.contains("usb-packet seq=4 dir=out data=010203"));
        assert!(!out.contains("No raw USB OUT packets"));
    }
}
