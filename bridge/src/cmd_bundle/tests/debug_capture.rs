use super::pico_capture;
use crate::cmd_bundle::{
    RetainedDebugPacketLog, debug_capture_evidence_report_json, debug_capture_overall_status,
    debug_capture_verdict_text,
};
use crate::cmd_bundle::{UsbPacketSummarySource, summarize_sources};

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
    assert!(
        value["missing_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "lossless packet payload and harvest capture")
    );
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
    assert!(
        value["missing_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "raw USB packet payload lines from debug input mode")
    );
    assert_eq!(
        value["retained_logs"][0]["path"],
        "debug-packets/usb-packets-20260615-214000-02E22DA9.log"
    );
    assert!(
        value["retained_logs"][0]["missing_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry == "raw USB packet payload lines from this source")
    );
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
