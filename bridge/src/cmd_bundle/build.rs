use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use super::adapter_survey::AdapterConnectionReport;
use super::host_snapshot::HostSnapshotFile;
use super::usb_packet_summary::UsbPacketBundleSummary;
use super::{
    BundleZipContents, CaptureLog, DiagOutcome, ManifestHostSnapshot, ManifestInput,
    ManifestPicoCapture, PicoBundleCapture, PicoEnumState, RetainedDebugPacketLog, UsbDiagBundle,
    UsbPacketSummarySource, adapter_connection_json, adapter_connection_report,
    adapter_connection_text, adapter_survey_bundle_json, aggregate_adapter_survey_text,
    aggregate_bluetooth_report_text, aggregate_initial_usb_capture_text,
    bluetooth_report_bundle_json, build_manifest, build_system_info, capture_host_snapshots,
    capture_per_pico, capture_pico_diag, capture_usb_devices, capture_usb_diag_text,
    capture_windows_usb_events, classify_pico_enum, collect_crash_file_names,
    collect_retained_debug_packet_logs, collect_setup_transcript_names,
    control_transfers_text_for_sources, count_retained_debug_packet_lines,
    debug_capture_evidence_report_json, debug_capture_overall_status, debug_capture_verdict_text,
    enumeration_analysis_text_for_sources, hid_reports_text_for_sources,
    packet_timeline_text_for_sources, records_jsonl_for_sources, run_doctor_silently,
    summarize_sources, write_bundle_zip,
};
use crate::{journal, pico_cache};

#[derive(Clone, Debug)]
pub(crate) struct BundleSummary {
    pub zip_path: PathBuf,
    pub pico_diag_captured: bool,
    pub pico_diag_outcome: String,
    pub pico_diag_source: Option<String>,
    pub crash_file_count: usize,
    pub setup_transcript_count: usize,
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
}

pub(super) struct BundleReports {
    pub adapter_connection: AdapterConnectionReport,
    pub adapter_connection_text: String,
    pub adapter_connection_json: String,
    pub initial_usb_capture_text: String,
    pub adapter_survey_text: String,
    pub adapter_survey_json: String,
    pub bluetooth_report_text: String,
    pub bluetooth_report_json: String,
}

pub(super) struct PacketOutputs {
    pub summary: UsbPacketBundleSummary,
    pub summary_json: String,
    pub records_jsonl: String,
    pub control_transfers_text: String,
    pub hid_reports_text: String,
    pub timeline_text: String,
    pub enumeration_analysis_text: String,
    pub debug_capture_status: &'static str,
    pub debug_capture_verdict: String,
    pub debug_capture_evidence_json: String,
}

struct HostCaptures {
    diag: DiagOutcome,
    crash_files: Vec<String>,
    setup_transcripts: Vec<String>,
    usb_devices: Option<(String, &'static str)>,
    usb_events: Option<String>,
    usb_diag: UsbDiagBundle,
}

struct ManifestSources<'a> {
    host: &'a HostCaptures,
    pico_enum_state: &'a PicoEnumState,
    per_pico_captures: &'a [PicoBundleCapture],
    retained_debug_packet_logs: &'a [RetainedDebugPacketLog],
    host_snapshots: &'a [HostSnapshotFile],
    diagnostic_cache_included: bool,
}

pub(crate) async fn build_bundle(out_path: PathBuf) -> Result<BundleSummary> {
    journal!("bundle", "run started");
    tracing::info!("bundle: run started out_path={}", out_path.display());
    let mut capture_log = CaptureLog::default();

    let host = capture_host_side(&mut capture_log).await;
    let per_pico_captures = capture_per_pico(&mut capture_log).await;
    let reports = build_reports(&per_pico_captures)?;
    let retained_debug_packet_logs = collect_retained_debug_packet_logs(&mut capture_log);
    let packets = build_packet_outputs(&per_pico_captures, &retained_debug_packet_logs)?;
    record_report_sizes(&mut capture_log, &reports, &packets);
    record_packet_output_sizes(&mut capture_log, &packets);

    let host_snapshots = capture_logged_host_snapshots(&mut capture_log).await;
    let (cache_current, cache_history) = read_diagnostic_cache(&mut capture_log);
    let diagnostic_cache_included = cache_current.is_some() || cache_history.is_some();
    let pico_enum_state = host
        .usb_devices
        .as_ref()
        .filter(|(_, m)| *m == "pnputil")
        .map_or(PicoEnumState::NotEnumerated, |(text, _)| {
            classify_pico_enum(text)
        });
    let system_info = build_system_info().await;
    let manifest_json = bundle_manifest_json(&ManifestSources {
        host: &host,
        pico_enum_state: &pico_enum_state,
        per_pico_captures: &per_pico_captures,
        retained_debug_packet_logs: &retained_debug_packet_logs,
        host_snapshots: &host_snapshots,
        diagnostic_cache_included,
    })
    .await?;
    let doctor_text = run_doctor_silently().await;

    write_bundle_zip(&BundleZipContents {
        out_path: &out_path,
        manifest_json: &manifest_json,
        capture_log: &capture_log,
        doctor_text: &doctor_text,
        diag: &host.diag,
        pico_enum_state: &pico_enum_state,
        usb_diag: &host.usb_diag,
        reports: &reports,
        per_pico_captures: &per_pico_captures,
        retained_debug_packet_logs: &retained_debug_packet_logs,
        packets: &packets,
        cache_current: &cache_current,
        cache_history: &cache_history,
        host_snapshots: &host_snapshots,
        system_info: &system_info,
        usb_devices: &host.usb_devices,
        usb_events: &host.usb_events,
    })?;
    let usb_packet_dump_count: usize = per_pico_captures
        .iter()
        .map(|capture| capture.manifest.usb_packet_dump_count)
        .sum();
    let retained_debug_packet_count =
        count_retained_debug_packet_lines(&retained_debug_packet_logs);
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
        pico_diag_captured: matches!(host.diag, DiagOutcome::Captured { .. }),
        pico_diag_outcome: host.diag.discriminant_str().to_string(),
        pico_diag_source: host.diag.source_str().map(std::string::ToString::to_string),
        crash_file_count: host.crash_files.len(),
        setup_transcript_count: host.setup_transcripts.len(),
        usb_diag_captured: host.usb_diag.captured,
        usb_diag_target_count: host.usb_diag.target_count,
        usb_packet_dump_count,
        retained_debug_packet_log_count: retained_debug_packet_logs.len(),
        retained_debug_packet_count,
        debug_capture_status: packets.debug_capture_status.to_string(),
        adapter_connection_status: reports.adapter_connection.status.to_string(),
        adapter_connection_warning: reports.adapter_connection.warning,
        per_pico_capture_count: per_pico_captures.len(),
        bluetooth_report_count: per_pico_captures
            .iter()
            .filter(|capture| capture.bluetooth_report.is_some())
            .count(),
    })
}

async fn capture_host_side(capture_log: &mut CaptureLog) -> HostCaptures {
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

    let crash_files = collect_crash_file_names();
    let setup_transcripts = collect_setup_transcript_names();
    let usb_devices = capture_usb_devices().await;
    let usb_events = capture_windows_usb_events().await;

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
    HostCaptures {
        diag,
        crash_files,
        setup_transcripts,
        usb_devices,
        usb_events,
        usb_diag,
    }
}

fn build_reports(per_pico_captures: &[PicoBundleCapture]) -> Result<BundleReports> {
    let adapter_connection = adapter_connection_report(per_pico_captures);
    let adapter_connection_text = adapter_connection_text(&adapter_connection);
    let adapter_connection_json = adapter_connection_json(&adapter_connection)?;
    Ok(BundleReports {
        adapter_connection,
        adapter_connection_text,
        adapter_connection_json,
        initial_usb_capture_text: aggregate_initial_usb_capture_text(per_pico_captures),
        adapter_survey_text: aggregate_adapter_survey_text(per_pico_captures),
        adapter_survey_json: adapter_survey_bundle_json(per_pico_captures)?,
        bluetooth_report_text: aggregate_bluetooth_report_text(per_pico_captures),
        bluetooth_report_json: bluetooth_report_bundle_json(per_pico_captures)?,
    })
}

fn build_packet_outputs(
    per_pico_captures: &[PicoBundleCapture],
    retained_debug_packet_logs: &[RetainedDebugPacketLog],
) -> Result<PacketOutputs> {
    let per_pico: Vec<_> = per_pico_captures
        .iter()
        .map(|capture| UsbPacketSummarySource {
            label: capture.manifest.uid.clone(),
            path: format!("{}/usb-packets.txt", capture.manifest.path),
            text: &capture.usb_packets_text,
        })
        .collect();
    let retained: Vec<_> = retained_debug_packet_logs
        .iter()
        .map(|log| UsbPacketSummarySource {
            label: log.name.clone(),
            path: format!("debug-packets/{}", log.name),
            text: &log.text,
        })
        .collect();
    let summary = summarize_sources(&per_pico, &retained);
    let summary_json = serde_json::to_string_pretty(&summary)?;
    let records_jsonl = records_jsonl_for_sources(&per_pico, &retained)?;
    let debug_capture_status =
        debug_capture_overall_status(&summary, per_pico_captures, retained_debug_packet_logs);
    let debug_capture_verdict =
        debug_capture_verdict_text(per_pico_captures, retained_debug_packet_logs, &summary);
    let debug_capture_evidence_json = debug_capture_evidence_report_json(
        per_pico_captures,
        retained_debug_packet_logs,
        &summary,
    )?;
    Ok(PacketOutputs {
        summary_json,
        records_jsonl,
        control_transfers_text: control_transfers_text_for_sources(&per_pico, &retained),
        hid_reports_text: hid_reports_text_for_sources(&per_pico, &retained),
        timeline_text: packet_timeline_text_for_sources(&per_pico, &retained),
        enumeration_analysis_text: enumeration_analysis_text_for_sources(&per_pico, &retained),
        debug_capture_status,
        debug_capture_verdict,
        debug_capture_evidence_json,
        summary,
    })
}

fn record_report_sizes(
    capture_log: &mut CaptureLog,
    reports: &BundleReports,
    packets: &PacketOutputs,
) {
    capture_log.record_duration(
        "usb_packet_summary",
        0,
        "included",
        packets.summary_json.len(),
        format!(
            "raw_packets={}; stats={}",
            packets.summary.aggregate.packet_lines, packets.summary.aggregate.stats_lines
        ),
    );
    capture_log.record_duration(
        "adapter_survey",
        0,
        "included",
        reports
            .adapter_survey_text
            .len()
            .saturating_add(reports.adapter_survey_json.len()),
        "txt_and_json",
    );
    capture_log.record_duration(
        "adapter_connection",
        0,
        reports.adapter_connection.status,
        reports
            .adapter_connection_text
            .len()
            .saturating_add(reports.adapter_connection_json.len()),
        "txt_and_json",
    );
    capture_log.record_duration(
        "bluetooth_report",
        0,
        "included",
        reports
            .bluetooth_report_text
            .len()
            .saturating_add(reports.bluetooth_report_json.len()),
        "txt_and_json",
    );
}

fn record_packet_output_sizes(capture_log: &mut CaptureLog, packets: &PacketOutputs) {
    let outputs = [
        ("usb_packet_records", packets.records_jsonl.len(), "jsonl"),
        (
            "usb_control_transfers",
            packets.control_transfers_text.len(),
            "setup_and_control_in",
        ),
        (
            "usb_hid_reports",
            packets.hid_reports_text.len(),
            "hid_report_metadata",
        ),
        (
            "usb_packet_timeline",
            packets.timeline_text.len(),
            "packet_timing",
        ),
        (
            "usb_enumeration_analysis",
            packets.enumeration_analysis_text.len(),
            "enumeration_phase_checklist",
        ),
        (
            "debug_capture_verdict",
            packets.debug_capture_verdict.len(),
            packets.debug_capture_status,
        ),
        (
            "debug_capture_evidence",
            packets.debug_capture_evidence_json.len(),
            "json",
        ),
    ];
    for (name, bytes, detail) in outputs {
        capture_log.record_duration(name, 0, "included", bytes, detail);
    }
}

async fn capture_logged_host_snapshots(capture_log: &mut CaptureLog) -> Vec<HostSnapshotFile> {
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
    host_snapshots
}

fn read_diagnostic_cache(capture_log: &mut CaptureLog) -> (Option<String>, Option<String>) {
    let started = Instant::now();
    let cache_current = pico_cache::read_current();
    let cache_history = pico_cache::read_history();
    capture_log.record(
        "diagnostic_cache",
        started,
        if cache_current.is_some() || cache_history.is_some() {
            "included"
        } else {
            "not_present"
        },
        cache_current.as_ref().map_or(0, std::string::String::len)
            + cache_history.as_ref().map_or(0, std::string::String::len),
        "",
    );
    (cache_current, cache_history)
}

async fn bundle_manifest_json(sources: &ManifestSources<'_>) -> Result<String> {
    let host = sources.host;
    let per_pico_manifest: Vec<ManifestPicoCapture> = sources
        .per_pico_captures
        .iter()
        .map(|capture| capture.manifest.clone())
        .collect();
    let host_snapshot_manifest: Vec<ManifestHostSnapshot> = sources
        .host_snapshots
        .iter()
        .map(|snapshot| snapshot.manifest.clone())
        .collect();
    let retained_debug_packet_log_names: Vec<String> = sources
        .retained_debug_packet_logs
        .iter()
        .map(|log| log.name.clone())
        .collect();
    let pico_usb_mode = match sources.pico_enum_state {
        PicoEnumState::NotEnumerated => None,
        PicoEnumState::EnumeratedSetupMode => Some("setup"),
        PicoEnumState::EnumeratedParentOnly => Some("parent_only"),
        PicoEnumState::EnumeratedRunMode => Some("run"),
    };

    let manifest = build_manifest(&ManifestInput {
        pico_diag_captured: matches!(host.diag, DiagOutcome::Captured { .. }).into(),
        pico_diag_lost_bytes: host.diag.lost_bytes(),
        pico_diag_outcome: host.diag.discriminant_str(),
        pico_diag_source: host.diag.source_str(),
        usb_devices_captured: host.usb_devices.is_some().into(),
        usb_capture_method: host.usb_devices.as_ref().map_or("none", |(_, m)| *m),
        usb_events_captured: host.usb_events.is_some().into(),
        pico_usb_enumerated: pico_usb_mode.is_some(),
        pico_usb_mode,
        pico_usb_diag_captured: host.usb_diag.captured.into(),
        pico_usb_diag_target_count: host.usb_diag.target_count,
        retained_debug_packet_logs: &retained_debug_packet_log_names,
        retained_debug_packet_count: count_retained_debug_packet_lines(
            sources.retained_debug_packet_logs,
        ),
        diagnostic_cache_included: sources.diagnostic_cache_included.into(),
        per_pico_capture_outcomes: &per_pico_manifest,
        host_snapshots: &host_snapshot_manifest,
        crash_files: &host.crash_files,
        setup_transcripts: &host.setup_transcripts,
    })
    .await?;
    Ok(serde_json::to_string_pretty(&manifest)?)
}
