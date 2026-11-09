use super::{
    AdapterConnectionPico, AdapterConnectionReport, AdapterHostResponse, AdapterSurveyReport,
    attempt_has_usb_host_traffic, format_string_list_or_none,
};
use crate::cmd_bundle::PicoBundleCapture;
use crate::cmd_bundle::usb_packets::count_usb_packet_lines;
use anyhow::Result;
use std::fmt::Write as _;

pub(super) fn adapter_connection_pico(
    capture: &PicoBundleCapture,
    report: &AdapterSurveyReport,
) -> AdapterConnectionPico {
    let attempts = report.attempts.len();
    let accepted = report.attempts.iter().any(|attempt| attempt.accepted);
    let host_traffic_seen = report.attempts.iter().any(attempt_has_usb_host_traffic);
    let descriptor_or_report_rejected = report
        .attempts
        .iter()
        .any(|attempt| attempt.verdict == "descriptor_or_report_rejected");
    let usb_diag_missing = report
        .attempts
        .iter()
        .any(|attempt| !attempt.usb_diag_captured.is_captured());
    let device_desc_total = report
        .attempts
        .iter()
        .map(|attempt| u64::from(attempt.device_desc_count))
        .sum();
    let config_desc_total = report
        .attempts
        .iter()
        .map(|attempt| u64::from(attempt.config_desc_count))
        .sum();
    let mount_total = report
        .attempts
        .iter()
        .map(|attempt| u64::from(attempt.mount_count))
        .sum();
    let raw_packet_lines = count_usb_packet_lines(&capture.usb_packets_text);
    let status = if accepted {
        "adapter_accepted"
    } else if descriptor_or_report_rejected {
        "descriptor_or_report_rejected"
    } else if host_traffic_seen {
        "usb_host_traffic_seen"
    } else if usb_diag_missing {
        "usb_diag_incomplete"
    } else {
        "no_usb_host_traffic"
    };
    AdapterConnectionPico {
        uid: capture.manifest.uid.clone(),
        path: capture.manifest.path.clone(),
        live: capture.manifest.live,
        status,
        warning: status == "no_usb_host_traffic",
        attempts,
        coverage_status: report.coverage_status.to_string(),
        stop_reason: report.stop_reason.to_string(),
        restore_status: report.restore_status.clone(),
        attempted_personas: report.attempted_personas.clone(),
        missing_adapter_personas: report.missing_adapter_personas.clone(),
        failed_usb_diag_personas: report.failed_usb_diag_personas.clone(),
        host: AdapterHostResponse {
            accepted,
            host_traffic_seen,
            descriptor_or_report_rejected,
        },
        usb_diag_missing,
        device_desc_total,
        config_desc_total,
        mount_total,
        raw_packet_lines,
    }
}

pub(in crate::cmd_bundle) fn adapter_connection_report(
    captures: &[PicoBundleCapture],
) -> AdapterConnectionReport {
    let live_pico_count = captures
        .iter()
        .filter(|capture| capture.manifest.live)
        .count();
    let per_pico: Vec<AdapterConnectionPico> = captures
        .iter()
        .filter_map(|capture| {
            let report = capture.adapter_survey_report.as_ref()?;
            Some(adapter_connection_pico(capture, report))
        })
        .collect();

    let surveyed_live_pico_count = per_pico.len();
    let no_usb_host_pico_count = per_pico
        .iter()
        .filter(|pico| pico.status == "no_usb_host_traffic")
        .count();
    let host_traffic_pico_count = per_pico
        .iter()
        .filter(|pico| pico.host.host_traffic_seen)
        .count();
    let accepted_pico_count = per_pico.iter().filter(|pico| pico.host.accepted).count();
    let descriptor_or_report_rejected_pico_count = per_pico
        .iter()
        .filter(|pico| pico.host.descriptor_or_report_rejected)
        .count();

    let status = if surveyed_live_pico_count == 0 {
        "not_checked"
    } else if accepted_pico_count > 0 {
        "adapter_accepted"
    } else if descriptor_or_report_rejected_pico_count > 0 {
        "descriptor_or_report_rejected"
    } else if host_traffic_pico_count > 0 {
        "usb_host_traffic_seen"
    } else if no_usb_host_pico_count == surveyed_live_pico_count {
        "no_usb_host_traffic"
    } else {
        "usb_diag_incomplete"
    };
    let warning = status == "no_usb_host_traffic";
    AdapterConnectionReport {
        artifact_schema_version: 1,
        status,
        warning,
        surveyed_live_pico_count,
        live_pico_count,
        no_usb_host_pico_count,
        host_traffic_pico_count,
        accepted_pico_count,
        descriptor_or_report_rejected_pico_count,
        per_pico,
        next_steps: adapter_connection_next_steps(status),
        notes: vec![
            "This verdict is based on live Pico USB counters captured during bundle.",
            "It does not infer physical cabling when no live Pico survey was captured.",
            "device_desc_count=0 means the Pico did not observe USB host enumeration traffic.",
            "Run bundle with the Pico connected to the console adapter and console USB host you want to support.",
        ],
    }
}

pub(in crate::cmd_bundle) fn adapter_connection_next_steps(status: &str) -> Vec<&'static str> {
    match status {
        "no_usb_host_traffic" => vec![
            "Confirm adapter-survey.txt lists PS3, generic HID, PS4, keyboard, XInput, Xbox One, and Maple attempts. If any are missing, use bundle-capture.txt to find the failed switch or USB diagnostic step.",
            "If every attempted persona reports device_desc_count=0, the Pico did not observe the console adapter as a USB host during the survey window.",
            "Plug the Pico into the console adapter and console USB host you want it to work on, then run couchlink bundle again.",
            "If the adapter only handshakes once, power-cycle or physically replug the console-side adapter path before running bundle.",
            "A bundle taken with no USB host traffic cannot prove whether PS3, generic HID, PS4, keyboard, XInput, Xbox One, or Maple personas work with the adapter.",
        ],
        "descriptor_or_report_rejected" => vec![
            "Keep the full bundle; descriptor traffic was seen, so adapter firmware or report-shape work can use this evidence.",
            "Review initial-usb-capture.txt and usb-enumeration-analysis.txt before changing descriptors.",
        ],
        "adapter_accepted" => vec![
            "Use adapter-survey.txt for the accepted persona and score.",
            "Use usb-enumeration-analysis.txt if runtime input still fails after configuration.",
        ],
        "not_checked" => vec![
            "No live Pico adapter survey was captured, so this bundle cannot determine console-adapter connection state.",
        ],
        _ => vec!["Check bundle-capture.txt for USB diagnostic failures before repeating the run."],
    }
}

pub(in crate::cmd_bundle) fn adapter_connection_text(report: &AdapterConnectionReport) -> String {
    let mut out = String::from("Adapter connection verdict\n\n");
    let _ = writeln!(out, "status={}", report.status);
    let _ = writeln!(out, "warning={}", report.warning);
    let _ = writeln!(out, "live_pico_count={}", report.live_pico_count);
    let _ = writeln!(
        out,
        "surveyed_live_pico_count={}",
        report.surveyed_live_pico_count
    );
    let _ = writeln!(
        out,
        "no_usb_host_pico_count={}",
        report.no_usb_host_pico_count
    );
    let _ = writeln!(
        out,
        "host_traffic_pico_count={}",
        report.host_traffic_pico_count
    );
    let _ = writeln!(out, "accepted_pico_count={}", report.accepted_pico_count);
    let _ = writeln!(
        out,
        "descriptor_or_report_rejected_pico_count={}",
        report.descriptor_or_report_rejected_pico_count
    );
    let _ = writeln!(out);
    if report.warning {
        out.push_str("warning_text=No USB host enumeration traffic was observed from a live Pico. This bundle will not contain the adapter diagnostics needed to prove console-adapter support.\n\n");
    }
    out.push_str("next_steps=\n");
    for step in &report.next_steps {
        let _ = writeln!(out, "- {step}");
    }
    let _ = writeln!(out);
    out.push_str("per_pico=\n");
    if report.per_pico.is_empty() {
        out.push_str("- none\n");
    } else {
        for pico in &report.per_pico {
            let _ = writeln!(
                out,
                "- uid={} status={} warning={} attempts={} coverage_status={} stop_reason={} restore_status={} attempted_personas={} missing_adapter_personas={} failed_usb_diag_personas={} accepted={} host_traffic_seen={} descriptor_or_report_rejected={} usb_diag_missing={} device_desc_total={} config_desc_total={} mount_total={} raw_packet_lines={} path={}",
                pico.uid,
                pico.status,
                pico.warning,
                pico.attempts,
                pico.coverage_status,
                pico.stop_reason,
                pico.restore_status,
                format_string_list_or_none(&pico.attempted_personas),
                format_string_list_or_none(&pico.missing_adapter_personas),
                format_string_list_or_none(&pico.failed_usb_diag_personas),
                pico.host.accepted,
                pico.host.host_traffic_seen,
                pico.host.descriptor_or_report_rejected,
                pico.usb_diag_missing,
                pico.device_desc_total,
                pico.config_desc_total,
                pico.mount_total,
                pico.raw_packet_lines,
                pico.path
            );
        }
    }
    out
}

pub(in crate::cmd_bundle) fn adapter_connection_json(
    report: &AdapterConnectionReport,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}
