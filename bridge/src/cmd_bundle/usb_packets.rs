//! USB packet extraction, counters, and aggregate packet evidence.

use std::fmt::Write as _;
use std::time::Duration;

use crate::debug_packets;

use super::{PicoBundleCapture, RetainedDebugPacketLog};

pub(super) fn aggregate_initial_usb_capture_text(captures: &[PicoBundleCapture]) -> String {
    let mut out = String::from("# Aggregate initial USB capture evidence\n\n");
    let mut count = 0usize;
    for capture in captures {
        if capture.initial_usb_capture_text.is_empty() {
            continue;
        }
        count += 1;
        let _ = writeln!(
            out,
            "## {} path={}/initial-usb-capture.txt",
            capture.manifest.uid, capture.manifest.path
        );
        out.push_str(&capture.initial_usb_capture_text);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    if count == 0 {
        out.push_str("No live Pico initial USB capture was present.\n");
    }
    out
}
pub(super) fn usb_packets_text_from_diag(uid: &str, diag_text: &str) -> String {
    let lines = diag_text
        .lines()
        .filter_map(|line| usb_packet_line_index(line).map(|idx| line[idx..].to_string()))
        .collect::<Vec<_>>();
    usb_packets_text_from_lines(uid, &lines, None)
}

pub(super) fn usb_packets_text_from_debug_snapshot(
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
    let event_lines = lines
        .iter()
        .filter(|line| line.starts_with("usb-event "))
        .count();
    let harvest = debug_packets::HarvestOkRecord {
        duration_ms,
        snapshot: snapshot.clone(),
        packet_lines: lines.len(),
        raw_packet_lines,
        stats_lines,
        event_lines,
        new_lines: lines.len(),
    };
    let harvest_line = debug_packets::harvest_ok_line(&harvest, lines.len());
    usb_packets_text_from_lines(uid, &lines, Some(harvest_line.as_str()))
}

pub(super) fn usb_packets_text_from_lines(
    uid: &str,
    lines: &[String],
    harvest_line: Option<&str>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Raw USB packet dump extracted from firmware diagnostics"
    );
    let _ = writeln!(out, "# uid={uid}");
    let _ = writeln!(
        out,
        "# These lines are present when debug input mode, bundle USB capture, or the normal-persona boot snapshot is active."
    );
    let _ = writeln!(out);
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    if lines.is_empty() {
        let _ = writeln!(
            out,
            "No usb-packet, usb-event, or usb-packet-stats lines were present in this diagnostic source."
        );
    }
    if let Some(line) = harvest_line {
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub(super) fn count_usb_packet_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("usb-packet "))
        .count()
}

pub(super) fn count_usb_packet_stats_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("usb-packet-stats "))
        .count()
}

pub(super) fn count_usb_packet_event_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("usb-event "))
        .count()
}

pub(super) fn count_usb_packet_harvest_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("# harvest "))
        .count()
}

pub(super) fn usb_packet_line_index(line: &str) -> Option<usize> {
    line.find("usb-packet ")
        .or_else(|| line.find("usb-packet-stats "))
        .or_else(|| line.find("usb-event "))
}

pub(super) fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(super) fn count_retained_debug_packet_lines(logs: &[RetainedDebugPacketLog]) -> usize {
    logs.iter()
        .flat_map(|log| log.text.lines())
        .filter(|line| line.starts_with("usb-packet "))
        .count()
}

pub(super) fn aggregate_usb_packets(
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
) -> String {
    let mut out = String::from("# Aggregate USB packet capture evidence\n\n");
    let mut raw_total = 0usize;
    let mut stats_total = 0usize;
    let mut event_total = 0usize;
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
                } else if line.starts_with("usb-event ") {
                    event_total += 1;
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
                    } else if line.starts_with("usb-event ") {
                        event_total += 1;
                    } else if line.starts_with("# harvest ") {
                        harvest_total += 1;
                    }
                }
            }
            out.push('\n');
        }
    }
    if diagnostic_total == 0 {
        out.push_str("No USB packet, lifecycle event, stat, or harvest lines were captured in this bundle.\n");
    } else if raw_total == 0 {
        let mut kinds = Vec::new();
        if stats_total > 0 {
            kinds.push("packet stats");
        }
        if event_total > 0 {
            kinds.push("USB lifecycle events");
        }
        if harvest_total > 0 {
            kinds.push("harvest records");
        }
        if !kinds.is_empty() {
            let _ = writeln!(
                out,
                "No raw USB packet payload lines were captured, but {} were present.",
                kinds.join(", ")
            );
        }
    }
    out
}

fn is_usb_packet_diagnostic_line(line: &str) -> bool {
    line.starts_with("usb-packet ")
        || line.starts_with("usb-packet-stats ")
        || line.starts_with("usb-event ")
        || line.starts_with("# harvest ")
}
