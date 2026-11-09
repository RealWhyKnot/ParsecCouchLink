mod adapter_survey;
mod bluetooth;
mod bluetooth_link;
mod debug_capture;
mod fixtures;

use super::usb_packets::aggregate_usb_packets;
use super::{
    RetainedDebugPacketLog, count_usb_packet_event_lines, count_usb_packet_harvest_lines,
    count_usb_packet_lines, count_usb_packet_stats_lines, sanitize_path_component,
    usb_packets_text_from_debug_snapshot, usb_packets_text_from_diag,
};
use fixtures::{
    bluetooth_cdc_status, bluetooth_pico_state, bluetooth_target, bluetooth_usb_diag,
    bt_report_input, pico_capture,
};

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
