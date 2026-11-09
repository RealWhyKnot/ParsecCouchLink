mod adapter_survey;
mod bluetooth_report;
mod build;
mod capture;
mod collect;
mod debug_capture;
mod flags;
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

use anyhow::Result;
use chrono::Local;

#[cfg(test)]
use adapter_survey::{
    AdapterSurveyAttempt, AdapterSurveyRawCapture, AdapterSurveyReport, adapter_survey_candidates,
    adapter_survey_report_json, adapter_survey_text, build_adapter_survey_report,
};
use adapter_survey::{
    adapter_connection_json, adapter_connection_report, adapter_connection_text,
    adapter_survey_bundle_json, aggregate_adapter_survey_text,
};
use bluetooth_report::{aggregate_bluetooth_report_text, bluetooth_report_bundle_json};
#[cfg(test)]
use bluetooth_report::{
    bluetooth_usb_packets_stub, build_bluetooth_report, format_bluetooth_report_json,
    format_bluetooth_report_text,
};
use build::{BundleSummary, build_bundle};
#[cfg(test)]
use capture::sanitize_path_component;
use capture::{CaptureLog, PicoBundleCapture, RetainedDebugPacketLog, UsbDiagBundle};
use capture::{capture_per_pico, capture_usb_diag_text, collect_retained_debug_packet_logs};
use collect::{collect_crash_file_names, collect_setup_transcript_names};
use debug_capture::{
    debug_capture_evidence_report_json, debug_capture_overall_status, debug_capture_verdict_text,
};
use host_snapshot::capture_host_snapshots;
use manifest::{ManifestHostSnapshot, ManifestInput, ManifestPicoCapture, build_manifest};
use pico_diag::{DiagOutcome, capture_pico_diag};
use sysinfo::{build_system_info, run_doctor_silently};
use usb_enum::{
    PicoEnumState, capture_usb_devices, capture_windows_usb_events, classify_pico_enum,
};
use usb_packet_summary::{
    UsbPacketSummarySource, control_transfers_text_for_sources,
    enumeration_analysis_text_for_sources, hid_reports_text_for_sources,
    packet_timeline_text_for_sources, records_jsonl_for_sources, summarize_sources,
};
use usb_packets::{aggregate_initial_usb_capture_text, count_retained_debug_packet_lines};
#[cfg(test)]
use usb_packets::{
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, usb_packets_text_from_debug_snapshot, usb_packets_text_from_diag,
};
use zip_writer::{BundleZipContents, write_bundle_zip};

pub(crate) async fn run(output: Option<PathBuf>) -> Result<()> {
    let out_path = output.unwrap_or_else(|| {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        PathBuf::from(format!("couchlink-bundle-{stamp}.zip"))
    });
    let summary = build_bundle(out_path).await?;

    let issue_url = crate::support::issue_url();
    print_capture_lines(&summary);
    print_packet_and_file_lines(&summary);

    outln!();
    outln!("Wi-Fi credentials, local addresses, and profile paths are redacted. Safe to share.");
    outln!();
    outln!("Report this bundle at: {issue_url}");
    offer_issue_page(&issue_url).await;
    Ok(())
}

fn print_capture_lines(summary: &BundleSummary) {
    outln!("Wrote {}", summary.zip_path.display());
    outln!("  manifest.json + doctor.txt + bridge logs");
    if summary.pico_diag_captured {
        if let Some(src) = summary.pico_diag_source.as_deref() {
            outln!("  pico-diag.txt: captured via {src}");
        } else {
            outln!("  pico-diag.txt: captured");
        }
    } else {
        outln!(
            "  pico-diag.txt: not captured ({}) -- see the file for details",
            summary.pico_diag_outcome
        );
    }
    if summary.usb_diag_captured {
        outln!(
            "  usb-diag.txt: captured for {} Pico board(s)",
            summary.usb_diag_target_count
        );
    } else {
        outln!("  usb-diag.txt: not captured -- see the file for details");
    }
    outln!(
        "  adapter-survey.txt: included for {} Pico capture(s)",
        summary.per_pico_capture_count
    );
    if summary.bluetooth_report_count > 0 {
        outln!(
            "  bluetooth-report.txt: captured for {} Bluetooth Pico board(s)",
            summary.bluetooth_report_count
        );
    } else {
        outln!("  bluetooth-report.txt: no Bluetooth-mode Pico captured");
    }
    if summary.adapter_connection_warning {
        outln!();
        outln!("Adapter connection warning:");
        outln!(
            "  No USB host enumeration traffic was observed from a live Pico. This bundle will not contain the adapter diagnostics needed to prove console-adapter support."
        );
        outln!(
            "  Plug the Pico into the console adapter and console USB host you want it to work on, then run couchlink bundle again."
        );
        outln!(
            "  If the adapter only handshakes once, power-cycle or physically replug the console-side adapter path before running bundle."
        );
    } else {
        outln!(
            "  adapter-connection.txt: {}",
            summary.adapter_connection_status
        );
    }
}

fn print_packet_and_file_lines(summary: &BundleSummary) {
    let total_packet_count = summary.usb_packet_dump_count + summary.retained_debug_packet_count;
    if total_packet_count > 0 {
        outln!(
            "  usb-packets.txt: captured {} raw USB packet(s)",
            total_packet_count
        );
    } else {
        outln!("  usb-packets.txt: no raw packets captured");
    }
    outln!(
        "  debug-capture-verdict.txt: {}",
        summary.debug_capture_status
    );
    if summary.retained_debug_packet_log_count > 0 {
        outln!(
            "  debug-packets/: {} retained packet log(s)",
            summary.retained_debug_packet_log_count
        );
    }
    if summary.crash_file_count == 0 {
        outln!("  crashes/: none");
    } else {
        outln!("  crashes/: {} files", summary.crash_file_count);
    }
    if summary.setup_transcript_count == 0 {
        outln!("  setup transcripts: none");
    } else {
        outln!(
            "  setup transcripts: {} files",
            summary.setup_transcript_count
        );
    }
}

async fn offer_issue_page(issue_url: &str) {
    if console::Term::stdout().is_term() {
        let url = issue_url.to_string();
        let open_it = tokio::task::spawn_blocking(move || {
            dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Open the issues page in your browser now?")
                .default(false)
                .interact()
        })
        .await
        .ok()
        .and_then(std::result::Result::ok)
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
}

#[cfg(test)]
mod tests;
