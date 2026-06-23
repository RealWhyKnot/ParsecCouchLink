//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! crash files, Pico diag log, and a manifest.json with non-sensitive system
//! info. Intended to be attached to a bug report.
//!
//! NEVER include Wi-Fi credentials. The Pico stores them and the bridge
//! never reads them. SSID is also omitted by default to be safe.

mod adapter_survey;
mod bluetooth_report;
mod capture;
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

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use chrono::Local;

use crate::{journal, pico_cache};

use adapter_survey::{
    adapter_connection_json, adapter_connection_report, adapter_connection_text,
    adapter_survey_bundle_json, aggregate_adapter_survey_text,
};
#[cfg(test)]
use adapter_survey::{
    adapter_survey_candidates, adapter_survey_report_json, adapter_survey_text,
    build_adapter_survey_report, AdapterSurveyAttempt, AdapterSurveyRawCapture,
    AdapterSurveyReport,
};
use bluetooth_report::{aggregate_bluetooth_report_text, bluetooth_report_bundle_json};
#[cfg(test)]
use bluetooth_report::{
    bluetooth_usb_packets_stub, build_bluetooth_report, format_bluetooth_report_json,
    format_bluetooth_report_text,
};
#[cfg(test)]
use capture::sanitize_path_component;
use capture::{capture_per_pico, capture_usb_diag_text, collect_retained_debug_packet_logs};
use capture::{CaptureLog, PicoBundleCapture, RetainedDebugPacketLog, UsbDiagBundle};
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
use usb_packets::{aggregate_initial_usb_capture_text, count_retained_debug_packet_lines};
#[cfg(test)]
use usb_packets::{
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, usb_packets_text_from_debug_snapshot, usb_packets_text_from_diag,
};
use zip_writer::{write_bundle_zip, BundleZipContents};

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
mod tests;
