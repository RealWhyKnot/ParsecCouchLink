use super::bluetooth_report::BluetoothReportInput;
use super::usb_packets::aggregate_usb_packets;
use super::{
    adapter_connection_json, adapter_connection_report, adapter_connection_text,
    adapter_survey_bundle_json, adapter_survey_candidates, adapter_survey_report_json,
    adapter_survey_text, aggregate_adapter_survey_text, aggregate_bluetooth_report_text,
    aggregate_initial_usb_capture_text, bluetooth_report_bundle_json, bluetooth_usb_packets_stub,
    build_adapter_survey_report, build_bluetooth_report, count_usb_packet_event_lines,
    count_usb_packet_harvest_lines, count_usb_packet_lines, count_usb_packet_stats_lines,
    debug_capture_evidence_report_json, debug_capture_overall_status, debug_capture_verdict_text,
    format_bluetooth_report_json, format_bluetooth_report_text, sanitize_path_component,
    usb_packets_text_from_debug_snapshot, usb_packets_text_from_diag, AdapterSurveyAttempt,
    AdapterSurveyRawCapture, PicoBundleCapture, RetainedDebugPacketLog,
};
use super::{summarize_sources, ManifestPicoCapture, UsbPacketSummarySource};
use crate::{cdc, protocol};

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
        bt_report_input(
            Some(&pico_state),
            None,
            Some(&usb_diag),
            "run: Bluetooth persona = bluetooth\nbt_hid: connected\ncdc: dispatching cmd=0x0C seq=9 payload=13 bytes\n",
        ),
    );

    assert_eq!(report.status, "reports_sent");
    assert!(!report.warning);
    assert_eq!(report.persona, "bluetooth");
    assert_eq!(report.target_label, "bluetooth");
    assert!(report.bt_started);
    assert!(report.bt_connected);
    assert_eq!(report.bt_report_send_count, 3);
    assert_eq!(report.bt_receiver_contact, "hid_receiver_contact_seen");
    assert!(!report.bt_status_cdc_captured);
    assert_eq!(report.usb_mounted, Some(true));
    assert_eq!(report.usb_device_desc_count, Some(2));
    assert_eq!(report.relevant_diag_lines.len(), 3);

    let text = format_bluetooth_report_text(&report);
    assert!(text.contains("expected_connection=pc_usb_input_bluetooth_output"));
    assert!(text.contains("usb_transport=cdc_framed_controller_state"));
    assert!(text.contains("bt_receiver_contact=hid_receiver_contact_seen"));
    assert!(text.contains("bt_control_plane="));
    assert!(text.contains("- get_report_count=not_captured"));
    assert!(text.contains("- bt_report_send_count=3"));
    assert!(text.contains("- device_desc_count=2"));

    let value: serde_json::Value =
        serde_json::from_str(&format_bluetooth_report_json(&report)).unwrap();
    assert_eq!(value["status"], "reports_sent");
    assert_eq!(value["usb_input_required"], true);
    assert_eq!(value["bt_connected"], true);
    assert_eq!(value["bt_status_cdc_captured"], false);
    assert_eq!(value["bt_receiver_contact"], "hid_receiver_contact_seen");
    assert_eq!(value["usb_transport"], "cdc_framed_controller_state");
}

#[test]
fn bluetooth_report_uses_cdc_status_for_receiver_control_plane() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let mut pico_state = bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0);
    pico_state.bt_target = 1;
    let mut status = bluetooth_cdc_status(
        cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED,
        5,
    );
    status.target = 1;
    status.local_name = "Xbox Wireless Controller".to_string();
    status.get_report_count = 2;
    status.get_report_success_count = 1;
    status.get_report_unsupported_count = 1;
    status.set_report_count = 1;
    status.set_report_accepted_count = 1;
    status.out_report_count = 1;
    status.out_report_accepted_count = 1;
    status.last_get_report_id = 0x01;
    status.last_get_report_type = 1;
    status.last_get_report_len = 16;
    status.last_set_report_id = 0x05;
    status.last_set_report_type = 2;
    status.last_set_report_len = 4;
    status.last_out_report_id = 0x03;
    status.last_out_report_type = 2;
    status.last_out_report_len = 3;

    let report = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&pico_state),
            Some(&status),
            Some(&bluetooth_usb_diag()),
            "bt_hid: connected\n",
        ),
    );

    assert_eq!(report.status, "reports_sent");
    assert!(report.bt_status_cdc_captured);
    assert_eq!(report.bt_receiver_contact, "hid_receiver_contact_seen");
    assert!(report.bt_connected);
    assert_eq!(report.bt_report_send_count, 5);
    assert_eq!(
        report.bt_reported_local_name.as_deref(),
        Some("Xbox Wireless Controller")
    );
    assert_eq!(report.bt_get_report_count, Some(2));
    assert_eq!(report.bt_set_report_count, Some(1));
    assert_eq!(report.bt_out_report_count, Some(1));

    let text = format_bluetooth_report_text(&report);
    assert!(text.contains("bt_status_cdc_captured=true"));
    assert!(text.contains("- reported_local_name=Xbox Wireless Controller"));
    assert!(text.contains("- get_report_count=2"));
    assert!(text.contains("- get_report_unsupported_count=1"));
    assert!(text.contains("- set_report_accepted_count=1"));
    assert!(text.contains("- out_report_accepted_count=1"));
    assert!(text.contains("- last_get_report_id=0x01"));
    assert!(text.contains("- last_get_report_type=input"));
    assert!(text.contains("- last_set_report_type=output"));
}

#[test]
fn bluetooth_report_statuses_are_actionable() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let missing = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(None, None, None, ""),
    );
    assert_eq!(missing.status, "pico_state_missing");
    assert!(missing.warning);

    let mut xbox_state = bluetooth_pico_state(0, 0);
    xbox_state.bt_target = 1;
    let not_started = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&xbox_state),
            None,
            None,
            "run: Bluetooth persona = bluetooth-xbox\n",
        ),
    );
    assert_eq!(not_started.status, "bluetooth_stack_not_started");
    assert_eq!(not_started.target_label, "bluetooth-xbox");

    let waiting = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0)),
            None,
            None,
            "bt_hid: init target=bluetooth-xbox\n",
        ),
    );
    assert_eq!(waiting.status, "waiting_for_receiver");
    assert_eq!(waiting.bt_receiver_contact, "discoverable_no_hid_contact");
    assert!(waiting.next_steps.iter().any(|step| step.contains("pair")));

    let connected_no_reports = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&bluetooth_pico_state(
                protocol::BT_HID_STATUS_STARTED | protocol::BT_HID_STATUS_CONNECTED,
                0,
            )),
            None,
            None,
            "bt_hid: connected\n",
        ),
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
        bt_report_input(
            Some(&ps_state),
            None,
            Some(&bluetooth_usb_diag()),
            "bt_hid: connected\n",
        ),
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
    assert!(text
        .contains("expected_adapter_personas=ps3,generic-hid,ps4,keyboard,xinput,xboxone,maple"));
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
    assert!(
        text.contains("attempted_personas=debug,ps3,generic-hid,ps4,keyboard,xinput,xboxone,maple")
    );
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
    let report = test_survey_report("02E22DA9", "xinput", "confirmed", Some("xinput"), attempts);
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

fn bt_report_input<'a>(
    pico_state: Option<&'a protocol::PicoStateDiag>,
    bt_status: Option<&'a cdc::BtStatus>,
    usb_diag: Option<&'a protocol::UsbDiag>,
    pico_diag_text: &'a str,
) -> BluetoothReportInput<'a> {
    BluetoothReportInput {
        pico_state,
        bt_status,
        bt_status_error: None,
        usb_diag,
        pico_diag_text,
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

fn bluetooth_cdc_status(flags: u8, report_send_count: u32) -> cdc::BtStatus {
    cdc::BtStatus {
        flags,
        target: 0,
        last_status: 0,
        report_len: 16,
        cid: 7,
        init_count: if flags & cdc::BT_STATUS_FLAG_STARTED != 0 {
            1
        } else {
            0
        },
        ready_count: if flags & cdc::BT_STATUS_FLAG_STARTED != 0 {
            1
        } else {
            0
        },
        open_count: if flags & cdc::BT_STATUS_FLAG_CONNECTED != 0 {
            1
        } else {
            0
        },
        close_count: 0,
        can_send_count: report_send_count,
        report_build_count: report_send_count,
        report_send_count,
        send_request_count: report_send_count,
        last_event_ms: 800,
        last_send_ms: if report_send_count > 0 { 900 } else { 0 },
        get_report_count: 0,
        get_report_success_count: 0,
        get_report_unsupported_count: 0,
        set_report_count: 0,
        set_report_accepted_count: 0,
        set_report_unsupported_count: 0,
        out_report_count: 0,
        out_report_accepted_count: 0,
        out_report_unsupported_count: 0,
        last_get_report_id: 0,
        last_get_report_type: 0,
        last_set_report_id: 0,
        last_set_report_type: 0,
        last_out_report_id: 0,
        last_out_report_type: 0,
        last_get_report_len: 0,
        last_set_report_len: 0,
        last_out_report_len: 0,
        local_name: String::new(),
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
