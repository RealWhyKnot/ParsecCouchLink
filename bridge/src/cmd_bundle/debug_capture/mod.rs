mod evidence;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::usb_packet_summary::{UsbPacketBundleSummary, UsbPacketSummary, summarize_text};
use super::{PicoBundleCapture, RetainedDebugPacketLog};
pub(super) use evidence::debug_capture_evidence_report_json;

pub(super) fn debug_capture_verdict_text(
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
    summary: &UsbPacketBundleSummary,
) -> String {
    let status = debug_capture_overall_status(summary, captures, retained_logs);
    let evidence_grade = debug_capture_evidence_grade(summary);
    let capture_quality = debug_capture_quality(summary);
    let (gate, gate_reason) = debug_capture_gate(summary);
    let debug_persona_captures = captures
        .iter()
        .filter(|capture| state_json_persona(&capture.state_json).as_deref() == Some("debug"))
        .count();
    let mut out = String::from("Debug input packet capture verdict\n\n");
    out.push_str(
        "scope=debug_xinput_only; use adapter-survey.txt for persona-specific adapter acceptance.\n",
    );
    let _ = writeln!(out, "overall_status={status}");
    let _ = writeln!(out, "evidence_grade={evidence_grade}");
    let _ = writeln!(out, "capture_quality={capture_quality}");
    let _ = writeln!(out, "adapter_reverse_engineering_gate={gate}");
    let _ = writeln!(out, "gate_reason={gate_reason}");
    write_capture_health(&mut out, summary);
    write_traffic_counts(&mut out, summary);

    let _ = writeln!(out, "debug_persona_captures={debug_persona_captures}");
    let _ = writeln!(
        out,
        "harvest_statuses={}",
        format_count_map(&summary.aggregate.harvest_statuses)
    );
    let _ = writeln!(out, "retained_debug_packet_logs={}", retained_logs.len());
    let _ = writeln!(out, "per_pico_captures={}", captures.len());
    let _ = writeln!(out);
    write_minimum_evidence(&mut out);

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
    write_meaning(&mut out, status);
    write_next_steps(&mut out, summary);
    write_per_pico(&mut out, captures);
    write_retained_logs(&mut out, retained_logs);
    out
}

fn write_capture_health(out: &mut String, summary: &UsbPacketBundleSummary) {
    let _ = writeln!(out, "raw_packet_lines={}", summary.aggregate.packet_lines);
    let _ = writeln!(out, "packet_stats_lines={}", summary.aggregate.stats_lines);
    let _ = writeln!(out, "usb_event_lines={}", summary.aggregate.event_lines);
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
    let _ = writeln!(
        out,
        "truncated_packet_lines={}",
        summary.aggregate.truncated_packet_lines
    );
    let _ = writeln!(
        out,
        "max_packet_truncated_bytes={}",
        summary.aggregate.max_packet_truncated_bytes.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "max_stats_truncated_packets={}",
        summary.aggregate.max_stats_truncated_packets.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "max_stats_truncated_bytes={}",
        summary.aggregate.max_stats_truncated_bytes.unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "max_harvest_lost_bytes={}",
        summary.aggregate.max_harvest_lost_bytes.unwrap_or(0)
    );
}

fn write_traffic_counts(out: &mut String, summary: &UsbPacketBundleSummary) {
    let endpoint_in_lines = debug_summary_direction_count(summary, "in");
    let endpoint_out_lines = debug_summary_direction_count(summary, "out");
    let setup_lines = debug_summary_direction_count(summary, "setup");
    let control_in_lines = debug_summary_direction_count(summary, "control-in");
    let hid_report_lines = summary.aggregate.hid_report_lines;
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
    let _ = writeln!(
        out,
        "usb_events={}",
        format_count_map(&summary.aggregate.events)
    );
    let _ = writeln!(
        out,
        "setup_requests={}",
        format_count_map(&summary.aggregate.setup_requests)
    );
    let _ = writeln!(
        out,
        "setup_descriptor_requests={}",
        format_count_map(&summary.aggregate.setup_descriptor_requests)
    );
    let _ = writeln!(
        out,
        "setup_known_requests={}",
        format_count_map(&summary.aggregate.setup_known_requests)
    );
    let _ = writeln!(
        out,
        "control_payload_kinds={}",
        format_count_map(&summary.aggregate.control_payload_kinds)
    );
    let _ = writeln!(
        out,
        "control_descriptor_replies={}",
        format_count_map(&summary.aggregate.control_descriptor_replies)
    );
    let _ = writeln!(
        out,
        "control_payload_summaries={}",
        format_count_map(&summary.aggregate.control_payload_summaries)
    );
}

fn write_minimum_evidence(out: &mut String) {
    out.push_str("minimum_evidence=\n");
    out.push_str("- raw_packet_lines > 0 is required before this bundle is enough for adapter reverse engineering.\n");
    out.push_str("- capture_quality=lossless_observed means bundled packet payloads and harvest chunks did not report truncation or ring loss.\n");
    out.push_str("- setup_requests and control_payload_summaries show whether enumeration, descriptor fetches, class/HID probes, or known vendor probes were observed.\n");
    out.push_str("- setup_lines or control_in_lines > 0 is preferred for enumeration/control-transfer failures.\n");
    out.push_str("- hid_report_lines > 0 is useful for HID-class adapter report analysis.\n");
    out.push_str("- usb_event_lines > 0 is useful for USB lifecycle failures such as mount, unmount, suspend, or resume without raw traffic.\n");
    out.push_str(
        "- endpoint_in_lines or endpoint_out_lines > 0 is preferred for runtime adapter traffic.\n",
    );
    out.push_str("- debug_persona_captures > 0 proves the Pico was in debug input mode when bundle captured current state; it does not prove PS3, generic HID, PS4, keyboard, Xbox One, or Maple acceptance.\n");
    let _ = writeln!(out);
}

fn write_meaning(out: &mut String, status: &str) {
    out.push_str("meaning=");
    match status {
        "raw_packets_captured" => out.push_str(
            "This bundle contains raw debug input USB packets. That evidence uses the debug/XInput USB shape; adapter-survey.txt records persona-specific acceptance.",
        ),
        "debug_stats_only" => out.push_str(
            "Debug input packet counters were present, but raw packet payload lines were not retained.",
        ),
        "harvest_attempted_no_packets" => out.push_str(
            "The bridge attempted retained debug packet harvests, but no raw packet payload lines were captured.",
        ),
        "debug_lifecycle_only" => out.push_str(
            "USB lifecycle events were captured, but raw packet payload lines were not retained.",
        ),
        "retained_logs_without_packet_lines" => out.push_str(
            "Retained debug packet log files exist, but they did not contain packet, lifecycle, stats, or harvest records.",
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
}

fn write_next_steps(out: &mut String, summary: &UsbPacketBundleSummary) {
    out.push_str("next_steps=\n");
    if summary.aggregate.packet_lines == 0 {
        out.push_str(
            "- Check adapter-survey.txt for persona-specific USB verdicts. If raw capture was needed, bundle attempted capture in that same persona.\n",
        );
        out.push_str(
            "- If bundle-capture.txt shows adapter_survey, usb_packet_harvest, or GET_LOG failures, use that row's reason as the failure point.\n",
        );
        out.push_str(
            "- If retained debug-packets/*.log files are present, include the whole bundle; those files contain prior stream-time harvest evidence.\n",
        );
    } else {
        out.push_str(
            "- Use usb-enumeration-analysis.txt for the enumeration/configuration checklist, usb-packets.jsonl for scripts, usb-packet-timeline.txt for timing, usb-hid-reports.txt for HID report traffic, and usb-packets-summary.json for sequence, direction, truncation, setup/control behavior, and harvest health totals.\n",
        );
        if debug_capture_has_loss(summary) {
            out.push_str(
                "- Reproduce with debug input mode running longer before bundle if capture_quality is lossy; truncated packet payloads or lost diag-ring bytes may hide adapter details.\n",
            );
        }
    }
    let _ = writeln!(out);
}

fn write_per_pico(out: &mut String, captures: &[PicoBundleCapture]) {
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
}

fn write_retained_logs(out: &mut String, retained_logs: &[RetainedDebugPacketLog]) {
    out.push_str("retained_logs=\n");
    if retained_logs.is_empty() {
        out.push_str("- none\n");
    } else {
        for log in retained_logs {
            let log_summary = summarize_text(&log.text);
            let _ = writeln!(
                out,
                "- path=debug-packets/{} raw_packets={} stats={} events={} event_names={} hid_reports={} max_gap_ms={} harvest_lines={} harvest_statuses={} chunk_statuses={} max_missing_chunks={} max_lost_bytes={} truncated_packets={} max_packet_truncated_bytes={} max_diag_bytes={}",
                log.name,
                log_summary.packet_lines,
                log_summary.stats_lines,
                log_summary.event_lines,
                format_count_map(&log_summary.events),
                log_summary.hid_report_lines,
                log_summary.max_inter_packet_gap_ms.unwrap_or(0),
                log_summary.harvest_lines,
                format_count_map(&log_summary.harvest_statuses),
                format_count_map(&log_summary.harvest_chunk_statuses),
                log_summary.max_harvest_missing_chunks.unwrap_or(0),
                log_summary.max_harvest_lost_bytes.unwrap_or(0),
                log_summary.truncated_packet_lines,
                log_summary.max_packet_truncated_bytes.unwrap_or(0),
                log_summary.max_harvest_diag_bytes.unwrap_or(0)
            );
        }
    }
}

pub(super) fn debug_capture_evidence_grade(summary: &UsbPacketBundleSummary) -> &'static str {
    if summary.aggregate.packet_lines > 0
        && (debug_summary_direction_count(summary, "setup") > 0
            || debug_summary_direction_count(summary, "control-in") > 0)
        && (debug_summary_direction_count(summary, "in") > 0
            || debug_summary_direction_count(summary, "out") > 0)
    {
        "complete"
    } else if summary.aggregate.packet_lines > 0 {
        "usable_raw_packets"
    } else if summary.aggregate.stats_lines > 0
        || summary.aggregate.event_lines > 0
        || summary.aggregate.harvest_lines > 0
    {
        "partial_no_payloads"
    } else {
        "missing"
    }
}

pub(super) fn debug_capture_gate(summary: &UsbPacketBundleSummary) -> (&'static str, &'static str) {
    if summary.aggregate.packet_lines > 0 {
        if debug_capture_has_loss(summary) {
            (
                "pass",
                "raw debug input packet payload lines are present, but capture is lossy",
            )
        } else {
            ("pass", "raw debug input packet payload lines are present")
        }
    } else {
        ("fail", "raw debug input packet payload lines are missing")
    }
}

pub(super) fn debug_capture_missing_evidence_lines(
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
        && summary.aggregate.event_lines == 0
        && summary.aggregate.harvest_lines == 0
        && retained_logs.is_empty()
    {
        lines.push("retained host harvest logs proving stream-time capture ran");
    }
    if debug_capture_has_loss(summary) {
        lines.push("lossless packet payload and harvest capture");
    }
    if captures.is_empty() && retained_logs.is_empty() {
        lines.push("live, cached, or retained Pico evidence");
    }
    if lines.is_empty() {
        lines.push("none");
    }
    lines
}

pub(super) fn debug_summary_direction_count(
    summary: &UsbPacketBundleSummary,
    direction: &str,
) -> u64 {
    summary
        .aggregate
        .directions
        .get(direction)
        .copied()
        .unwrap_or(0)
}

pub(super) fn debug_capture_quality(summary: &UsbPacketBundleSummary) -> &'static str {
    if summary.aggregate.packet_lines == 0 {
        "no_packet_payloads"
    } else if debug_capture_has_loss(summary) {
        "lossy"
    } else {
        "lossless_observed"
    }
}

pub(super) fn debug_capture_has_loss(summary: &UsbPacketBundleSummary) -> bool {
    debug_capture_summary_has_loss(&summary.aggregate)
}

pub(super) fn debug_capture_summary_has_loss(summary: &UsbPacketSummary) -> bool {
    summary.truncated_packet_lines > 0
        || summary.max_packet_truncated_bytes.unwrap_or(0) > 0
        || summary.max_stats_truncated_packets.unwrap_or(0) > 0
        || summary.max_stats_truncated_bytes.unwrap_or(0) > 0
        || summary.max_harvest_lost_bytes.unwrap_or(0) > 0
        || summary.max_harvest_missing_chunks.unwrap_or(0) > 0
}

pub(super) fn debug_capture_overall_status(
    summary: &UsbPacketBundleSummary,
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
) -> &'static str {
    if summary.aggregate.packet_lines > 0 {
        "raw_packets_captured"
    } else if summary.aggregate.stats_lines > 0 {
        "debug_stats_only"
    } else if summary.aggregate.event_lines > 0 {
        "debug_lifecycle_only"
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

pub(super) fn state_json_persona(state_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(state_json).ok()?;
    value
        .get("persona")?
        .as_str()
        .map(std::string::ToString::to_string)
}

pub(super) fn format_count_map(map: &BTreeMap<String, u64>) -> String {
    if map.is_empty() {
        return "none".to_string();
    }
    map.iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect::<Vec<_>>()
        .join(",")
}
