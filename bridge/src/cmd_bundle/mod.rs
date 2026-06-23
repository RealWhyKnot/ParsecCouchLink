//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! crash files, Pico diag log, and a manifest.json with non-sensitive system
//! info. Intended to be attached to a bug report.
//!
//! NEVER include Wi-Fi credentials. The Pico stores them and the bridge
//! never reads them. SSID is also omitted by default to be safe.

mod adapter_survey;
mod bluetooth_report;
mod collect;
mod debug_capture;
mod host_snapshot;
mod manifest;
mod pico_diag;
mod redact;
mod sysinfo;
mod usb_enum;
mod usb_packet_summary;
mod usb_packets;
mod zip_writer;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Local;

use crate::{
    cmd_auto, cmd_persona, cmd_run, cmd_usb_diag, config, debug_packets, journal, pico_cache,
    pico_mode, pico_state, protocol,
};

#[cfg(test)]
use adapter_survey::AdapterSurveyAttempt;
use adapter_survey::{
    adapter_connection_json, adapter_connection_report, adapter_connection_text,
    adapter_survey_bundle_json, adapter_survey_candidates, adapter_survey_report_json,
    adapter_survey_text, aggregate_adapter_survey_text, build_adapter_survey_report,
    diag_has_usb_host_traffic, survey_attempt_from_diag, survey_diag_accepted,
    AdapterSurveyRawCapture, AdapterSurveyReport,
};
use bluetooth_report::{
    aggregate_bluetooth_report_text, bluetooth_report_bundle_json, bluetooth_usb_packets_stub,
    build_bluetooth_report, format_bluetooth_report_json, format_bluetooth_report_text,
    BluetoothReport,
};
use collect::{collect_crash_file_names, collect_setup_transcript_names};
use debug_capture::{
    debug_capture_evidence_report_json, debug_capture_overall_status, debug_capture_verdict_text,
};
use host_snapshot::capture_host_snapshots;
use manifest::{build_manifest, ManifestHostSnapshot, ManifestPicoCapture};
use pico_diag::{capture_pico_diag, DiagOutcome};
use sysinfo::{build_system_info, run_doctor_silently};
use usb_enum::{
    capture_usb_devices, capture_windows_usb_events, classify_pico_enum, PicoEnumState,
};
use usb_packet_summary::{
    control_transfers_text_for_sources, enumeration_analysis_text_for_sources,
    hid_reports_text_for_sources, packet_timeline_text_for_sources, records_jsonl_for_sources,
    summarize_sources, UsbPacketSummarySource,
};
use usb_packets::{
    aggregate_initial_usb_capture_text, count_retained_debug_packet_lines,
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, duration_ms_u64, usb_packets_text_from_debug_snapshot,
    usb_packets_text_from_diag,
};
use zip_writer::{write_bundle_zip, BundleZipContents};

const BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT: Duration = Duration::from_secs(2);
const BUNDLE_PERSONA_WAIT: Duration = Duration::from_secs(60);
const BUNDLE_RESTORE_PERSONA_WAIT: Duration = Duration::from_secs(60);

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
    pub adapter_connection_status: String,
    pub adapter_connection_warning: bool,
    pub per_pico_capture_count: usize,
    pub bluetooth_report_count: usize,
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
    initial_usb_capture_text: String,
    usb_packets_text: String,
    adapter_survey_text: String,
    adapter_survey_json: String,
    adapter_survey_report: Option<AdapterSurveyReport>,
    bluetooth_report_text: String,
    bluetooth_report_json: String,
    bluetooth_report: Option<BluetoothReport>,
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

#[derive(Clone, Debug)]
struct BundleUsbPacketCapture {
    text: String,
    capture_target: Option<cmd_run::PicoTarget>,
    adapter_survey_text: String,
    adapter_survey_json: String,
    adapter_survey_report: Option<AdapterSurveyReport>,
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
    let adapter_connection_report = adapter_connection_report(&per_pico_captures);
    let adapter_connection_text = adapter_connection_text(&adapter_connection_report);
    let adapter_connection_json = adapter_connection_json(&adapter_connection_report)?;
    let initial_usb_capture_text = aggregate_initial_usb_capture_text(&per_pico_captures);
    let adapter_survey_text = aggregate_adapter_survey_text(&per_pico_captures);
    let adapter_survey_json = adapter_survey_bundle_json(&per_pico_captures)?;
    let bluetooth_report_text = aggregate_bluetooth_report_text(&per_pico_captures);
    let bluetooth_report_json = bluetooth_report_bundle_json(&per_pico_captures)?;
    let bluetooth_report_count = per_pico_captures
        .iter()
        .filter(|capture| capture.bluetooth_report.is_some())
        .count();
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
    let usb_enumeration_analysis_text =
        enumeration_analysis_text_for_sources(&per_pico_packet_sources, &retained_packet_sources);
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
        "adapter_survey",
        0,
        "included",
        adapter_survey_text
            .len()
            .saturating_add(adapter_survey_json.len()),
        "txt_and_json",
    );
    capture_log.record_duration(
        "adapter_connection",
        0,
        adapter_connection_report.status,
        adapter_connection_text
            .len()
            .saturating_add(adapter_connection_json.len()),
        "txt_and_json",
    );
    capture_log.record_duration(
        "bluetooth_report",
        0,
        "included",
        bluetooth_report_text
            .len()
            .saturating_add(bluetooth_report_json.len()),
        "txt_and_json",
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
        "usb_enumeration_analysis",
        0,
        "included",
        usb_enumeration_analysis_text.len(),
        "enumeration_phase_checklist",
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

    write_bundle_zip(BundleZipContents {
        out_path: &out_path,
        manifest_json: &manifest_json,
        capture_log: &capture_log,
        doctor_text: &doctor_text,
        diag: &diag,
        pico_enum_state: &pico_enum_state,
        usb_diag: &usb_diag,
        adapter_connection_text: &adapter_connection_text,
        adapter_connection_json: &adapter_connection_json,
        initial_usb_capture_text: &initial_usb_capture_text,
        adapter_survey_text: &adapter_survey_text,
        adapter_survey_json: &adapter_survey_json,
        bluetooth_report_text: &bluetooth_report_text,
        bluetooth_report_json: &bluetooth_report_json,
        per_pico_captures: &per_pico_captures,
        retained_debug_packet_logs: &retained_debug_packet_logs,
        usb_packet_summary_json: &usb_packet_summary_json,
        usb_packet_records_jsonl: &usb_packet_records_jsonl,
        usb_control_transfers_text: &usb_control_transfers_text,
        usb_hid_reports_text: &usb_hid_reports_text,
        usb_packet_timeline_text: &usb_packet_timeline_text,
        usb_enumeration_analysis_text: &usb_enumeration_analysis_text,
        debug_capture_verdict: &debug_capture_verdict,
        debug_capture_evidence_json: &debug_capture_evidence_json,
        cache_current: &cache_current,
        cache_history: &cache_history,
        host_snapshots: &host_snapshots,
        system_info: &system_info,
        usb_devices: &usb_devices,
        usb_events: &usb_events,
    })?;
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
        adapter_connection_status: adapter_connection_report.status.to_string(),
        adapter_connection_warning: adapter_connection_report.warning,
        per_pico_capture_count: per_pico_captures.len(),
        bluetooth_report_count,
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
                     - If the Pico is on Wi-Fi but broadcast discovery is blocked, choose `Enter Pico IP manually` from the guided menu once, then run bundle again.\n\n\
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
    let initial_usb_capture_text: String;
    let usb_packets_text: String;
    let adapter_survey_text: String;
    let adapter_survey_json: String;
    let adapter_survey_report: Option<AdapterSurveyReport>;
    let bluetooth_report_text: String;
    let bluetooth_report_json: String;
    let bluetooth_report: Option<BluetoothReport>;

    let state_json = if let Some(target) = seed.target.as_ref() {
        state_captured = true;
        let mut snapshot = pico_cache::PicoStateSnapshot::from_target("bundle", target);
        let target_pico_state_status;
        let target_usb_diag_status;
        let mut target_pico_state_data: Option<protocol::PicoStateDiag> = None;
        let mut target_usb_diag_data: Option<protocol::UsbDiag> = None;

        let started = Instant::now();
        match pico_state::query_pico_state(target, Duration::from_millis(900)).await {
            Ok(state) => {
                target_pico_state_status = "captured".to_string();
                snapshot = snapshot.with_pico_state(&state);
                target_pico_state_data = Some(state);
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
                    target_usb_diag_data = Some(diag);
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
        initial_usb_capture_text = usb_packets_text_from_diag(&seed.uid, &pico_diag_text);
        if target.persona.is_bluetooth() {
            let report = build_bluetooth_report(
                &seed.uid,
                &path,
                target,
                target_pico_state_data.as_ref(),
                target_usb_diag_data.as_ref(),
                &pico_diag_text,
            );
            let text = format_bluetooth_report_text(&report);
            let json = format_bluetooth_report_json(&report);
            capture_log.record_duration(
                format!("per_pico.{}.bluetooth_report", seed.uid),
                0,
                report.status,
                text.len().saturating_add(json.len()),
                "txt_and_json",
            );
            usb_packets_text = bluetooth_usb_packets_stub(&seed.uid, target);
            adapter_survey_text = String::new();
            adapter_survey_json = String::new();
            adapter_survey_report = None;
            bluetooth_report_text = text;
            bluetooth_report_json = json;
            bluetooth_report = Some(report);
        } else {
            let packet_capture = bundle_usb_packets_for_target(
                &seed.uid,
                target,
                &pico_diag_text,
                target_usb_diag_data.as_ref(),
                capture_log,
            )
            .await;
            if let Some(capture_target) = packet_capture.capture_target.as_ref() {
                pico_cache::record(
                    pico_cache::PicoStateSnapshot::from_target(
                        "bundle-usb-capture",
                        capture_target,
                    )
                    .with_outcome("bundle: persona USB capture"),
                );
            }
            usb_packets_text = packet_capture.text;
            adapter_survey_text = packet_capture.adapter_survey_text;
            adapter_survey_json = packet_capture.adapter_survey_json;
            adapter_survey_report = packet_capture.adapter_survey_report;
            bluetooth_report_text = String::new();
            bluetooth_report_json = String::new();
            bluetooth_report = None;
        }
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
        initial_usb_capture_text = offline_pico_text(&seed, "initial USB packet dump");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        adapter_survey_text = String::new();
        adapter_survey_json = String::new();
        adapter_survey_report = None;
        bluetooth_report_text = String::new();
        bluetooth_report_json = String::new();
        bluetooth_report = None;
        state_json_from_snapshot(&snapshot)
    } else if let Some(cached) = seed.cached_state_json.as_ref() {
        state_captured = true;
        pico_diag_status = "cache_only_not_attempted".to_string();
        usb_diag_status = "cache_only_not_attempted".to_string();
        pico_state_status = "cache_only".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        initial_usb_capture_text = offline_pico_text(&seed, "initial USB packet dump");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        adapter_survey_text = String::new();
        adapter_survey_json = String::new();
        adapter_survey_report = None;
        bluetooth_report_text = String::new();
        bluetooth_report_json = String::new();
        bluetooth_report = None;
        cached.clone()
    } else {
        state_captured = false;
        pico_diag_status = "no_state".to_string();
        usb_diag_status = "no_state".to_string();
        pico_state_status = "no_state".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        initial_usb_capture_text = offline_pico_text(&seed, "initial USB packet dump");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        adapter_survey_text = String::new();
        adapter_survey_json = String::new();
        adapter_survey_report = None;
        bluetooth_report_text = String::new();
        bluetooth_report_json = String::new();
        bluetooth_report = None;
        "{}\n".to_string()
    };
    let usb_packet_count = count_usb_packet_lines(&usb_packets_text);
    let usb_packet_stats_count = count_usb_packet_stats_lines(&usb_packets_text);
    let usb_packet_event_count = count_usb_packet_event_lines(&usb_packets_text);
    let usb_packet_harvest_count = count_usb_packet_harvest_lines(&usb_packets_text);
    let usb_packet_status = if usb_packet_count > 0 {
        "captured"
    } else if usb_packet_stats_count > 0 {
        "stats_only"
    } else if usb_packet_event_count > 0 {
        "lifecycle_only"
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
            "count={usb_packet_count}; stats={usb_packet_stats_count}; events={usb_packet_event_count}; harvest={usb_packet_harvest_count}"
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
            bluetooth_report_status: bluetooth_report
                .as_ref()
                .map(|report| report.status.to_string())
                .unwrap_or_else(|| {
                    if seed
                        .target
                        .as_ref()
                        .map(|target| target.persona.is_bluetooth())
                        .unwrap_or(false)
                    {
                        "not_captured".to_string()
                    } else {
                        "not_applicable".to_string()
                    }
                }),
            cached_state_included: seed.cached_state_json.is_some(),
        },
        state_json,
        pico_diag_text,
        usb_diag_text,
        initial_usb_capture_text,
        usb_packets_text,
        adapter_survey_text,
        adapter_survey_json,
        adapter_survey_report,
        bluetooth_report_text,
        bluetooth_report_json,
        bluetooth_report,
    }
}

async fn bundle_usb_packets_for_target(
    uid: &str,
    target: &cmd_run::PicoTarget,
    fallback_diag_text: &str,
    current_diag: Option<&protocol::UsbDiag>,
    capture_log: &mut CaptureLog,
) -> BundleUsbPacketCapture {
    let original_persona = target.persona;
    let mut current = target.clone();
    let mut attempts = Vec::new();
    let mut capture_sections = Vec::new();
    let mut capture_target = None;
    let mut current_raw_capture = AdapterSurveyRawCapture::not_attempted("not_needed");
    let current_needs_capture = current_diag
        .map(|diag| diag.device_desc_count > 0 && !survey_diag_accepted(target.persona, diag))
        .unwrap_or(false);
    if current_needs_capture {
        let captured = if target.persona == protocol::Persona::Debug {
            let text =
                harvest_usb_packets_for_target(uid, target, fallback_diag_text, capture_log).await;
            PersonaPacketCapture {
                raw_capture: AdapterSurveyRawCapture {
                    attempted: true,
                    status: "captured".to_string(),
                    raw_packet_lines: count_usb_packet_lines(&text),
                    packet_stats_lines: count_usb_packet_stats_lines(&text),
                    usb_event_lines: count_usb_packet_event_lines(&text),
                    harvest_lines: count_usb_packet_harvest_lines(&text),
                },
                text,
                capture_target: Some(target.clone()),
            }
        } else {
            capture_persona_usb_packets(
                uid,
                target,
                target.persona,
                fallback_diag_text,
                capture_log,
            )
            .await
        };
        current_raw_capture = captured.raw_capture;
        if let Some(target) = captured.capture_target {
            current = target.clone();
            capture_target = Some(target);
        }
        if !captured.text.is_empty() {
            capture_sections.push(captured.text);
        }
    }

    let current_attempt = survey_attempt_from_diag(
        target.persona,
        true,
        false,
        current_diag.cloned(),
        current_raw_capture,
    );
    let current_has_no_usb_host = current_diag
        .map(|diag| !diag_has_usb_host_traffic(diag))
        .unwrap_or(false);
    let current_accepted = current_attempt.accepted;
    attempts.push(current_attempt);

    if current_has_no_usb_host {
        capture_log.record_duration(
            format!("per_pico.{uid}.adapter_survey.current"),
            0,
            "no_usb_host_traffic",
            0,
            "current USB diagnostic had no descriptor, mount, suspend, report, or OUT traffic",
        );
    }

    let candidates = adapter_survey_candidates(target.persona, current_accepted);
    for candidate in candidates {
        let switched = current.persona != candidate;
        let Some(active) =
            switch_to_survey_persona(uid, current.clone(), candidate, capture_log).await
        else {
            attempts.push(survey_attempt_from_diag(
                candidate,
                false,
                switched,
                None,
                AdapterSurveyRawCapture::not_attempted("switch_failed"),
            ));
            continue;
        };
        current = active.clone();

        capture_log.record_duration(
            format!(
                "per_pico.{uid}.adapter_survey.{}.usb_settle",
                candidate.label()
            ),
            cmd_auto::USB_SETTLE.as_millis() as u64,
            "sleep",
            0,
            "allow adapter USB host detection",
        );
        tokio::time::sleep(cmd_auto::USB_SETTLE).await;

        let diag = query_survey_usb_diag(uid, &active, candidate, capture_log).await;
        let needs_capture = diag
            .as_ref()
            .map(|diag| diag.device_desc_count > 0 && !survey_diag_accepted(candidate, diag))
            .unwrap_or(false);
        let mut raw_capture = AdapterSurveyRawCapture::not_attempted("not_needed");
        if needs_capture {
            let captured = capture_persona_usb_packets(
                uid,
                &active,
                candidate,
                fallback_diag_text,
                capture_log,
            )
            .await;
            raw_capture = captured.raw_capture;
            if let Some(target) = captured.capture_target {
                current = target.clone();
                capture_target = Some(target);
            }
            if !captured.text.is_empty() {
                capture_sections.push(captured.text);
            }
        }

        let attempt = survey_attempt_from_diag(candidate, false, switched, diag, raw_capture);
        let accepted = attempt.accepted;
        attempts.push(attempt);
        if accepted {
            capture_log.record_duration(
                format!("per_pico.{uid}.adapter_survey.stop"),
                0,
                "accepted",
                0,
                format!("persona={}", candidate.label()),
            );
            break;
        }
    }

    let restore_status =
        restore_persona_after_bundle(uid, &current, original_persona, capture_log).await;
    let restored_persona = if restore_status == "confirmed" || restore_status == "already_current" {
        Some(original_persona.label().to_string())
    } else {
        None
    };
    let report = build_adapter_survey_report(
        uid.to_string(),
        original_persona.label().to_string(),
        restore_status,
        restored_persona,
        attempts,
        vec![
            "PS3 is tested first for USB-to-Maple adapters, followed by a generic HID gamepad fallback.",
            "Debug mode uses the XInput USB shape and is not selected as adapter proof.",
            "Polling or configured means the adapter accepted that persona.",
            "device_desc_count=0 means the adapter did not enumerate that persona.",
            "Descriptor traffic without configuration points to descriptor or report rejection.",
        ],
    );

    let adapter_survey_text = adapter_survey_text(&report);
    let adapter_survey_json = adapter_survey_report_json(&report);
    let mut text = usb_packets_text_from_diag(uid, fallback_diag_text);
    for section in capture_sections {
        text.push('\n');
        text.push_str(&section);
    }
    BundleUsbPacketCapture {
        text,
        capture_target,
        adapter_survey_text,
        adapter_survey_json,
        adapter_survey_report: Some(report),
    }
}

async fn switch_to_survey_persona(
    uid: &str,
    current: cmd_run::PicoTarget,
    candidate: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> Option<cmd_run::PicoTarget> {
    if current.persona == candidate {
        return Some(current);
    }

    let started = Instant::now();
    match pico_mode::request_set_persona(&current, candidate).await {
        Ok(()) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.switch_request",
                candidate.label()
            ),
            started,
            "sent",
            1,
            format!("from={} to={}", current.persona.label(), candidate.label()),
        ),
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.switch_request",
                    candidate.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return None;
        }
    }

    let started = Instant::now();
    let matched = match cmd_persona::wait_for_persona(
        &[current.info.unique_id_short],
        candidate,
        BUNDLE_PERSONA_WAIT,
    )
    .await
    {
        Ok(matched) => matched,
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.switch_wait",
                    candidate.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return None;
        }
    };
    let found = matched
        .iter()
        .find(|pico| pico.info.unique_id_short == current.info.unique_id_short)
        .cloned();
    capture_log.record(
        format!(
            "per_pico.{uid}.adapter_survey.{}.switch_wait",
            candidate.label()
        ),
        started,
        if found
            .as_ref()
            .map(|pico| pico.persona == candidate)
            .unwrap_or(false)
        {
            "confirmed"
        } else {
            "not_confirmed"
        },
        matched.len(),
        format!("observed={}", format_observed_personas(&matched)),
    );
    match found {
        Some(pico) if pico.persona == candidate => Some(pico),
        _ => None,
    }
}

async fn query_survey_usb_diag(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> Option<protocol::UsbDiag> {
    let started = Instant::now();
    match cmd_usb_diag::query_usb_diag(target, cmd_auto::USB_PROBE).await {
        Ok(diag) => {
            capture_log.record(
                format!("per_pico.{uid}.adapter_survey.{}.usb_diag", persona.label()),
                started,
                "captured",
                protocol::USB_DIAG_WIRE_SIZE,
                format!(
                    "score={}; device_desc_count={}; config_desc_count={}",
                    cmd_auto::score_label(cmd_auto::score_usb_diag(&diag)),
                    diag.device_desc_count,
                    diag.config_desc_count
                ),
            );
            Some(diag)
        }
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.adapter_survey.{}.usb_diag", persona.label()),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            None
        }
    }
}

struct PersonaPacketCapture {
    text: String,
    raw_capture: AdapterSurveyRawCapture,
    capture_target: Option<cmd_run::PicoTarget>,
}

async fn capture_persona_usb_packets(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    fallback_diag_text: &str,
    capture_log: &mut CaptureLog,
) -> PersonaPacketCapture {
    let started = Instant::now();
    match pico_mode::request_set_usb_capture_persona(target, persona).await {
        Ok(()) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.capture_request",
                persona.label()
            ),
            started,
            "sent",
            1,
            "usb_capture=enabled",
        ),
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.capture_request",
                    persona.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return PersonaPacketCapture {
                text: String::new(),
                raw_capture: AdapterSurveyRawCapture::not_attempted("request_failed"),
                capture_target: None,
            };
        }
    }

    let started = Instant::now();
    let matched = match cmd_persona::wait_for_persona(
        &[target.info.unique_id_short],
        persona,
        BUNDLE_PERSONA_WAIT,
    )
    .await
    {
        Ok(matched) => matched,
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.capture_wait",
                    persona.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return PersonaPacketCapture {
                text: String::new(),
                raw_capture: AdapterSurveyRawCapture::not_attempted("wait_failed"),
                capture_target: None,
            };
        }
    };
    let found = matched
        .iter()
        .find(|pico| pico.info.unique_id_short == target.info.unique_id_short)
        .cloned();
    capture_log.record(
        format!(
            "per_pico.{uid}.adapter_survey.{}.capture_wait",
            persona.label()
        ),
        started,
        if found
            .as_ref()
            .map(|pico| pico.persona == persona)
            .unwrap_or(false)
        {
            "confirmed"
        } else {
            "not_confirmed"
        },
        matched.len(),
        format!("observed={}", format_observed_personas(&matched)),
    );
    let Some(capture_target) = found.filter(|pico| pico.persona == persona) else {
        return PersonaPacketCapture {
            text: String::new(),
            raw_capture: AdapterSurveyRawCapture::not_attempted("not_confirmed"),
            capture_target: None,
        };
    };

    capture_log.record_duration(
        format!(
            "per_pico.{uid}.adapter_survey.{}.capture_settle",
            persona.label()
        ),
        cmd_auto::USB_SETTLE.as_millis() as u64,
        "sleep",
        0,
        "allow capture-enabled persona to enumerate",
    );
    tokio::time::sleep(cmd_auto::USB_SETTLE).await;

    let mut text =
        harvest_usb_packets_for_target(uid, &capture_target, fallback_diag_text, capture_log).await;
    let _ = writeln!(
        text,
        "# adapter-survey-capture persona={} status=attempted",
        persona.label()
    );
    let raw_capture = AdapterSurveyRawCapture {
        attempted: true,
        status: "captured".to_string(),
        raw_packet_lines: count_usb_packet_lines(&text),
        packet_stats_lines: count_usb_packet_stats_lines(&text),
        usb_event_lines: count_usb_packet_event_lines(&text),
        harvest_lines: count_usb_packet_harvest_lines(&text),
    };

    let started = Instant::now();
    match pico_mode::request_clear_usb_capture(&capture_target).await {
        Ok(()) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.capture_clear",
                persona.label()
            ),
            started,
            "sent",
            1,
            "usb_capture=disabled",
        ),
        Err(e) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.capture_clear",
                persona.label()
            ),
            started,
            "error",
            0,
            format!("{e:#}"),
        ),
    }

    PersonaPacketCapture {
        text,
        raw_capture,
        capture_target: Some(capture_target),
    }
}

async fn harvest_usb_packets_for_target(
    uid: &str,
    target: &cmd_run::PicoTarget,
    fallback_diag_text: &str,
    capture_log: &mut CaptureLog,
) -> String {
    let started = Instant::now();
    match debug_packets::capture_run_diag_log(target.peer, BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT)
        .await
    {
        Ok(snapshot) => {
            let duration_ms = duration_ms_u64(started.elapsed());
            let text = usb_packets_text_from_debug_snapshot(uid, &snapshot, duration_ms);
            capture_log.record(
                format!("per_pico.{uid}.usb_packet_harvest"),
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
                format!("per_pico.{uid}.usb_packet_harvest"),
                started,
                "error",
                text.len(),
                format!("{e:#}"),
            );
            text
        }
    }
}

async fn restore_persona_after_bundle(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> String {
    if target.persona == persona {
        capture_log.record_duration(
            format!("per_pico.{uid}.restore_persona_request"),
            0,
            "already_current",
            0,
            format!("persona={}", persona.label()),
        );
        return "already_current".to_string();
    }

    let started = Instant::now();
    match pico_mode::request_set_persona(target, persona).await {
        Ok(()) => capture_log.record(
            format!("per_pico.{uid}.restore_persona_request"),
            started,
            "sent",
            1,
            format!("persona={}", persona.label()),
        ),
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.restore_persona_request"),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return "request_failed".to_string();
        }
    }

    let started = Instant::now();
    match cmd_persona::wait_for_persona(
        &[target.info.unique_id_short],
        persona,
        BUNDLE_RESTORE_PERSONA_WAIT,
    )
    .await
    {
        Ok(matched) => {
            let restored = matched.iter().find(|pico| pico.persona == persona);
            let status = if restored.is_some() {
                "confirmed"
            } else {
                "not_confirmed"
            };
            capture_log.record(
                format!("per_pico.{uid}.restore_persona_wait"),
                started,
                status,
                matched.len(),
                format!("observed={}", format_observed_personas(&matched)),
            );
            if let Some(restored) = restored {
                pico_cache::record(
                    pico_cache::PicoStateSnapshot::from_target("bundle-restore", restored)
                        .with_outcome(format!("restored_{}", persona.label())),
                );
            }
            status.to_string()
        }
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.restore_persona_wait"),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            "wait_failed".to_string()
        }
    }
}

fn format_observed_personas(targets: &[cmd_run::PicoTarget]) -> String {
    if targets.is_empty() {
        return "none".to_string();
    }
    targets
        .iter()
        .map(|target| format!("{}:{}", target.uid_hex(), target.persona.label()))
        .collect::<Vec<_>>()
        .join(",")
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
    println!(
        "  adapter-survey.txt: included for {} Pico capture(s)",
        summary.per_pico_capture_count
    );
    if summary.bluetooth_report_count > 0 {
        println!(
            "  bluetooth-report.txt: captured for {} Bluetooth Pico board(s)",
            summary.bluetooth_report_count
        );
    } else {
        println!("  bluetooth-report.txt: no Bluetooth-mode Pico captured");
    }
    if summary.adapter_connection_warning {
        println!();
        println!("Adapter connection warning:");
        println!(
            "  No USB host enumeration traffic was observed from a live Pico. This bundle will not contain the adapter diagnostics needed to prove console-adapter support."
        );
        println!(
            "  Plug the Pico into the console adapter and console USB host you want it to work on, then run couchlink bundle again."
        );
        println!(
            "  If the adapter only handshakes once, power-cycle or physically replug the console-side adapter path before running bundle."
        );
    } else {
        println!(
            "  adapter-connection.txt: {}",
            summary.adapter_connection_status
        );
    }
    let total_packet_count = summary.usb_packet_dump_count + summary.retained_debug_packet_count;
    if total_packet_count > 0 {
        println!(
            "  usb-packets.txt: captured {} raw USB packet(s)",
            total_packet_count
        );
    } else {
        println!("  usb-packets.txt: no raw packets captured");
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
    println!("Wi-Fi credentials, local addresses, and profile paths are redacted. Safe to share.");
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
    use super::usb_packets::aggregate_usb_packets;
    use super::{
        adapter_connection_json, adapter_connection_report, adapter_connection_text,
        adapter_survey_bundle_json, adapter_survey_candidates, adapter_survey_report_json,
        adapter_survey_text, aggregate_adapter_survey_text, aggregate_bluetooth_report_text,
        aggregate_initial_usb_capture_text, bluetooth_report_bundle_json,
        bluetooth_usb_packets_stub, build_adapter_survey_report, build_bluetooth_report,
        count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
        count_usb_packet_stats_lines, debug_capture_evidence_report_json,
        debug_capture_overall_status, debug_capture_verdict_text, format_bluetooth_report_json,
        format_bluetooth_report_text, sanitize_path_component,
        usb_packets_text_from_debug_snapshot, usb_packets_text_from_diag, AdapterSurveyAttempt,
        AdapterSurveyRawCapture, PicoBundleCapture, RetainedDebugPacketLog,
    };
    use super::{summarize_sources, ManifestPicoCapture, UsbPacketSummarySource};
    use crate::protocol;

    #[test]
    fn pico_bundle_path_component_is_sanitized() {
        assert_eq!(sanitize_path_component("02E22DA9"), "02E22DA9");
        assert_eq!(sanitize_path_component("../02:E2\\2D/A9"), "02E22DA9");
        assert_eq!(sanitize_path_component(""), "unknown");
    }

    #[test]
    fn extracts_usb_packet_lines_from_diag_log() {
        let diag = "[      10] boot\n[      11] usb-packet seq=0 dir=out len=3 data=010203\n[      12] usb-event t=22 event=mount\n[      13] usb-packet-stats total=64 in=10 out=54\n";
        let out = usb_packets_text_from_diag("02E22DA9", diag);
        assert!(out.contains("usb-packet seq=0 dir=out len=3 data=010203"));
        assert!(out.contains("usb-event t=22 event=mount"));
        assert!(out.contains("usb-packet-stats total=64 in=10 out=54"));
        assert_eq!(count_usb_packet_lines(&out), 1);
        assert_eq!(count_usb_packet_event_lines(&out), 1);
        assert_eq!(count_usb_packet_stats_lines(&out), 1);
        assert_eq!(count_usb_packet_harvest_lines(&out), 0);
    }

    #[test]
    fn bundle_debug_snapshot_includes_harvest_health() {
        let snapshot = crate::debug_packets::DiagLogSnapshot {
            text: "usb-packet seq=1 dir=out data=010203\nusb-event t=22 event=mount\nusb-packet-stats total=1 out=1\n"
                .to_string(),
            lost_bytes: 7,
            chunk_count: 2,
            expected_chunks: Some(3),
            missing_chunks: vec![1],
            duplicate_chunk_count: 1,
            got_last: true,
            byte_count: 72,
            line_count: 3,
        };
        let out = usb_packets_text_from_debug_snapshot("02E22DA9", &snapshot, 25);
        assert!(out.contains("usb-packet seq=1 dir=out data=010203"));
        assert!(out.contains("usb-event t=22 event=mount"));
        assert!(out.contains("usb-packet-stats total=1 out=1"));
        assert!(out.contains("# harvest {"));
        assert!(out.contains("\"duration_ms\":25"));
        assert!(out.contains("\"missing_chunk_count\":1"));
        assert!(out.contains("\"duplicate_chunk_count\":1"));
        assert!(out.contains("\"chunk_complete\":false"));
        assert!(out.contains("\"lost_bytes\":7"));
        assert!(out.contains("\"raw_packet_lines\":1"));
        assert!(out.contains("\"stats_lines\":1"));
        assert!(out.contains("\"event_lines\":1"));
        assert_eq!(count_usb_packet_lines(&out), 1);
        assert_eq!(count_usb_packet_event_lines(&out), 1);
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
            text: "# header\nusb-packet seq=4 dir=out data=010203\nusb-event t=22 event=mount\nusb-packet-stats total=64 in=10 out=54\n# harvest {\"status\":\"ok\",\"duration_ms\":14,\"packet_lines\":2}\n".to_string(),
        }];
        let out = aggregate_usb_packets(&[], &retained);
        assert!(out.contains("debug-packets/usb-packets-20260615-214000-02E22DA9.log"));
        assert!(out.contains("usb-packet seq=4 dir=out data=010203"));
        assert!(out.contains("usb-event t=22 event=mount"));
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
        assert!(text.contains("capture_quality=lossless_observed"));
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
        assert!(text.contains("capture_quality=lossless_observed"));
        assert!(text.contains("adapter_reverse_engineering_gate=pass"));
        assert!(text.contains("setup_lines=1"));
        assert!(text.contains("endpoint_out_lines=1"));
        assert!(text.contains("setup_requests=get_descriptor:1"));
        assert!(text.contains("setup_descriptor_requests=device:1"));
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
    fn debug_capture_verdict_identifies_lifecycle_without_payloads() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-event t=22 event=mount\nusb-event t=24 event=suspend remote_wakeup=1\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        assert_eq!(
            debug_capture_overall_status(&summary, std::slice::from_ref(&capture), &[]),
            "debug_lifecycle_only"
        );

        let text = debug_capture_verdict_text(&[capture], &[], &summary);
        assert!(text.contains("overall_status=debug_lifecycle_only"));
        assert!(text.contains("evidence_grade=partial_no_payloads"));
        assert!(text.contains("adapter_reverse_engineering_gate=fail"));
        assert!(text.contains("usb_event_lines=2"));
        assert!(text.contains("usb_events=mount:1,suspend:1"));
        assert!(text.contains("- raw USB packet payload lines from debug input mode"));
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
        assert_eq!(value["artifact_schema_version"], 3);
        assert_eq!(value["adapter_reverse_engineering_gate"], "pass");
        assert_eq!(value["evidence_grade"], "complete");
        assert_eq!(value["capture_quality"], "lossless_observed");
        assert_eq!(value["aggregate"]["packet_lines"], 2);
        assert_eq!(value["aggregate"]["hid_report_lines"], 1);
        assert_eq!(value["aggregate"]["max_inter_packet_gap_ms"], 25);
        assert_eq!(value["aggregate"]["setup_requests"]["get_descriptor"], 1);
        assert_eq!(value["aggregate"]["setup_descriptor_requests"]["device"], 1);
        assert_eq!(value["per_pico"][0]["uid"], "02E22DA9");
        assert_eq!(value["per_pico"][0]["persona"], "debug");
        assert_eq!(value["per_pico"][0]["missing_evidence"][0], "none");
    }

    #[test]
    fn debug_capture_evidence_marks_lossy_packet_capture() {
        let capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"debug\"}\n",
            "usb-packet seq=1 t=10 dir=out len=70 captured=64 truncated=6 dropped=6 reason=host-out data=000102\nusb-packet-stats t=11 total=1 out=1 truncated_bytes=6 truncated_packets=1 idle_in_suppressed=0\n# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":2,\"expected_chunks\":3,\"missing_chunk_count\":1,\"got_last\":true,\"chunk_complete\":false,\"lost_bytes\":8,\"diag_bytes\":512,\"diag_lines\":20,\"packet_lines\":1,\"raw_packet_lines\":1,\"stats_lines\":1,\"new_lines\":1,\"duplicate_lines\":0,\"total_packet_lines\":1}\n",
        );
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: &capture.usb_packets_text,
        }];
        let summary = summarize_sources(&per_pico, &[]);
        let text = debug_capture_verdict_text(std::slice::from_ref(&capture), &[], &summary);
        assert!(text.contains("capture_quality=lossy"));
        assert!(text.contains(
            "gate_reason=raw debug input packet payload lines are present, but capture is lossy"
        ));
        assert!(text.contains("truncated_packet_lines=1"));
        assert!(text.contains("max_packet_truncated_bytes=6"));
        assert!(text.contains("max_harvest_lost_bytes=8"));
        assert!(text.contains("- lossless packet payload and harvest capture"));

        let json = debug_capture_evidence_report_json(&[capture], &[], &summary).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["capture_quality"], "lossy");
        assert_eq!(value["aggregate"]["truncated_packet_lines"], 1);
        assert_eq!(value["aggregate"]["max_packet_truncated_bytes"], 6);
        assert_eq!(value["aggregate"]["max_harvest_lost_bytes"], 8);
        assert!(value["missing_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "lossless packet payload and harvest capture"));
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

    #[test]
    fn bluetooth_report_captures_usb_input_and_bt_send_state() {
        let target = bluetooth_target(protocol::Persona::BluetoothHid);
        let pico_state = bluetooth_pico_state(
            protocol::BT_HID_STATUS_STARTED | protocol::BT_HID_STATUS_CONNECTED,
            3,
        );
        let usb_diag = bluetooth_usb_diag();
        let report = build_bluetooth_report(
            "02E22DA9",
            "picos/02E22DA9",
            &target,
            Some(&pico_state),
            Some(&usb_diag),
            "run: Bluetooth persona = bluetooth\nbt_hid: connected\ncdc: dispatching cmd=0x0C seq=9 payload=13 bytes\n",
        );

        assert_eq!(report.status, "reports_sent");
        assert!(!report.warning);
        assert_eq!(report.persona, "bluetooth");
        assert_eq!(report.target_label, "bluetooth");
        assert!(report.bt_started);
        assert!(report.bt_connected);
        assert_eq!(report.bt_report_send_count, 3);
        assert_eq!(report.usb_mounted, Some(true));
        assert_eq!(report.usb_device_desc_count, Some(2));
        assert_eq!(report.relevant_diag_lines.len(), 3);

        let text = format_bluetooth_report_text(&report);
        assert!(text.contains("expected_connection=pc_usb_input_bluetooth_output"));
        assert!(text.contains("usb_transport=cdc_framed_controller_state"));
        assert!(text.contains("- bt_report_send_count=3"));
        assert!(text.contains("- device_desc_count=2"));

        let value: serde_json::Value =
            serde_json::from_str(&format_bluetooth_report_json(&report)).unwrap();
        assert_eq!(value["status"], "reports_sent");
        assert_eq!(value["usb_input_required"], true);
        assert_eq!(value["bt_connected"], true);
        assert_eq!(value["usb_transport"], "cdc_framed_controller_state");
    }

    #[test]
    fn bluetooth_report_statuses_are_actionable() {
        let target = bluetooth_target(protocol::Persona::BluetoothXbox);
        let missing = build_bluetooth_report("02E22DA9", "picos/02E22DA9", &target, None, None, "");
        assert_eq!(missing.status, "pico_state_missing");
        assert!(missing.warning);

        let mut xbox_state = bluetooth_pico_state(0, 0);
        xbox_state.bt_target = 1;
        let not_started = build_bluetooth_report(
            "02E22DA9",
            "picos/02E22DA9",
            &target,
            Some(&xbox_state),
            None,
            "run: Bluetooth persona = bluetooth-xbox\n",
        );
        assert_eq!(not_started.status, "bluetooth_stack_not_started");
        assert_eq!(not_started.target_label, "bluetooth-xbox");

        let waiting = build_bluetooth_report(
            "02E22DA9",
            "picos/02E22DA9",
            &target,
            Some(&bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0)),
            None,
            "bt_hid: init target=bluetooth-xbox\n",
        );
        assert_eq!(waiting.status, "waiting_for_receiver");
        assert!(waiting.next_steps.iter().any(|step| step.contains("pair")));

        let connected_no_reports = build_bluetooth_report(
            "02E22DA9",
            "picos/02E22DA9",
            &target,
            Some(&bluetooth_pico_state(
                protocol::BT_HID_STATUS_STARTED | protocol::BT_HID_STATUS_CONNECTED,
                0,
            )),
            None,
            "bt_hid: connected\n",
        );
        assert_eq!(connected_no_reports.status, "connected_waiting_for_input");
        assert!(connected_no_reports
            .next_steps
            .iter()
            .any(|step| step.contains("source controller")));
    }

    #[test]
    fn aggregate_bluetooth_report_only_includes_bluetooth_captures() {
        let target = bluetooth_target(protocol::Persona::BluetoothPlaystation);
        let mut ps_state = bluetooth_pico_state(
            protocol::BT_HID_STATUS_STARTED | protocol::BT_HID_STATUS_CONNECTED,
            1,
        );
        ps_state.bt_target = 2;
        let report = build_bluetooth_report(
            "02E22DA9",
            "picos/02E22DA9",
            &target,
            Some(&ps_state),
            Some(&bluetooth_usb_diag()),
            "bt_hid: connected\n",
        );
        let mut capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"bluetooth-playstation\"}\n",
            "",
        );
        capture.manifest.bluetooth_report_status = report.status.to_string();
        capture.bluetooth_report_text = format_bluetooth_report_text(&report);
        capture.bluetooth_report_json = format_bluetooth_report_json(&report);
        capture.bluetooth_report = Some(report);

        let text = aggregate_bluetooth_report_text(std::slice::from_ref(&capture));
        assert!(text.contains("path=picos/02E22DA9/bluetooth-report.txt"));
        assert!(text.contains("persona=bluetooth-playstation"));
        assert!(text.contains("bt_report_send_count=1"));

        let json = bluetooth_report_bundle_json(&[capture]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["report_count"], 1);
        assert_eq!(
            value["per_pico"][0]["target_label"],
            "bluetooth-playstation"
        );
    }

    #[test]
    fn bluetooth_usb_packets_stub_explains_no_adapter_survey() {
        let target = bluetooth_target(protocol::Persona::BluetoothHid);
        let text = bluetooth_usb_packets_stub("02E22DA9", &target);
        assert!(text.contains("streams controller input over USB CDC frames"));
        assert!(text.contains("No console USB adapter packet capture"));
        assert_eq!(count_usb_packet_lines(&text), 0);
    }

    #[test]
    fn adapter_survey_candidates_cycle_after_debug_no_host_traffic() {
        assert_eq!(
            adapter_survey_candidates(crate::protocol::Persona::Debug, false),
            vec![
                crate::protocol::Persona::Ps3,
                crate::protocol::Persona::GenericHid,
                crate::protocol::Persona::Ps4,
                crate::protocol::Persona::Keyboard,
                crate::protocol::Persona::Xinput,
                crate::protocol::Persona::XboxOne,
                crate::protocol::Persona::Maple,
            ]
        );
    }

    #[test]
    fn adapter_survey_candidates_stop_after_accepted_current_persona() {
        assert!(adapter_survey_candidates(crate::protocol::Persona::Ps4, true).is_empty());
    }

    #[test]
    fn adapter_survey_candidates_try_remaining_personas_after_rejected_current_persona() {
        assert_eq!(
            adapter_survey_candidates(crate::protocol::Persona::Ps4, false),
            vec![
                crate::protocol::Persona::Ps3,
                crate::protocol::Persona::GenericHid,
                crate::protocol::Persona::Keyboard,
                crate::protocol::Persona::Xinput,
                crate::protocol::Persona::XboxOne,
                crate::protocol::Persona::Maple,
            ]
        );
    }

    #[test]
    fn adapter_survey_text_selects_accepted_ps4_candidate() {
        let attempts = vec![
            current_survey_attempt("debug", false, "debug_xinput_evidence_only", 4, 1),
            survey_attempt("ps4", true, "accepted_by_adapter", 5, 2),
            survey_attempt("keyboard", false, "adapter_did_not_enumerate", 0, 0),
        ];
        let report = test_survey_report("02E22DA9", "debug", "confirmed", Some("debug"), attempts);

        let text = adapter_survey_text(&report);
        assert!(text.contains("selected_best=ps4 accepted=true"));
        assert!(text.contains(
            "expected_adapter_personas=ps3,generic-hid,ps4,keyboard,xinput,xboxone,maple"
        ));
        assert!(text.contains("attempted_personas=debug,ps4,keyboard"));
        assert!(text.contains("missing_adapter_personas=ps3,generic-hid,xinput,xboxone,maple"));
        assert!(text.contains("current_no_usb_host_traffic=false"));
        assert!(text.contains("coverage_status=stopped_after_acceptance"));
        assert!(text.contains("stop_reason=accepted_candidate"));
        assert!(text.contains("persona=keyboard"));
        assert!(text.contains("verdict=adapter_did_not_enumerate"));
        assert!(text.contains("debug_xinput_evidence_only"));

        let json = adapter_survey_report_json(&report);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["best_candidate"]["persona"], "ps4");
        assert_eq!(value["best_candidate"]["accepted"], true);
        assert_eq!(value["coverage_status"], "stopped_after_acceptance");
        assert_eq!(value["stop_reason"], "accepted_candidate");
        assert_eq!(value["missing_adapter_personas"][0], "ps3");
        assert_eq!(value["attempts"][2]["device_desc_count"], 0);
    }

    #[test]
    fn aggregate_adapter_survey_json_reports_bundle_best() {
        let attempts = vec![
            survey_attempt("ps4", false, "descriptor_or_report_rejected", 1, 1),
            survey_attempt("keyboard", true, "accepted_by_adapter", 4, 1),
        ];
        let report = test_survey_report(
            "02E22DA9",
            "xinput",
            "already_current",
            Some("xinput"),
            attempts,
        );
        let mut capture = pico_capture("02E22DA9", true, "{\"persona\":\"xinput\"}\n", "");
        capture.adapter_survey_text = adapter_survey_text(&report);
        capture.adapter_survey_json = adapter_survey_report_json(&report);
        capture.adapter_survey_report = Some(report);

        let text = aggregate_adapter_survey_text(std::slice::from_ref(&capture));
        assert!(text.contains("path=picos/02E22DA9/adapter-survey.txt"));
        assert!(text.contains("selected_best=keyboard accepted=true"));

        let json = adapter_survey_bundle_json(&[capture]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["survey_count"], 1);
        assert_eq!(value["best_candidate"]["persona"], "keyboard");
        assert_eq!(value["best_candidate"]["accepted"], true);
        assert_eq!(value["per_pico"][0]["original_persona"], "xinput");
    }

    #[test]
    fn adapter_connection_warns_when_live_survey_has_no_host_traffic() {
        let report = test_survey_report(
            "02E22DA9",
            "debug",
            "already_current",
            Some("debug"),
            all_no_host_survey_attempts(),
        );
        let mut capture = pico_capture("02E22DA9", true, "{\"persona\":\"debug\"}\n", "");
        capture.adapter_survey_report = Some(report);

        let connection = adapter_connection_report(&[capture]);
        assert_eq!(connection.status, "no_usb_host_traffic");
        assert!(connection.warning);
        assert_eq!(connection.surveyed_live_pico_count, 1);
        assert_eq!(connection.no_usb_host_pico_count, 1);
        assert_eq!(connection.host_traffic_pico_count, 0);
        assert_eq!(connection.per_pico[0].attempts, 8);
        assert_eq!(
            connection.per_pico[0].coverage_status,
            "all_adapter_personas_attempted"
        );
        assert_eq!(connection.per_pico[0].stop_reason, "exhausted_candidates");
        assert!(connection.per_pico[0].missing_adapter_personas.is_empty());
        assert!(connection.per_pico[0].warning);

        let text = adapter_connection_text(&connection);
        assert!(text.contains("warning_text=No USB host enumeration traffic was observed"));
        assert!(text.contains("coverage_status=all_adapter_personas_attempted"));
        assert!(text.contains(
            "attempted_personas=debug,ps3,generic-hid,ps4,keyboard,xinput,xboxone,maple"
        ));
        assert!(text.contains("missing_adapter_personas=none"));
        assert!(text.contains("If every attempted persona reports device_desc_count=0"));
        assert!(text.contains("power-cycle or physically replug"));

        let json = adapter_connection_json(&connection).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["status"], "no_usb_host_traffic");
        assert_eq!(value["warning"], true);
        assert_eq!(
            value["per_pico"][0]["coverage_status"],
            "all_adapter_personas_attempted"
        );
        assert_eq!(value["per_pico"][0]["attempted_personas"][7], "maple");
        assert_eq!(value["per_pico"][0]["device_desc_total"], 0);
    }

    #[test]
    fn adapter_connection_does_not_warn_for_offline_or_cache_only_capture() {
        let capture = pico_capture("02E22DA9", false, "{\"persona\":\"xinput\"}\n", "");
        let connection = adapter_connection_report(&[capture]);
        assert_eq!(connection.status, "not_checked");
        assert!(!connection.warning);
        assert_eq!(connection.live_pico_count, 0);
        assert_eq!(connection.surveyed_live_pico_count, 0);
        assert!(connection.per_pico.is_empty());
    }

    #[test]
    fn adapter_connection_reports_descriptor_rejection_as_actionable_evidence() {
        let attempts = vec![survey_attempt(
            "ps4",
            false,
            "descriptor_or_report_rejected",
            1,
            1,
        )];
        let report =
            test_survey_report("02E22DA9", "xinput", "confirmed", Some("xinput"), attempts);
        let mut capture = pico_capture(
            "02E22DA9",
            true,
            "{\"persona\":\"xinput\"}\n",
            "usb-packet seq=1 dir=setup data=8006000100001200\n",
        );
        capture.adapter_survey_report = Some(report);

        let connection = adapter_connection_report(&[capture]);
        assert_eq!(connection.status, "descriptor_or_report_rejected");
        assert!(!connection.warning);
        assert_eq!(connection.host_traffic_pico_count, 1);
        assert_eq!(connection.descriptor_or_report_rejected_pico_count, 1);
        assert_eq!(connection.per_pico[0].raw_packet_lines, 1);
    }

    #[test]
    fn aggregate_initial_usb_capture_preserves_pre_survey_packet_lines() {
        let mut capture = pico_capture("02E22DA9", true, "{\"persona\":\"ps4\"}\n", "");
        capture.initial_usb_capture_text = usb_packets_text_from_diag(
            "02E22DA9",
            "usb-packet seq=1 dir=setup bm=0x80 req=0x06 data=8006000100001200\n",
        );

        let text = aggregate_initial_usb_capture_text(&[capture]);
        assert!(text.contains("path=picos/02E22DA9/initial-usb-capture.txt"));
        assert!(text.contains("usb-packet seq=1 dir=setup"));
        assert_eq!(count_usb_packet_lines(&text), 1);
    }

    fn survey_attempt(
        persona: &str,
        accepted: bool,
        verdict: &str,
        score_rank: u8,
        device_desc_count: u32,
    ) -> AdapterSurveyAttempt {
        AdapterSurveyAttempt {
            persona: persona.to_string(),
            current_at_start: false,
            switched: true,
            usb_diag_captured: true,
            score_rank,
            score: "test score".to_string(),
            accepted,
            verdict: verdict.to_string(),
            device_desc_count,
            config_desc_count: if accepted { 1 } else { 0 },
            mount_count: if accepted { 1 } else { 0 },
            umount_count: 0,
            suspend_count: 0,
            resume_count: 0,
            input_report_sent_count: 0,
            host_out_count: 0,
            raw_capture: AdapterSurveyRawCapture::not_attempted("not_needed"),
        }
    }

    fn current_survey_attempt(
        persona: &str,
        accepted: bool,
        verdict: &str,
        score_rank: u8,
        device_desc_count: u32,
    ) -> AdapterSurveyAttempt {
        let mut attempt = survey_attempt(persona, accepted, verdict, score_rank, device_desc_count);
        attempt.current_at_start = true;
        attempt.switched = false;
        attempt
    }

    fn all_no_host_survey_attempts() -> Vec<AdapterSurveyAttempt> {
        vec![
            current_survey_attempt("debug", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("ps3", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("generic-hid", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("ps4", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("keyboard", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("xinput", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("xboxone", false, "adapter_did_not_enumerate", 0, 0),
            survey_attempt("maple", false, "adapter_did_not_enumerate", 0, 0),
        ]
    }

    fn test_survey_report(
        uid: &str,
        original_persona: &str,
        restore_status: &str,
        restored_persona: Option<&str>,
        attempts: Vec<AdapterSurveyAttempt>,
    ) -> super::AdapterSurveyReport {
        build_adapter_survey_report(
            uid.to_string(),
            original_persona.to_string(),
            restore_status.to_string(),
            restored_persona.map(|persona| persona.to_string()),
            attempts,
            vec![],
        )
    }

    fn bluetooth_target(persona: protocol::Persona) -> crate::cmd_run::PicoTarget {
        crate::cmd_run::PicoTarget {
            peer: "10.0.0.24:4242".parse().unwrap(),
            info: protocol::AckInfo {
                proto_version: protocol::PROTO_VERSION,
                fw_major: 26,
                fw_minor: 6,
                fw_patch: 20,
                board_type: protocol::BOARD_PICO_2_W,
                uptime_seconds: 20,
                unique_id_short: 0x02E22DA9,
                full_version: None,
            },
            persona,
            ack_flags: 0,
        }
    }

    fn bluetooth_pico_state(bt_flags: u8, bt_report_send_count: u32) -> protocol::PicoStateDiag {
        protocol::PicoStateDiag {
            seq: 1,
            flags: 0,
            version: protocol::PICO_STATE_VERSION,
            proto_version: protocol::PROTO_VERSION,
            board_type: protocol::BOARD_PICO_2_W,
            persona_byte: protocol::Persona::BluetoothHid.flash_byte(),
            unique_id_short: 0x02E22DA9,
            uptime_seconds: 20,
            tx_count: 10,
            rx_count: 20,
            now_ms: 1000,
            last_bridge_packet_ms: 990,
            mount_count: 1,
            umount_count: 0,
            suspend_count: 0,
            resume_count: 0,
            device_desc_count: 2,
            config_desc_count: 1,
            xinput_in_queued_count: 3,
            xinput_in_sent_count: 3,
            xinput_out_count: 0,
            xinput_in_blocked_not_mounted_count: 0,
            xinput_in_blocked_not_ready_count: 0,
            xinput_in_blocked_short_write_count: 0,
            xinput_in_idle_suppressed_count: 0,
            last_mount_ms: 100,
            last_umount_ms: 0,
            last_in_queued_ms: 900,
            last_in_sent_ms: 901,
            last_out_ms: 0,
            last_in_blocked_ms: 0,
            last_in_blocked_reason: protocol::USB_DIAG_IN_BLOCKED_NONE,
            last_in_blocked_want: 0,
            last_in_blocked_got: 0,
            last_out_len: 0,
            last_out_byte0: 0,
            last_out_byte1: 0,
            usb_flags: protocol::USB_DIAG_FLAG_MOUNTED,
            activity_flags: protocol::USB_DIAG_ACTIVITY_SENT,
            malformed_udp_count: 0,
            bt_flags,
            bt_target: 0,
            bt_last_status: 0,
            bt_report_len: 12,
            bt_cid: 7,
            bt_init_count: if bt_flags & protocol::BT_HID_STATUS_STARTED != 0 {
                1
            } else {
                0
            },
            bt_ready_count: if bt_flags & protocol::BT_HID_STATUS_STARTED != 0 {
                1
            } else {
                0
            },
            bt_open_count: if bt_flags & protocol::BT_HID_STATUS_CONNECTED != 0 {
                1
            } else {
                0
            },
            bt_close_count: 0,
            bt_can_send_count: bt_report_send_count,
            bt_report_build_count: bt_report_send_count,
            bt_report_send_count,
            bt_send_request_count: bt_report_send_count,
            bt_last_event_ms: 800,
            bt_last_send_ms: if bt_report_send_count > 0 { 900 } else { 0 },
        }
    }

    fn bluetooth_usb_diag() -> protocol::UsbDiag {
        protocol::UsbDiag {
            seq: 1,
            flags: 0,
            version: protocol::USB_DIAG_VERSION,
            usb_flags: protocol::USB_DIAG_FLAG_MOUNTED,
            activity_flags: protocol::USB_DIAG_ACTIVITY_SENT,
            last_out_len: 0,
            now_ms: 1000,
            last_bridge_packet_ms: 990,
            mount_count: 1,
            umount_count: 0,
            suspend_count: 0,
            resume_count: 0,
            device_desc_count: 2,
            config_desc_count: 1,
            xinput_in_queued_count: 3,
            xinput_in_sent_count: 3,
            xinput_out_count: 0,
            xinput_in_blocked_not_mounted_count: 0,
            xinput_in_blocked_not_ready_count: 0,
            xinput_in_blocked_short_write_count: 0,
            xinput_in_idle_suppressed_count: 0,
            last_mount_ms: 100,
            last_umount_ms: 0,
            last_in_queued_ms: 900,
            last_in_sent_ms: 901,
            last_out_ms: 0,
            last_in_blocked_ms: 0,
            last_in_blocked_reason: protocol::USB_DIAG_IN_BLOCKED_NONE,
            last_in_blocked_want: 0,
            last_in_blocked_got: 0,
            last_out_byte0: 0,
            last_out_byte1: 0,
        }
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
                } else if usb_packets_text.contains("usb-event ") {
                    "lifecycle_only"
                } else {
                    "no_packets"
                }
                .to_string(),
                usb_packet_dump_count: count_usb_packet_lines(usb_packets_text),
                bluetooth_report_status: "not_applicable".to_string(),
                cached_state_included: false,
            },
            state_json: state_json.to_string(),
            pico_diag_text: String::new(),
            usb_diag_text: String::new(),
            initial_usb_capture_text: String::new(),
            usb_packets_text: usb_packets_text.to_string(),
            adapter_survey_text: String::new(),
            adapter_survey_json: String::new(),
            adapter_survey_report: None,
            bluetooth_report_text: String::new(),
            bluetooth_report_json: String::new(),
            bluetooth_report: None,
        }
    }
}
