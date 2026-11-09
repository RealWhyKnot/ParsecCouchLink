use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::build::{BundleReports, PacketOutputs};
use super::collect::bundle_log_prefix;
use super::host_snapshot::HostSnapshotFile;
use super::pico_diag::DiagOutcome;
use super::redact::redact_bundle_text;
use super::usb_enum::{PicoEnumState, parent_only_stub_text, vendor_not_found_stub_text};
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
    pub reports: &'a BundleReports,
    pub per_pico_captures: &'a [PicoBundleCapture],
    pub retained_debug_packet_logs: &'a [RetainedDebugPacketLog],
    pub packets: &'a PacketOutputs,
    pub cache_current: &'a Option<String>,
    pub cache_history: &'a Option<String>,
    pub host_snapshots: &'a [HostSnapshotFile],
    pub system_info: &'a str,
    pub usb_devices: &'a Option<(String, &'static str)>,
    pub usb_events: &'a Option<String>,
}

type BundleZip = ZipWriter<std::fs::File>;

fn file_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
}

fn add_redacted(zip: &mut BundleZip, name: &str, text: &str) -> Result<()> {
    zip.start_file(name, file_options())?;
    zip.write_all(redact_bundle_text(text).as_bytes())?;
    Ok(())
}

pub(super) fn write_bundle_zip(contents: &BundleZipContents<'_>) -> Result<()> {
    let out_path = contents.out_path;
    let f = std::fs::File::create(out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    let mut zip = ZipWriter::new(f);

    add_top_level_reports(&mut zip, contents)?;
    for pico in contents.per_pico_captures {
        add_pico_files(&mut zip, pico)?;
    }
    add_packet_outputs(&mut zip, contents)?;
    add_host_files(&mut zip, contents)?;
    add_usb_device_files(&mut zip, contents)?;
    add_crash_files(&mut zip)?;

    if let Ok(log_dir) = config::log_dir() {
        bundle_log_prefix(&log_dir, "couchlink.", &mut zip, file_options())?;
        bundle_log_prefix(&log_dir, "setup-", &mut zip, file_options())?;
    }
    add_state_journal(&mut zip)?;

    zip.finish()?;
    Ok(())
}

fn add_top_level_reports(zip: &mut BundleZip, contents: &BundleZipContents<'_>) -> Result<()> {
    let reports = contents.reports;
    add_redacted(zip, "manifest.json", contents.manifest_json)?;
    add_redacted(zip, "bundle-capture.txt", &contents.capture_log.text())?;
    add_redacted(zip, "doctor.txt", contents.doctor_text)?;

    let pico_diag_body = match (contents.diag, contents.pico_enum_state) {
        (DiagOutcome::VendorNotFound, _) => vendor_not_found_stub_text(contents.pico_enum_state),
        (DiagOutcome::VendorOpenFailed { .. }, PicoEnumState::EnumeratedParentOnly) => {
            parent_only_stub_text()
        }
        _ => contents.diag.stub_text(),
    };
    add_redacted(zip, "pico-diag.txt", &pico_diag_body)?;
    add_redacted(zip, "usb-diag.txt", &contents.usb_diag.text)?;
    add_redacted(
        zip,
        "adapter-connection.txt",
        &reports.adapter_connection_text,
    )?;
    add_redacted(
        zip,
        "adapter-connection.json",
        &reports.adapter_connection_json,
    )?;
    add_redacted(
        zip,
        "initial-usb-capture.txt",
        &reports.initial_usb_capture_text,
    )?;
    add_redacted(zip, "adapter-survey.txt", &reports.adapter_survey_text)?;
    add_redacted(zip, "adapter-survey.json", &reports.adapter_survey_json)?;
    add_redacted(zip, "bluetooth-report.txt", &reports.bluetooth_report_text)?;
    add_redacted(zip, "bluetooth-report.json", &reports.bluetooth_report_json)
}

fn add_pico_files(zip: &mut BundleZip, pico: &PicoBundleCapture) -> Result<()> {
    let base = pico.manifest.path.trim_end_matches('/');
    add_redacted(zip, &format!("{base}/state.json"), &pico.state_json)?;
    add_redacted(zip, &format!("{base}/pico-diag.txt"), &pico.pico_diag_text)?;
    add_redacted(zip, &format!("{base}/usb-diag.txt"), &pico.usb_diag_text)?;
    add_redacted(
        zip,
        &format!("{base}/initial-usb-capture.txt"),
        &pico.initial_usb_capture_text,
    )?;

    let optional = [
        ("adapter-survey.txt", &pico.adapter_survey_text),
        ("adapter-survey.json", &pico.adapter_survey_json),
        ("bluetooth-report.txt", &pico.bluetooth_report_text),
        ("bluetooth-report.json", &pico.bluetooth_report_json),
    ];
    for (name, text) in optional {
        if !text.is_empty() {
            add_redacted(zip, &format!("{base}/{name}"), text)?;
        }
    }

    let uid = &pico.manifest.uid;
    let packets_path = format!("{base}/usb-packets.txt");
    let packets_text = &pico.usb_packets_text;
    add_redacted(zip, &packets_path, packets_text)?;

    let summary_json = serde_json::to_string_pretty(&summarize_text(packets_text))?;
    add_redacted(
        zip,
        &format!("{base}/usb-packets-summary.json"),
        &summary_json,
    )?;
    let records_jsonl = records_jsonl_for_text(uid, &packets_path, packets_text)?;
    add_redacted(zip, &format!("{base}/usb-packets.jsonl"), &records_jsonl)?;
    let control_transfers = control_transfers_text_for_text(uid, &packets_path, packets_text);
    add_redacted(
        zip,
        &format!("{base}/usb-control-transfers.txt"),
        &control_transfers,
    )?;
    let hid_reports = hid_reports_text_for_text(uid, &packets_path, packets_text);
    add_redacted(zip, &format!("{base}/usb-hid-reports.txt"), &hid_reports)?;
    let packet_timeline = packet_timeline_text_for_text(uid, &packets_path, packets_text);
    add_redacted(
        zip,
        &format!("{base}/usb-packet-timeline.txt"),
        &packet_timeline,
    )?;
    let enumeration_analysis = enumeration_analysis_text_for_text(uid, &packets_path, packets_text);
    add_redacted(
        zip,
        &format!("{base}/usb-enumeration-analysis.txt"),
        &enumeration_analysis,
    )
}

fn add_packet_outputs(zip: &mut BundleZip, contents: &BundleZipContents<'_>) -> Result<()> {
    let packets = contents.packets;
    add_redacted(
        zip,
        "usb-packets.txt",
        &aggregate_usb_packets(
            contents.per_pico_captures,
            contents.retained_debug_packet_logs,
        ),
    )?;
    add_redacted(zip, "usb-packets-summary.json", &packets.summary_json)?;
    add_redacted(zip, "usb-packets.jsonl", &packets.records_jsonl)?;
    add_redacted(
        zip,
        "usb-control-transfers.txt",
        &packets.control_transfers_text,
    )?;
    add_redacted(zip, "usb-hid-reports.txt", &packets.hid_reports_text)?;
    add_redacted(zip, "usb-packet-timeline.txt", &packets.timeline_text)?;
    add_redacted(
        zip,
        "usb-enumeration-analysis.txt",
        &packets.enumeration_analysis_text,
    )?;
    add_redacted(
        zip,
        "debug-capture-verdict.txt",
        &packets.debug_capture_verdict,
    )?;
    add_redacted(
        zip,
        "debug-capture-evidence.json",
        &packets.debug_capture_evidence_json,
    )?;

    for log in contents.retained_debug_packet_logs {
        add_redacted(zip, &format!("debug-packets/{}", log.name), &log.text)?;
    }
    Ok(())
}

fn add_host_files(zip: &mut BundleZip, contents: &BundleZipContents<'_>) -> Result<()> {
    if let Some(text) = contents.cache_current.as_ref() {
        add_redacted(zip, "diagnostics/pico-state-current.json", text)?;
    }
    if let Some(text) = contents.cache_history.as_ref() {
        add_redacted(zip, "diagnostics/pico-state-history.jsonl", text)?;
    }

    for snapshot in contents.host_snapshots {
        add_redacted(zip, snapshot.manifest.path.as_str(), &snapshot.text)?;
    }

    add_redacted(zip, "system-info.txt", contents.system_info)
}

fn add_usb_device_files(zip: &mut BundleZip, contents: &BundleZipContents<'_>) -> Result<()> {
    zip.start_file("usb-devices.txt", file_options())?;
    if let Some((text, method)) = contents.usb_devices.as_ref() {
        zip.write_all(format!("# capture method: {method}\n\n").as_bytes())?;
        zip.write_all(redact_bundle_text(text).as_bytes())?;
    } else {
        zip.write_all(
            b"(USB device enumeration unavailable: pnputil is missing AND the serialport \
              fallback returned an error. Run `pnputil /enum-devices /class USB /connected` \
              manually and attach the output.)",
        )?;
    }

    zip.start_file("usb-events.txt", file_options())?;
    if let Some(text) = contents.usb_events.as_ref() {
        zip.write_all(b"# Windows event log entries from the last 15 minutes\n")?;
        zip.write_all(b"# filtered to USB / usbhub / usbser / Kernel-PNP providers\n\n")?;
        zip.write_all(redact_bundle_text(text).as_bytes())?;
    } else {
        zip.write_all(
            b"(Get-WinEvent returned no output -- either no recent USB events were \
              recorded, the Windows PowerShell event log cmdlet timed out, or the \
              capture script returned an error. This is not necessarily a problem; \
              uneventful enumeration leaves no trace.)",
        )?;
    }
    Ok(())
}

fn add_crash_files(zip: &mut BundleZip) -> Result<()> {
    let Ok(crash_dir) = config::crash_dir() else {
        return Ok(());
    };
    if !crash_dir.is_dir() {
        return Ok(());
    }
    let entries = match std::fs::read_dir(&crash_dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                "bundle: could not enumerate crash dir {}: {e}",
                crash_dir.display()
            );
            return Ok(());
        }
    };
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
                let text = String::from_utf8_lossy(&bytes);
                add_redacted(zip, &format!("crashes/{}", name.to_string_lossy()), &text)?;
            }
            Err(e) => {
                tracing::debug!("bundle: could not read crash file {}: {e}", p.display(),);
            }
        }
    }
    Ok(())
}

fn add_state_journal(zip: &mut BundleZip) -> Result<()> {
    if let Some(jp) = journal::path()
        && jp.is_file()
    {
        match std::fs::read(&jp) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                add_redacted(zip, "state-journal.log", &text)?;
            }
            Err(e) => {
                tracing::debug!("bundle: could not read state journal: {e}");
            }
        }
    }
    Ok(())
}
