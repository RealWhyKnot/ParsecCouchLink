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
mod usb_packet_summary;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::{
    cmd_run, cmd_usb_diag, config, debug_packets, journal, pico_cache, pico_state, protocol,
};

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
use usb_packet_summary::{
    control_transfers_text_for_sources, control_transfers_text_for_text,
    hid_reports_text_for_sources, hid_reports_text_for_text, packet_timeline_text_for_sources,
    packet_timeline_text_for_text, records_jsonl_for_sources, records_jsonl_for_text,
    summarize_sources, summarize_text, UsbPacketBundleSummary, UsbPacketSummary,
    UsbPacketSummarySource,
};

const BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT: Duration = Duration::from_secs(2);

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
    pub debug_capture_status: String,
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
    let per_pico_packet_sources: Vec<_> = per_pico_captures
        .iter()
        .map(|capture| UsbPacketSummarySource {
            label: capture.manifest.uid.clone(),
            path: format!("{}/usb-packets.txt", capture.manifest.path),
            text: &capture.usb_packets_text,
        })
        .collect();
    let retained_packet_sources: Vec<_> = retained_debug_packet_logs
        .iter()
        .map(|log| UsbPacketSummarySource {
            label: log.name.clone(),
            path: format!("debug-packets/{}", log.name),
            text: &log.text,
        })
        .collect();
    let usb_packet_summary = summarize_sources(&per_pico_packet_sources, &retained_packet_sources);
    let usb_packet_summary_json = serde_json::to_string_pretty(&usb_packet_summary)?;
    let usb_packet_records_jsonl =
        records_jsonl_for_sources(&per_pico_packet_sources, &retained_packet_sources)?;
    let usb_control_transfers_text =
        control_transfers_text_for_sources(&per_pico_packet_sources, &retained_packet_sources);
    let usb_hid_reports_text =
        hid_reports_text_for_sources(&per_pico_packet_sources, &retained_packet_sources);
    let usb_packet_timeline_text =
        packet_timeline_text_for_sources(&per_pico_packet_sources, &retained_packet_sources);
    let debug_capture_status = debug_capture_overall_status(
        &usb_packet_summary,
        &per_pico_captures,
        &retained_debug_packet_logs,
    );
    let debug_capture_verdict = debug_capture_verdict_text(
        &per_pico_captures,
        &retained_debug_packet_logs,
        &usb_packet_summary,
    );
    let debug_capture_evidence_json = debug_capture_evidence_report_json(
        &per_pico_captures,
        &retained_debug_packet_logs,
        &usb_packet_summary,
    )?;
    capture_log.record_duration(
        "usb_packet_summary",
        0,
        "included",
        usb_packet_summary_json.len(),
        format!(
            "raw_packets={}; stats={}",
            usb_packet_summary.aggregate.packet_lines, usb_packet_summary.aggregate.stats_lines
        ),
    );
    capture_log.record_duration(
        "usb_packet_records",
        0,
        "included",
        usb_packet_records_jsonl.len(),
        "jsonl",
    );
    capture_log.record_duration(
        "usb_control_transfers",
        0,
        "included",
        usb_control_transfers_text.len(),
        "setup_and_control_in",
    );
    capture_log.record_duration(
        "usb_hid_reports",
        0,
        "included",
        usb_hid_reports_text.len(),
        "hid_report_metadata",
    );
    capture_log.record_duration(
        "usb_packet_timeline",
        0,
        "included",
        usb_packet_timeline_text.len(),
        "packet_timing",
    );
    capture_log.record_duration(
        "debug_capture_verdict",
        0,
        "included",
        debug_capture_verdict.len(),
        debug_capture_status,
    );
    capture_log.record_duration(
        "debug_capture_evidence",
        0,
        "included",
        debug_capture_evidence_json.len(),
        "json",
    );

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

        zip.start_file(format!("{base}/usb-packets-summary.json"), opts)?;
        let summary_json = serde_json::to_string_pretty(&summarize_text(&pico.usb_packets_text))?;
        zip.write_all(redact_bundle_text(&summary_json).as_bytes())?;

        zip.start_file(format!("{base}/usb-packets.jsonl"), opts)?;
        let records_jsonl = records_jsonl_for_text(
            &pico.manifest.uid,
            &format!("{base}/usb-packets.txt"),
            &pico.usb_packets_text,
        )?;
        zip.write_all(redact_bundle_text(&records_jsonl).as_bytes())?;

        zip.start_file(format!("{base}/usb-control-transfers.txt"), opts)?;
        let control_transfers = control_transfers_text_for_text(
            &pico.manifest.uid,
            &format!("{base}/usb-packets.txt"),
            &pico.usb_packets_text,
        );
        zip.write_all(redact_bundle_text(&control_transfers).as_bytes())?;

        zip.start_file(format!("{base}/usb-hid-reports.txt"), opts)?;
        let hid_reports = hid_reports_text_for_text(
            &pico.manifest.uid,
            &format!("{base}/usb-packets.txt"),
            &pico.usb_packets_text,
        );
        zip.write_all(redact_bundle_text(&hid_reports).as_bytes())?;

        zip.start_file(format!("{base}/usb-packet-timeline.txt"), opts)?;
        let packet_timeline = packet_timeline_text_for_text(
            &pico.manifest.uid,
            &format!("{base}/usb-packets.txt"),
            &pico.usb_packets_text,
        );
        zip.write_all(redact_bundle_text(&packet_timeline).as_bytes())?;
    }

    zip.start_file("usb-packets.txt", opts)?;
    zip.write_all(
        redact_bundle_text(&aggregate_usb_packets(
            &per_pico_captures,
            &retained_debug_packet_logs,
        ))
        .as_bytes(),
    )?;

    zip.start_file("usb-packets-summary.json", opts)?;
    zip.write_all(redact_bundle_text(&usb_packet_summary_json).as_bytes())?;

    zip.start_file("usb-packets.jsonl", opts)?;
    zip.write_all(redact_bundle_text(&usb_packet_records_jsonl).as_bytes())?;

    zip.start_file("usb-control-transfers.txt", opts)?;
    zip.write_all(redact_bundle_text(&usb_control_transfers_text).as_bytes())?;

    zip.start_file("usb-hid-reports.txt", opts)?;
    zip.write_all(redact_bundle_text(&usb_hid_reports_text).as_bytes())?;

    zip.start_file("usb-packet-timeline.txt", opts)?;
    zip.write_all(redact_bundle_text(&usb_packet_timeline_text).as_bytes())?;

    zip.start_file("debug-capture-verdict.txt", opts)?;
    zip.write_all(redact_bundle_text(&debug_capture_verdict).as_bytes())?;

    zip.start_file("debug-capture-evidence.json", opts)?;
    zip.write_all(redact_bundle_text(&debug_capture_evidence_json).as_bytes())?;

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
        debug_capture_status: debug_capture_status.to_string(),
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
        usb_packets_text =
            bundle_usb_packets_for_target(&seed.uid, target, &pico_diag_text, capture_log).await;
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
    let usb_packet_stats_count = count_usb_packet_stats_lines(&usb_packets_text);
    let usb_packet_harvest_count = count_usb_packet_harvest_lines(&usb_packets_text);
    let usb_packet_status = if usb_packet_count > 0 {
        "captured"
    } else if usb_packet_stats_count > 0 {
        "stats_only"
    } else if usb_packet_harvest_count > 0 {
        "harvest_only"
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
        format!(
            "count={usb_packet_count}; stats={usb_packet_stats_count}; harvest={usb_packet_harvest_count}"
        ),
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

async fn bundle_usb_packets_for_target(
    uid: &str,
    target: &cmd_run::PicoTarget,
    fallback_diag_text: &str,
    capture_log: &mut CaptureLog,
) -> String {
    if target.persona != protocol::Persona::Debug {
        return usb_packets_text_from_diag(uid, fallback_diag_text);
    }

    let started = Instant::now();
    match debug_packets::capture_run_diag_log(target.peer, BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT)
        .await
    {
        Ok(snapshot) => {
            let duration_ms = duration_ms_u64(started.elapsed());
            let text = usb_packets_text_from_debug_snapshot(uid, &snapshot, duration_ms);
            capture_log.record(
                format!("per_pico.{uid}.debug_packet_harvest"),
                started,
                "captured",
                text.len(),
                format!(
                    "chunks={}; missing_chunks={}; lost_bytes={}; diag_bytes={}",
                    snapshot.chunk_count,
                    snapshot.missing_chunks.len(),
                    snapshot.lost_bytes,
                    snapshot.byte_count
                ),
            );
            text
        }
        Err(e) => {
            let duration_ms = duration_ms_u64(started.elapsed());
            let mut text = usb_packets_text_from_diag(uid, fallback_diag_text);
            text.push_str(&debug_packets::harvest_error_line(
                duration_ms,
                &format!("{e:#}"),
            ));
            text.push('\n');
            capture_log.record(
                format!("per_pico.{uid}.debug_packet_harvest"),
                started,
                "error",
                text.len(),
                format!("{e:#}"),
            );
            text
        }
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
    let lines = diag_text
        .lines()
        .filter_map(|line| usb_packet_line_index(line).map(|idx| line[idx..].to_string()))
        .collect::<Vec<_>>();
    usb_packets_text_from_lines(uid, &lines, None)
}

fn usb_packets_text_from_debug_snapshot(
    uid: &str,
    snapshot: &debug_packets::DiagLogSnapshot,
    duration_ms: u64,
) -> String {
    let lines = debug_packets::extract_usb_packet_lines(&snapshot.text);
    let raw_packet_lines = lines
        .iter()
        .filter(|line| line.starts_with("usb-packet "))
        .count();
    let stats_lines = lines
        .iter()
        .filter(|line| line.starts_with("usb-packet-stats "))
        .count();
    let harvest = debug_packets::HarvestOkRecord {
        duration_ms,
        snapshot: snapshot.clone(),
        packet_lines: lines.len(),
        raw_packet_lines,
        stats_lines,
        new_lines: lines.len(),
    };
    let harvest_line = debug_packets::harvest_ok_line(&harvest, lines.len());
    usb_packets_text_from_lines(uid, &lines, Some(harvest_line.as_str()))
}

fn usb_packets_text_from_lines(uid: &str, lines: &[String], harvest_line: Option<&str>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Raw USB packet dump extracted from firmware diagnostics"
    );
    let _ = writeln!(out, "# uid={uid}");
    let _ = writeln!(
        out,
        "# These lines are present only when the Pico is in debug input mode."
    );
    let _ = writeln!(out);
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    if lines.is_empty() {
        let _ = writeln!(
            out,
            "No usb-packet lines were present. Switch the Pico to debug input mode, reproduce the adapter traffic, then run bundle again."
        );
    }
    if let Some(line) = harvest_line {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn count_usb_packet_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("usb-packet "))
        .count()
}

fn count_usb_packet_stats_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("usb-packet-stats "))
        .count()
}

fn count_usb_packet_harvest_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("# harvest "))
        .count()
}

fn usb_packet_line_index(line: &str) -> Option<usize> {
    line.find("usb-packet ")
        .or_else(|| line.find("usb-packet-stats "))
}

fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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
    let mut out = String::from("# Aggregate USB packet capture evidence\n\n");
    let mut raw_total = 0usize;
    let mut stats_total = 0usize;
    let mut harvest_total = 0usize;
    let mut diagnostic_total = 0usize;
    for capture in captures {
        let count = capture.manifest.usb_packet_dump_count;
        let _ = writeln!(
            out,
            "## {} packets={} path={}/usb-packets.txt",
            capture.manifest.uid, count, capture.manifest.path
        );
        for line in capture.usb_packets_text.lines() {
            if is_usb_packet_diagnostic_line(line) {
                out.push_str(line);
                out.push('\n');
                diagnostic_total += 1;
                if line.starts_with("usb-packet ") {
                    raw_total += 1;
                } else if line.starts_with("usb-packet-stats ") {
                    stats_total += 1;
                } else if line.starts_with("# harvest ") {
                    harvest_total += 1;
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
                if is_usb_packet_diagnostic_line(line) {
                    out.push_str(line);
                    out.push('\n');
                    diagnostic_total += 1;
                    if line.starts_with("usb-packet ") {
                        raw_total += 1;
                    } else if line.starts_with("usb-packet-stats ") {
                        stats_total += 1;
                    } else if line.starts_with("# harvest ") {
                        harvest_total += 1;
                    }
                }
            }
            out.push('\n');
        }
    }
    if diagnostic_total == 0 {
        out.push_str("No raw USB packets were captured in this bundle.\n");
    } else if raw_total == 0 {
        match (stats_total > 0, harvest_total > 0) {
            (true, true) => out.push_str(
                "No raw USB packet payload lines were captured, but packet stats and harvest records were present.\n",
            ),
            (true, false) => out.push_str(
                "No raw USB packet payload lines were captured, but packet stats were present.\n",
            ),
            (false, true) => out.push_str(
                "No raw USB packet payload lines were captured, but harvest records were present.\n",
            ),
            (false, false) => {}
        }
    }
    out
}

fn debug_capture_verdict_text(
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
    summary: &UsbPacketBundleSummary,
) -> String {
    let status = debug_capture_overall_status(summary, captures, retained_logs);
    let evidence_grade = debug_capture_evidence_grade(summary);
    let (gate, gate_reason) = debug_capture_gate(summary);
    let endpoint_in_lines = debug_summary_direction_count(summary, "in");
    let endpoint_out_lines = debug_summary_direction_count(summary, "out");
    let setup_lines = debug_summary_direction_count(summary, "setup");
    let control_in_lines = debug_summary_direction_count(summary, "control-in");
    let hid_report_lines = summary.aggregate.hid_report_lines;
    let debug_persona_captures = captures
        .iter()
        .filter(|capture| state_json_persona(&capture.state_json).as_deref() == Some("debug"))
        .count();
    let mut out = String::from("Debug input packet capture verdict\n\n");
    let _ = writeln!(out, "overall_status={status}");
    let _ = writeln!(out, "evidence_grade={evidence_grade}");
    let _ = writeln!(out, "adapter_reverse_engineering_gate={gate}");
    let _ = writeln!(out, "gate_reason={gate_reason}");
    let _ = writeln!(out, "raw_packet_lines={}", summary.aggregate.packet_lines);
    let _ = writeln!(out, "packet_stats_lines={}", summary.aggregate.stats_lines);
    let _ = writeln!(out, "harvest_lines={}", summary.aggregate.harvest_lines);
    let _ = writeln!(
        out,
        "packet_time_span_ms={}",
        summary.aggregate.packet_time_span_ms.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "max_inter_packet_gap_ms={}",
        summary.aggregate.max_inter_packet_gap_ms.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "packet_time_regressions={}",
        summary.aggregate.packet_time_regressions
    );
    let _ = writeln!(
        out,
        "harvest_chunk_statuses={}",
        format_count_map(&summary.aggregate.harvest_chunk_statuses)
    );
    let _ = writeln!(
        out,
        "max_harvest_missing_chunks={}",
        summary.aggregate.max_harvest_missing_chunks.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "max_harvest_duplicate_chunks={}",
        summary.aggregate.max_harvest_duplicate_chunks.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "max_harvest_diag_bytes={}",
        summary.aggregate.max_harvest_diag_bytes.unwrap_or(0)
    );
    let _ = writeln!(out, "endpoint_in_lines={endpoint_in_lines}");
    let _ = writeln!(out, "endpoint_out_lines={endpoint_out_lines}");
    let _ = writeln!(out, "setup_lines={setup_lines}");
    let _ = writeln!(out, "control_in_lines={control_in_lines}");
    let _ = writeln!(out, "hid_report_lines={hid_report_lines}");
    let _ = writeln!(
        out,
        "hid_report_types={}",
        format_count_map(&summary.aggregate.hid_report_types)
    );
    let _ = writeln!(
        out,
        "hid_report_ids={}",
        format_count_map(&summary.aggregate.hid_report_ids)
    );
    let _ = writeln!(out, "debug_persona_captures={debug_persona_captures}");
    let _ = writeln!(
        out,
        "harvest_statuses={}",
        format_count_map(&summary.aggregate.harvest_statuses)
    );
    let _ = writeln!(out, "retained_debug_packet_logs={}", retained_logs.len());
    let _ = writeln!(out, "per_pico_captures={}", captures.len());
    let _ = writeln!(out);

    out.push_str("minimum_evidence=\n");
    out.push_str("- raw_packet_lines > 0 is required before this bundle is enough for adapter reverse engineering.\n");
    out.push_str("- setup_lines or control_in_lines > 0 is preferred for enumeration/control-transfer failures.\n");
    out.push_str("- hid_report_lines > 0 is useful for HID-class adapter report analysis.\n");
    out.push_str(
        "- endpoint_in_lines or endpoint_out_lines > 0 is preferred for runtime adapter traffic.\n",
    );
    out.push_str("- debug_persona_captures > 0 proves the Pico was in debug input mode when bundle captured current state.\n");
    let _ = writeln!(out);

    out.push_str("missing_evidence=\n");
    for line in debug_capture_missing_evidence_lines(
        summary,
        captures,
        retained_logs,
        debug_persona_captures,
    ) {
        let _ = writeln!(out, "- {line}");
    }
    let _ = writeln!(out);

    out.push_str("meaning=");
    match status {
        "raw_packets_captured" => out.push_str(
            "This bundle contains raw debug input USB packets for adapter reverse engineering.",
        ),
        "debug_stats_only" => out.push_str(
            "Debug input packet counters were present, but raw packet payload lines were not retained.",
        ),
        "harvest_attempted_no_packets" => out.push_str(
            "The bridge attempted retained debug packet harvests, but no raw packet payload lines were captured.",
        ),
        "retained_logs_without_packet_lines" => out.push_str(
            "Retained debug packet log files exist, but they did not contain packet, stats, or harvest records.",
        ),
        "live_picos_no_packet_evidence" => out.push_str(
            "At least one Pico was reachable, but the captured diagnostics did not include debug input packet evidence.",
        ),
        "only_offline_or_cached_picos" => out.push_str(
            "No Pico was reachable during bundle capture; packet evidence can only come from retained debug logs.",
        ),
        _ => out.push_str("No Pico or retained debug packet evidence was present in this bundle."),
    }
    let _ = writeln!(out);
    let _ = writeln!(out);

    out.push_str("next_steps=\n");
    if summary.aggregate.packet_lines == 0 {
        out.push_str(
            "- Switch the target Pico to debug input mode, reproduce adapter traffic, then run bundle again.\n",
        );
        out.push_str(
            "- Keep the bridge stream running while reproducing the issue so retained debug packet harvests can write debug-packets/*.log.\n",
        );
        out.push_str(
            "- If harvest_statuses shows error, check bundle-capture.txt and debug-packets/*.log for GET_LOG failures and timing.\n",
        );
    } else {
        out.push_str(
            "- Use usb-packets.jsonl for scripts, usb-packet-timeline.txt for timing, usb-hid-reports.txt for HID report traffic, and usb-packets-summary.json for sequence, direction, truncation, and harvest health totals.\n",
        );
    }
    let _ = writeln!(out);

    out.push_str("per_pico=\n");
    if captures.is_empty() {
        out.push_str("- none\n");
    } else {
        for capture in captures {
            let persona =
                state_json_persona(&capture.state_json).unwrap_or_else(|| "unknown".to_string());
            let _ = writeln!(
                out,
                "- uid={} live={} peer={} persona={} source={} pico_state={} pico_diag={} usb_diag={} packet_status={} raw_packets={} path={}",
                capture.manifest.uid,
                capture.manifest.live,
                capture.manifest.peer.as_deref().unwrap_or("none"),
                persona,
                capture.manifest.source,
                capture.manifest.pico_state_status,
                capture.manifest.pico_diag_status,
                capture.manifest.usb_diag_status,
                capture.manifest.usb_packet_dump_status,
                capture.manifest.usb_packet_dump_count,
                capture.manifest.path
            );
        }
    }
    let _ = writeln!(out);

    out.push_str("retained_logs=\n");
    if retained_logs.is_empty() {
        out.push_str("- none\n");
    } else {
        for log in retained_logs {
            let log_summary = summarize_text(&log.text);
            let _ = writeln!(
                out,
                "- path=debug-packets/{} raw_packets={} stats={} hid_reports={} max_gap_ms={} harvest_lines={} harvest_statuses={} chunk_statuses={} max_missing_chunks={} max_diag_bytes={}",
                log.name,
                log_summary.packet_lines,
                log_summary.stats_lines,
                log_summary.hid_report_lines,
                log_summary.max_inter_packet_gap_ms.unwrap_or(0),
                log_summary.harvest_lines,
                format_count_map(&log_summary.harvest_statuses),
                format_count_map(&log_summary.harvest_chunk_statuses),
                log_summary.max_harvest_missing_chunks.unwrap_or(0),
                log_summary.max_harvest_diag_bytes.unwrap_or(0)
            );
        }
    }

    out
}

#[derive(Serialize)]
struct DebugCaptureEvidenceReport {
    artifact_schema_version: u8,
    overall_status: &'static str,
    evidence_grade: &'static str,
    adapter_reverse_engineering_gate: &'static str,
    gate_reason: &'static str,
    missing_evidence: Vec<&'static str>,
    aggregate: UsbPacketSummary,
    per_pico: Vec<DebugCaptureEvidencePico>,
    retained_logs: Vec<DebugCaptureEvidenceRetainedLog>,
    notes: Vec<&'static str>,
}

#[derive(Serialize)]
struct DebugCaptureEvidencePico {
    uid: String,
    path: String,
    peer: Option<String>,
    live: bool,
    source: String,
    persona: Option<String>,
    packet_status: String,
    missing_evidence: Vec<&'static str>,
    summary: UsbPacketSummary,
}

#[derive(Serialize)]
struct DebugCaptureEvidenceRetainedLog {
    name: String,
    path: String,
    missing_evidence: Vec<&'static str>,
    summary: UsbPacketSummary,
}

fn debug_capture_evidence_report_json(
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
    summary: &UsbPacketBundleSummary,
) -> Result<String> {
    let status = debug_capture_overall_status(summary, captures, retained_logs);
    let evidence_grade = debug_capture_evidence_grade(summary);
    let (gate, gate_reason) = debug_capture_gate(summary);
    let debug_persona_captures = captures
        .iter()
        .filter(|capture| state_json_persona(&capture.state_json).as_deref() == Some("debug"))
        .count();
    let missing_evidence = debug_capture_missing_evidence_lines(
        summary,
        captures,
        retained_logs,
        debug_persona_captures,
    );
    let per_pico = captures
        .iter()
        .map(|capture| {
            let persona = state_json_persona(&capture.state_json);
            let source_summary = summarize_text(&capture.usb_packets_text);
            DebugCaptureEvidencePico {
                uid: capture.manifest.uid.clone(),
                path: capture.manifest.path.clone(),
                peer: capture.manifest.peer.clone(),
                live: capture.manifest.live,
                source: capture.manifest.source.clone(),
                persona: persona.clone(),
                packet_status: capture.manifest.usb_packet_dump_status.clone(),
                missing_evidence: debug_capture_source_missing_evidence(
                    &source_summary,
                    persona.as_deref(),
                    false,
                ),
                summary: source_summary,
            }
        })
        .collect();
    let retained_logs = retained_logs
        .iter()
        .map(|log| {
            let source_summary = summarize_text(&log.text);
            DebugCaptureEvidenceRetainedLog {
                name: log.name.clone(),
                path: format!("debug-packets/{}", log.name),
                missing_evidence: debug_capture_source_missing_evidence(
                    &source_summary,
                    None,
                    true,
                ),
                summary: source_summary,
            }
        })
        .collect();
    let report = DebugCaptureEvidenceReport {
        artifact_schema_version: 1,
        overall_status: status,
        evidence_grade,
        adapter_reverse_engineering_gate: gate,
        gate_reason,
        missing_evidence,
        aggregate: summary.aggregate.clone(),
        per_pico,
        retained_logs,
        notes: vec![
            "This file is machine-readable evidence for debug input packet capture quality.",
            "adapter_reverse_engineering_gate=pass requires raw debug input packet payload lines.",
            "Per-source summary counts are calculated independently; aggregate sequence gaps are summed per source.",
            "Raw packet dumps are only present when the Pico was intentionally switched into debug input mode.",
        ],
    };
    Ok(serde_json::to_string_pretty(&report)?)
}

fn debug_capture_source_missing_evidence(
    summary: &UsbPacketSummary,
    persona: Option<&str>,
    retained_log: bool,
) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if summary.packet_lines == 0 {
        lines.push("raw USB packet payload lines from this source");
    }
    if summary.directions.get("setup").copied().unwrap_or(0) == 0
        && summary.directions.get("control-in").copied().unwrap_or(0) == 0
    {
        lines.push("USB setup/control-IN traffic from this source");
    }
    if summary.directions.get("in").copied().unwrap_or(0) == 0
        && summary.directions.get("out").copied().unwrap_or(0) == 0
    {
        lines.push("endpoint IN/OUT traffic from this source");
    }
    if !retained_log && persona != Some("debug") {
        lines.push("current state proving persona=debug for this Pico");
    }
    if summary.packet_lines == 0 && summary.stats_lines == 0 && summary.harvest_lines == 0 {
        lines.push("debug packet stats or harvest records from this source");
    }
    if lines.is_empty() {
        lines.push("none");
    }
    lines
}

fn debug_capture_evidence_grade(summary: &UsbPacketBundleSummary) -> &'static str {
    if summary.aggregate.packet_lines > 0
        && (debug_summary_direction_count(summary, "setup") > 0
            || debug_summary_direction_count(summary, "control-in") > 0)
        && (debug_summary_direction_count(summary, "in") > 0
            || debug_summary_direction_count(summary, "out") > 0)
    {
        "complete"
    } else if summary.aggregate.packet_lines > 0 {
        "usable_raw_packets"
    } else if summary.aggregate.stats_lines > 0 || summary.aggregate.harvest_lines > 0 {
        "partial_no_payloads"
    } else {
        "missing"
    }
}

fn debug_capture_gate(summary: &UsbPacketBundleSummary) -> (&'static str, &'static str) {
    if summary.aggregate.packet_lines > 0 {
        ("pass", "raw debug input packet payload lines are present")
    } else {
        ("fail", "raw debug input packet payload lines are missing")
    }
}

fn debug_capture_missing_evidence_lines(
    summary: &UsbPacketBundleSummary,
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
    debug_persona_captures: usize,
) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if summary.aggregate.packet_lines == 0 {
        lines.push("raw USB packet payload lines from debug input mode");
    }
    if debug_summary_direction_count(summary, "setup") == 0
        && debug_summary_direction_count(summary, "control-in") == 0
    {
        lines.push("USB setup/control-IN traffic for enumeration analysis");
    }
    if debug_summary_direction_count(summary, "in") == 0
        && debug_summary_direction_count(summary, "out") == 0
    {
        lines.push("endpoint IN/OUT traffic for runtime adapter analysis");
    }
    if debug_persona_captures == 0 {
        lines.push("current per-Pico state proving persona=debug");
    }
    if summary.aggregate.packet_lines == 0
        && summary.aggregate.harvest_lines == 0
        && retained_logs.is_empty()
    {
        lines.push("retained host harvest logs proving stream-time capture ran");
    }
    if captures.is_empty() && retained_logs.is_empty() {
        lines.push("live, cached, or retained Pico evidence");
    }
    if lines.is_empty() {
        lines.push("none");
    }
    lines
}

fn debug_summary_direction_count(summary: &UsbPacketBundleSummary, direction: &str) -> u64 {
    summary
        .aggregate
        .directions
        .get(direction)
        .copied()
        .unwrap_or(0)
}

fn debug_capture_overall_status(
    summary: &UsbPacketBundleSummary,
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
) -> &'static str {
    if summary.aggregate.packet_lines > 0 {
        "raw_packets_captured"
    } else if summary.aggregate.stats_lines > 0 {
        "debug_stats_only"
    } else if summary.aggregate.harvest_lines > 0 {
        "harvest_attempted_no_packets"
    } else if !retained_logs.is_empty() {
        "retained_logs_without_packet_lines"
    } else if captures.iter().any(|capture| capture.manifest.live) {
        "live_picos_no_packet_evidence"
    } else if !captures.is_empty() {
        "only_offline_or_cached_picos"
    } else {
        "no_pico_or_packet_evidence"
    }
}

fn state_json_persona(state_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(state_json).ok()?;
    value
        .get("persona")?
        .as_str()
        .map(|value| value.to_string())
}

fn format_count_map(map: &BTreeMap<String, u64>) -> String {
    if map.is_empty() {
        return "none".to_string();
    }
    map.iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn is_usb_packet_diagnostic_line(line: &str) -> bool {
    line.starts_with("usb-packet ")
        || line.starts_with("usb-packet-stats ")
        || line.starts_with("# harvest ")
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
            "  usb-packets.txt: captured {} raw USB packet(s)",
            total_packet_count
        );
    } else {
        println!("  usb-packets.txt: no raw packets captured (debug input mode only)");
    }
    println!(
        "  debug-capture-verdict.txt: {}",
        summary.debug_capture_status
    );
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
        aggregate_usb_packets, count_usb_packet_harvest_lines, count_usb_packet_lines,
        count_usb_packet_stats_lines, debug_capture_evidence_report_json,
        debug_capture_overall_status, debug_capture_verdict_text, sanitize_path_component,
        usb_packets_text_from_debug_snapshot, usb_packets_text_from_diag, PicoBundleCapture,
        RetainedDebugPacketLog,
    };
    use super::{summarize_sources, ManifestPicoCapture, UsbPacketSummarySource};

    #[test]
    fn pico_bundle_path_component_is_sanitized() {
        assert_eq!(sanitize_path_component("02E22DA9"), "02E22DA9");
        assert_eq!(sanitize_path_component("../02:E2\\2D/A9"), "02E22DA9");
        assert_eq!(sanitize_path_component(""), "unknown");
    }

    #[test]
    fn extracts_usb_packet_lines_from_diag_log() {
        let diag = "[      10] boot\n[      11] usb-packet seq=0 dir=out len=3 data=010203\n[      12] usb-packet-stats total=64 in=10 out=54\n";
        let out = usb_packets_text_from_diag("02E22DA9", diag);
        assert!(out.contains("usb-packet seq=0 dir=out len=3 data=010203"));
        assert!(out.contains("usb-packet-stats total=64 in=10 out=54"));
        assert_eq!(count_usb_packet_lines(&out), 1);
        assert_eq!(count_usb_packet_stats_lines(&out), 1);
        assert_eq!(count_usb_packet_harvest_lines(&out), 0);
    }

    #[test]
    fn bundle_debug_snapshot_includes_harvest_health() {
        let snapshot = crate::debug_packets::DiagLogSnapshot {
            text: "usb-packet seq=1 dir=out data=010203\nusb-packet-stats total=1 out=1\n"
                .to_string(),
            lost_bytes: 7,
            chunk_count: 2,
            expected_chunks: Some(3),
            missing_chunks: vec![1],
            duplicate_chunk_count: 1,
            got_last: true,
            byte_count: 72,
            line_count: 2,
        };
        let out = usb_packets_text_from_debug_snapshot("02E22DA9", &snapshot, 25);
        assert!(out.contains("usb-packet seq=1 dir=out data=010203"));
        assert!(out.contains("usb-packet-stats total=1 out=1"));
        assert!(out.contains("# harvest {"));
        assert!(out.contains("\"duration_ms\":25"));
        assert!(out.contains("\"missing_chunk_count\":1"));
        assert!(out.contains("\"duplicate_chunk_count\":1"));
        assert!(out.contains("\"chunk_complete\":false"));
        assert!(out.contains("\"lost_bytes\":7"));
        assert!(out.contains("\"raw_packet_lines\":1"));
        assert!(out.contains("\"stats_lines\":1"));
        assert_eq!(count_usb_packet_lines(&out), 1);
        assert_eq!(count_usb_packet_stats_lines(&out), 1);
        assert_eq!(count_usb_packet_harvest_lines(&out), 1);
    }

    #[test]
    fn harvest_error_text_counts_as_harvest_only_evidence() {
        let mut text = usb_packets_text_from_diag("02E22DA9", "no packet lines\n");
        text.push_str(&crate::debug_packets::harvest_error_line(1200, "timeout"));
        text.push('\n');
        assert_eq!(count_usb_packet_lines(&text), 0);
        assert_eq!(count_usb_packet_stats_lines(&text), 0);
        assert_eq!(count_usb_packet_harvest_lines(&text), 1);
    }

    #[test]
    fn aggregate_usb_packets_includes_retained_host_logs() {
        let retained = vec![RetainedDebugPacketLog {
            name: "usb-packets-20260615-214000-02E22DA9.log".to_string(),
            text: "# header\nusb-packet seq=4 dir=out data=010203\nusb-packet-stats total=64 in=10 out=54\n# harvest {\"status\":\"ok\",\"duration_ms\":14,\"packet_lines\":2}\n".to_string(),
        }];
        let out = aggregate_usb_packets(&[], &retained);
        assert!(out.contains("debug-packets/usb-packets-20260615-214000-02E22DA9.log"));
        assert!(out.contains("usb-packet seq=4 dir=out data=010203"));
        assert!(out.contains("usb-packet-stats total=64 in=10 out=54"));
        assert!(out.contains("# harvest {\"status\":\"ok\",\"duration_ms\":14,\"packet_lines\":2}"));
        assert!(!out.contains("No raw USB packets"));
    }

    #[test]
    fn aggregate_usb_packets_explains_harvest_without_payloads() {
        let retained = vec![RetainedDebugPacketLog {
            name: "usb-packets-20260615-214000-02E22DA9.log".to_string(),
            text: "# harvest {\"status\":\"error\",\"duration_ms\":1200,\"missing_chunk_count\":2,\"duplicate_chunk_count\":1,\"diag_bytes\":64,\"chunk_complete\":false,\"error\":\"no log chunks received\"}\n"
                .to_string(),
        }];
        let out = aggregate_usb_packets(&[], &retained);
        assert!(out.contains("# Aggregate USB packet capture evidence"));
        assert!(out.contains("# harvest {\"status\":\"error\""));
        assert!(out.contains(
            "No raw USB packet payload lines were captured, but harvest records were present."
        ));
    }

    #[test]
    fn debug_capture_verdict_identifies_raw_packets() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-packet seq=1 dir=out data=010203\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        assert_eq!(
            debug_capture_overall_status(&summary, std::slice::from_ref(&capture), &[]),
            "raw_packets_captured"
        );

        let text = debug_capture_verdict_text(&[capture], &[], &summary);
        assert!(text.contains("overall_status=raw_packets_captured"));
        assert!(text.contains("evidence_grade=usable_raw_packets"));
        assert!(text.contains("adapter_reverse_engineering_gate=pass"));
        assert!(text.contains("endpoint_out_lines=1"));
        assert!(text.contains("debug_persona_captures=1"));
        assert!(text.contains("- USB setup/control-IN traffic for enumeration analysis"));
        assert!(text.contains("raw_packet_lines=1"));
        assert!(text.contains("persona=debug"));
        assert!(text.contains("path=picos/02E22DA9"));
    }

    #[test]
    fn debug_capture_verdict_marks_complete_adapter_evidence() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-packet seq=1 dir=setup bm=0x80 req=0x06 value=0x0100 index=0x0000 wlen=18 data=8006000100001200\nusb-packet seq=2 dir=out data=010203\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        let text = debug_capture_verdict_text(&[capture], &[], &summary);
        assert!(text.contains("evidence_grade=complete"));
        assert!(text.contains("adapter_reverse_engineering_gate=pass"));
        assert!(text.contains("setup_lines=1"));
        assert!(text.contains("endpoint_out_lines=1"));
        assert!(text.contains("- none"));
    }

    #[test]
    fn debug_capture_verdict_includes_hid_report_metadata() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-packet seq=1 dir=out src=hid-output report_id=0x01 report_type=2 data=050607\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        let text = debug_capture_verdict_text(&[capture], &[], &summary);
        assert!(text.contains("hid_report_lines=1"));
        assert!(text.contains("hid_report_types=output:1"));
        assert!(text.contains("hid_report_ids=0x01:1"));
        assert!(text.contains("usb-hid-reports.txt"));
    }

    #[test]
    fn debug_capture_verdict_includes_packet_timing() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-packet seq=1 t=10 dir=out src=hid-output data=050607\nusb-packet seq=2 t=35 dir=in src=xinput data=00\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        let text = debug_capture_verdict_text(&[capture], &[], &summary);
        assert!(text.contains("packet_time_span_ms=25"));
        assert!(text.contains("max_inter_packet_gap_ms=25"));
        assert!(text.contains("packet_time_regressions=0"));
        assert!(text.contains("usb-packet-timeline.txt"));
    }

    #[test]
    fn debug_capture_evidence_json_reports_pass_gate() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-packet seq=1 t=10 dir=setup bm=0x80 req=0x06 value=0x0100 index=0x0000 wlen=18 data=8006000100001200\nusb-packet seq=2 t=35 dir=out src=hid-output report_id=0x01 report_type=2 data=050607\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        let json = debug_capture_evidence_report_json(&[capture], &[], &summary).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["artifact_schema_version"], 1);
        assert_eq!(value["adapter_reverse_engineering_gate"], "pass");
        assert_eq!(value["evidence_grade"], "complete");
        assert_eq!(value["aggregate"]["packet_lines"], 2);
        assert_eq!(value["aggregate"]["hid_report_lines"], 1);
        assert_eq!(value["aggregate"]["max_inter_packet_gap_ms"], 25);
        assert_eq!(value["per_pico"][0]["uid"], "02E22DA9");
        assert_eq!(value["per_pico"][0]["persona"], "debug");
        assert_eq!(value["per_pico"][0]["missing_evidence"][0], "none");
    }

    #[test]
    fn debug_capture_evidence_json_reports_missing_payloads() {
        let retained = vec![RetainedDebugPacketLog {
            name: "usb-packets-20260615-214000-02E22DA9.log".to_string(),
            text: "# harvest {\"status\":\"error\",\"duration_ms\":1200,\"chunk_complete\":false,\"error\":\"no log chunks received\"}\n"
                .to_string(),
        }];
        let retained_sources = [UsbPacketSummarySource {
            label: retained[0].name.clone(),
            path: format!("debug-packets/{}", retained[0].name),
            text: &retained[0].text,
        }];
        let summary = summarize_sources(&[], &retained_sources);
        let json = debug_capture_evidence_report_json(&[], &retained, &summary).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["adapter_reverse_engineering_gate"], "fail");
        assert_eq!(value["overall_status"], "harvest_attempted_no_packets");
        assert_eq!(value["aggregate"]["harvest_lines"], 1);
        assert!(value["missing_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "raw USB packet payload lines from debug input mode"));
        assert_eq!(
            value["retained_logs"][0]["path"],
            "debug-packets/usb-packets-20260615-214000-02E22DA9.log"
        );
        assert!(value["retained_logs"][0]["missing_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "raw USB packet payload lines from this source"));
    }

    #[test]
    fn debug_capture_verdict_identifies_harvest_without_packets() {
        let retained = vec![RetainedDebugPacketLog {
            name: "usb-packets-20260615-214000-02E22DA9.log".to_string(),
            text: "# harvest {\"status\":\"error\",\"duration_ms\":1200,\"missing_chunk_count\":2,\"duplicate_chunk_count\":1,\"diag_bytes\":64,\"chunk_complete\":false,\"error\":\"no log chunks received\"}\n"
                .to_string(),
        }];
        let retained_sources = [UsbPacketSummarySource {
            label: retained[0].name.clone(),
            path: format!("debug-packets/{}", retained[0].name),
            text: &retained[0].text,
        }];
        let summary = summarize_sources(&[], &retained_sources);

        assert_eq!(
            debug_capture_overall_status(&summary, &[], &retained),
            "harvest_attempted_no_packets"
        );
        let text = debug_capture_verdict_text(&[], &retained, &summary);
        assert!(text.contains("overall_status=harvest_attempted_no_packets"));
        assert!(text.contains("evidence_grade=partial_no_payloads"));
        assert!(text.contains("adapter_reverse_engineering_gate=fail"));
        assert!(text.contains("gate_reason=raw debug input packet payload lines are missing"));
        assert!(text.contains("harvest_chunk_statuses=incomplete:1"));
        assert!(text.contains("max_harvest_missing_chunks=2"));
        assert!(text.contains("max_harvest_duplicate_chunks=1"));
        assert!(text.contains("max_harvest_diag_bytes=64"));
        assert!(text.contains("- raw USB packet payload lines from debug input mode"));
        assert!(text.contains("harvest_statuses=error:1"));
        assert!(text.contains("GET_LOG failures"));
        assert!(text.contains("debug-packets/usb-packets-20260615-214000-02E22DA9.log"));
    }

    fn pico_capture(
        uid: &str,
        live: bool,
        state_json: &str,
        usb_packets_text: &str,
    ) -> PicoBundleCapture {
        PicoBundleCapture {
            manifest: ManifestPicoCapture {
                uid: uid.to_string(),
                path: format!("picos/{uid}"),
                peer: live.then(|| "10.0.0.24:4242".to_string()),
                live,
                source: "test".to_string(),
                state_captured: true,
                pico_diag_status: "captured".to_string(),
                usb_diag_status: "captured".to_string(),
                pico_state_status: "captured".to_string(),
                usb_packet_dump_status: if usb_packets_text.contains("usb-packet ") {
                    "captured"
                } else {
                    "no_packets"
                }
                .to_string(),
                usb_packet_dump_count: count_usb_packet_lines(usb_packets_text),
                cached_state_included: false,
            },
            state_json: state_json.to_string(),
            pico_diag_text: String::new(),
            usb_diag_text: String::new(),
            usb_packets_text: usb_packets_text.to_string(),
        }
    }
}
