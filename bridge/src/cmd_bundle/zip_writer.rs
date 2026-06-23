use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::collect::bundle_log_prefix;
use super::host_snapshot::HostSnapshotFile;
use super::pico_diag::DiagOutcome;
use super::redact::redact_bundle_text;
use super::usb_enum::{parent_only_stub_text, vendor_not_found_stub_text, PicoEnumState};
use super::usb_packet_summary::{
    control_transfers_text_for_text, enumeration_analysis_text_for_text, hid_reports_text_for_text,
    packet_timeline_text_for_text, records_jsonl_for_text, summarize_text,
};
use super::usb_packets::aggregate_usb_packets;
use super::{CaptureLog, PicoBundleCapture, RetainedDebugPacketLog, UsbDiagBundle};
use crate::{config, journal};

pub(super) struct BundleZipContents<'a> {
    pub out_path: &'a Path,
    pub manifest_json: &'a str,
    pub capture_log: &'a CaptureLog,
    pub doctor_text: &'a str,
    pub diag: &'a DiagOutcome,
    pub pico_enum_state: &'a PicoEnumState,
    pub usb_diag: &'a UsbDiagBundle,
    pub adapter_connection_text: &'a str,
    pub adapter_connection_json: &'a str,
    pub initial_usb_capture_text: &'a str,
    pub adapter_survey_text: &'a str,
    pub adapter_survey_json: &'a str,
    pub bluetooth_report_text: &'a str,
    pub bluetooth_report_json: &'a str,
    pub per_pico_captures: &'a [PicoBundleCapture],
    pub retained_debug_packet_logs: &'a [RetainedDebugPacketLog],
    pub usb_packet_summary_json: &'a str,
    pub usb_packet_records_jsonl: &'a str,
    pub usb_control_transfers_text: &'a str,
    pub usb_hid_reports_text: &'a str,
    pub usb_packet_timeline_text: &'a str,
    pub usb_enumeration_analysis_text: &'a str,
    pub debug_capture_verdict: &'a str,
    pub debug_capture_evidence_json: &'a str,
    pub cache_current: &'a Option<String>,
    pub cache_history: &'a Option<String>,
    pub host_snapshots: &'a [HostSnapshotFile],
    pub system_info: &'a str,
    pub usb_devices: &'a Option<(String, &'static str)>,
    pub usb_events: &'a Option<String>,
}

pub(super) fn write_bundle_zip(contents: BundleZipContents<'_>) -> Result<()> {
    let BundleZipContents {
        out_path,
        manifest_json,
        capture_log,
        doctor_text,
        diag,
        pico_enum_state,
        usb_diag,
        adapter_connection_text,
        adapter_connection_json,
        initial_usb_capture_text,
        adapter_survey_text,
        adapter_survey_json,
        bluetooth_report_text,
        bluetooth_report_json,
        per_pico_captures,
        retained_debug_packet_logs,
        usb_packet_summary_json,
        usb_packet_records_jsonl,
        usb_control_transfers_text,
        usb_hid_reports_text,
        usb_packet_timeline_text,
        usb_enumeration_analysis_text,
        debug_capture_verdict,
        debug_capture_evidence_json,
        cache_current,
        cache_history,
        host_snapshots,
        system_info,
        usb_devices,
        usb_events,
    } = contents;

    let f = std::fs::File::create(out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    let mut zip = ZipWriter::new(f);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("manifest.json", opts)?;
    zip.write_all(redact_bundle_text(manifest_json).as_bytes())?;

    zip.start_file("bundle-capture.txt", opts)?;
    zip.write_all(redact_bundle_text(&capture_log.text()).as_bytes())?;

    zip.start_file("doctor.txt", opts)?;
    zip.write_all(redact_bundle_text(doctor_text).as_bytes())?;

    // Always write pico-diag.txt. The body is a self-narrating stub
    // when capture failed; the per-variant message names the failing
    // step so the bundle is actionable without reading the bridge log.
    // VendorNotFound and parent-only VendorOpenFailed are special: their
    // stub text depends on the USB topology captured in pico_enum_state.
    let pico_diag_body = match (diag, pico_enum_state) {
        (DiagOutcome::VendorNotFound, _) => vendor_not_found_stub_text(pico_enum_state),
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

    zip.start_file("adapter-connection.txt", opts)?;
    zip.write_all(redact_bundle_text(adapter_connection_text).as_bytes())?;

    zip.start_file("adapter-connection.json", opts)?;
    zip.write_all(redact_bundle_text(adapter_connection_json).as_bytes())?;

    zip.start_file("initial-usb-capture.txt", opts)?;
    zip.write_all(redact_bundle_text(initial_usb_capture_text).as_bytes())?;

    zip.start_file("adapter-survey.txt", opts)?;
    zip.write_all(redact_bundle_text(adapter_survey_text).as_bytes())?;

    zip.start_file("adapter-survey.json", opts)?;
    zip.write_all(redact_bundle_text(adapter_survey_json).as_bytes())?;

    zip.start_file("bluetooth-report.txt", opts)?;
    zip.write_all(redact_bundle_text(bluetooth_report_text).as_bytes())?;

    zip.start_file("bluetooth-report.json", opts)?;
    zip.write_all(redact_bundle_text(bluetooth_report_json).as_bytes())?;

    for pico in per_pico_captures {
        let base = pico.manifest.path.trim_end_matches('/');
        zip.start_file(format!("{base}/state.json"), opts)?;
        zip.write_all(redact_bundle_text(&pico.state_json).as_bytes())?;

        zip.start_file(format!("{base}/pico-diag.txt"), opts)?;
        zip.write_all(redact_bundle_text(&pico.pico_diag_text).as_bytes())?;

        zip.start_file(format!("{base}/usb-diag.txt"), opts)?;
        zip.write_all(redact_bundle_text(&pico.usb_diag_text).as_bytes())?;

        zip.start_file(format!("{base}/initial-usb-capture.txt"), opts)?;
        zip.write_all(redact_bundle_text(&pico.initial_usb_capture_text).as_bytes())?;

        if !pico.adapter_survey_text.is_empty() {
            zip.start_file(format!("{base}/adapter-survey.txt"), opts)?;
            zip.write_all(redact_bundle_text(&pico.adapter_survey_text).as_bytes())?;
        }

        if !pico.adapter_survey_json.is_empty() {
            zip.start_file(format!("{base}/adapter-survey.json"), opts)?;
            zip.write_all(redact_bundle_text(&pico.adapter_survey_json).as_bytes())?;
        }

        if !pico.bluetooth_report_text.is_empty() {
            zip.start_file(format!("{base}/bluetooth-report.txt"), opts)?;
            zip.write_all(redact_bundle_text(&pico.bluetooth_report_text).as_bytes())?;
        }

        if !pico.bluetooth_report_json.is_empty() {
            zip.start_file(format!("{base}/bluetooth-report.json"), opts)?;
            zip.write_all(redact_bundle_text(&pico.bluetooth_report_json).as_bytes())?;
        }

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

        zip.start_file(format!("{base}/usb-enumeration-analysis.txt"), opts)?;
        let enumeration_analysis = enumeration_analysis_text_for_text(
            &pico.manifest.uid,
            &format!("{base}/usb-packets.txt"),
            &pico.usb_packets_text,
        );
        zip.write_all(redact_bundle_text(&enumeration_analysis).as_bytes())?;
    }

    zip.start_file("usb-packets.txt", opts)?;
    zip.write_all(
        redact_bundle_text(&aggregate_usb_packets(
            per_pico_captures,
            retained_debug_packet_logs,
        ))
        .as_bytes(),
    )?;

    zip.start_file("usb-packets-summary.json", opts)?;
    zip.write_all(redact_bundle_text(usb_packet_summary_json).as_bytes())?;

    zip.start_file("usb-packets.jsonl", opts)?;
    zip.write_all(redact_bundle_text(usb_packet_records_jsonl).as_bytes())?;

    zip.start_file("usb-control-transfers.txt", opts)?;
    zip.write_all(redact_bundle_text(usb_control_transfers_text).as_bytes())?;

    zip.start_file("usb-hid-reports.txt", opts)?;
    zip.write_all(redact_bundle_text(usb_hid_reports_text).as_bytes())?;

    zip.start_file("usb-packet-timeline.txt", opts)?;
    zip.write_all(redact_bundle_text(usb_packet_timeline_text).as_bytes())?;

    zip.start_file("usb-enumeration-analysis.txt", opts)?;
    zip.write_all(redact_bundle_text(usb_enumeration_analysis_text).as_bytes())?;

    zip.start_file("debug-capture-verdict.txt", opts)?;
    zip.write_all(redact_bundle_text(debug_capture_verdict).as_bytes())?;

    zip.start_file("debug-capture-evidence.json", opts)?;
    zip.write_all(redact_bundle_text(debug_capture_evidence_json).as_bytes())?;

    for log in retained_debug_packet_logs {
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

    for snapshot in host_snapshots {
        zip.start_file(snapshot.manifest.path.as_str(), opts)?;
        zip.write_all(redact_bundle_text(&snapshot.text).as_bytes())?;
    }

    // system-info.txt: always present. Captures the Windows build,
    // couchlink version, last-known Pico identity, short hostname.
    zip.start_file("system-info.txt", opts)?;
    zip.write_all(redact_bundle_text(system_info).as_bytes())?;

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
                                let text = String::from_utf8_lossy(&bytes);
                                zip.write_all(redact_bundle_text(&text).as_bytes())?;
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
                    let text = String::from_utf8_lossy(&bytes);
                    zip.write_all(redact_bundle_text(&text).as_bytes())?;
                }
                Err(e) => {
                    tracing::debug!("bundle: could not read state journal: {e}");
                }
            }
        }
    }

    zip.finish()?;
    Ok(())
}
