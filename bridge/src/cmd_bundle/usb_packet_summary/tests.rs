use super::enumeration::known_device_identity;
use super::*;

#[test]
fn summary_counts_packets_stats_and_sequence_gaps() {
    let text = "\
usb-packet seq=1 t=10 dir=out src=vendor len=3 captured=3 dropped=0 suppressed=0 reason=host-out data=010203
usb-packet seq=3 t=11 dir=in src=xinput len=20 captured=20 dropped=0 suppressed=2 reason=changed data=00
usb-packet seq=3 t=12 dir=in src=xinput len=20 captured=20 dropped=0 suppressed=0 reason=changed data=00
usb-packet seq=2 t=13 dir=setup src=vendor-control len=10 captured=8 truncated=2 dropped=4 suppressed=0 reason=control-setup data=C0
usb-event t=13 event=mount
usb-packet-stats t=14 total=64 in=2 out=1 setup=1 control_in=0 truncated_bytes=4 truncated_packets=1 idle_in_suppressed=9
";
    let summary = summarize_text(text);
    assert_eq!(summary.packet_lines, 4);
    assert_eq!(summary.event_lines, 1);
    assert_eq!(summary.stats_lines, 1);
    assert_eq!(summary.events["mount"], 1);
    assert_eq!(summary.directions["in"], 2);
    assert_eq!(summary.sources["xinput"], 2);
    assert_eq!(summary.reasons["changed"], 2);
    assert_eq!(summary.first_seq, Some(1));
    assert_eq!(summary.last_seq, Some(2));
    assert_eq!(summary.min_seq, Some(1));
    assert_eq!(summary.max_seq, Some(3));
    assert_eq!(summary.missing_sequence_numbers, 1);
    assert_eq!(summary.duplicate_sequence_numbers, 1);
    assert_eq!(summary.out_of_order_sequence_lines, 1);
    assert_eq!(summary.max_reported_packet_len, Some(20));
    assert_eq!(summary.max_captured_len, Some(20));
    assert_eq!(summary.truncated_packet_lines, 1);
    assert_eq!(summary.max_packet_truncated_bytes, Some(2));
    assert_eq!(summary.max_truncated_bytes, Some(4));
    assert_eq!(summary.max_suppressed_idle_reports, Some(2));
    assert_eq!(summary.last_stats_total_packets, Some(64));
    assert_eq!(summary.max_stats_truncated_packets, Some(1));
    assert_eq!(summary.max_stats_idle_in_suppressed, Some(9));
    assert_eq!(summary.stats_direction_max["setup"], 1);
    assert_eq!(summary.first_packet_t_ms, Some(10));
    assert_eq!(summary.last_packet_t_ms, Some(13));
    assert_eq!(summary.min_packet_t_ms, Some(10));
    assert_eq!(summary.max_packet_t_ms, Some(13));
    assert_eq!(summary.packet_time_span_ms, Some(3));
    assert_eq!(summary.max_inter_packet_gap_ms, Some(1));
    assert_eq!(summary.packet_time_regressions, 0);
    assert_eq!(summary.harvest_lines, 0);
}

#[test]
fn summary_counts_harvest_health() {
    let text = "\
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":3,\"expected_chunks\":4,\"missing_chunk_count\":1,\"duplicate_chunk_count\":2,\"got_last\":true,\"chunk_complete\":false,\"lost_bytes\":4,\"diag_bytes\":256,\"diag_lines\":12,\"packet_lines\":8,\"raw_packet_lines\":6,\"stats_lines\":2,\"event_lines\":1,\"new_lines\":2,\"duplicate_lines\":6,\"total_packet_lines\":12}
# harvest {\"at\":\"2026-06-15T22:30:01-05:00\",\"status\":\"error\",\"duration_ms\":1200,\"error\":\"no log chunks received\"}
# harvest not-json
";
    let summary = summarize_text(text);
    assert_eq!(summary.harvest_lines, 3);
    assert_eq!(summary.harvest_statuses["ok"], 1);
    assert_eq!(summary.harvest_statuses["error"], 1);
    assert_eq!(summary.harvest_statuses["malformed"], 1);
    assert_eq!(summary.max_harvest_duration_ms, Some(1200));
    assert_eq!(summary.max_harvest_lost_bytes, Some(4));
    assert_eq!(summary.max_harvest_chunk_count, Some(3));
    assert_eq!(summary.max_harvest_expected_chunks, Some(4));
    assert_eq!(summary.max_harvest_missing_chunks, Some(1));
    assert_eq!(summary.max_harvest_duplicate_chunks, Some(2));
    assert_eq!(summary.max_harvest_diag_bytes, Some(256));
    assert_eq!(summary.max_harvest_diag_lines, Some(12));
    assert_eq!(summary.max_harvest_packet_lines, Some(8));
    assert_eq!(summary.max_harvest_raw_packet_lines, Some(6));
    assert_eq!(summary.max_harvest_stats_lines, Some(2));
    assert_eq!(summary.max_harvest_event_lines, Some(1));
    assert_eq!(summary.max_harvest_new_lines, Some(2));
    assert_eq!(summary.max_harvest_duplicate_lines, Some(6));
    assert_eq!(summary.harvest_chunk_statuses["incomplete"], 1);
}

#[test]
fn bundle_summary_keeps_per_source_sequence_accounting() {
    let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: "usb-packet seq=1 dir=out src=vendor reason=host-out\nusb-packet seq=3 dir=out src=vendor reason=host-out\n",
        }];
    let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text: "usb-event t=22 event=mount\nusb-packet-stats total=64 out=2 truncated_bytes=0 truncated_packets=0 idle_in_suppressed=0\n",
        }];
    let summary = summarize_sources(&per_pico, &retained);
    assert_eq!(summary.artifact_schema_version, 8);
    assert_eq!(summary.aggregate.packet_lines, 2);
    assert_eq!(summary.aggregate.event_lines, 1);
    assert_eq!(summary.aggregate.stats_lines, 1);
    assert_eq!(summary.aggregate.events["mount"], 1);
    assert_eq!(summary.aggregate.missing_sequence_numbers, 1);
    assert_eq!(summary.per_pico[0].summary.missing_sequence_numbers, 1);
    assert_eq!(
        summary.retained_logs[0].summary.last_stats_total_packets,
        Some(64)
    );
}

#[test]
fn packet_timeline_keeps_packets_stats_harvest_and_deltas() {
    let text = "\
usb-packet seq=7 t=10 dir=out src=vendor len=3 captured=3 truncated=0 dropped=0 suppressed=0 reason=host-out data=010203
usb-packet seq=8 t=15 dir=setup src=vendor-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup data=C020
usb-event t=18 event=mount
usb-packet-stats t=40 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_complete\":true,\"packet_lines\":8,\"raw_packet_lines\":6,\"new_lines\":2}
usb-packet seq=9 t=12 dir=in src=xinput len=20 captured=20 dropped=0 suppressed=0 reason=changed data=00
";
    let out = packet_timeline_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    assert!(out.contains("# USB packet timeline"));
    assert!(out.contains("packet line=1 seq=7 t=10 dt_ms=- dir=out src=vendor"));
    assert!(out.contains("truncated=0 dropped=0 suppressed=0"));
    assert!(out.contains("packet line=2 seq=8 t=15 dt_ms=5 dir=setup"));
    assert!(
        out.contains("event line=3 t=18 dt_ms=3 event=mount src=- len=- bytes=- remote_wakeup=-")
    );
    assert!(out.contains(
            "stats line=4 t=40 dt_ms=22 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9"
        ));
    assert!(out.contains("harvest line=5 at=2026-06-15T22:30:00-05:00 status=ok duration_ms=14 chunk_complete=true packet_lines=8 raw_packet_lines=6 new_lines=2 error=-"));
    assert!(out.contains("packet line=6 seq=9 t=12 dt_ms=regression dir=in"));

    let summary = summarize_text(text);
    assert_eq!(summary.max_inter_packet_gap_ms, Some(5));
    assert_eq!(summary.packet_time_regressions, 1);
}

#[test]
fn packet_timeline_sources_omit_empty_sources() {
    let per_pico = [UsbPacketSummarySource {
        label: "02E22DA9".to_string(),
        path: "picos/02E22DA9/usb-packets.txt".to_string(),
        text: "not packet evidence\n",
    }];
    let retained = [UsbPacketSummarySource {
        label: "usb-packets.log".to_string(),
        path: "debug-packets/usb-packets.log".to_string(),
        text: "usb-packet seq=2 t=22 dir=out src=hid-output data=050607\n",
    }];
    let out = packet_timeline_text_for_sources(&per_pico, &retained);
    assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
    assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
    assert!(out.contains("packet line=1 seq=2 t=22"));
}

#[test]
fn enumeration_analysis_reports_descriptor_configuration_and_endpoint_phases() {
    let text = "\
usb-packet seq=1 t=10 dir=setup src=standard-control bm=0x80 req=0x06 value=0x0100 index=0x0000 wlen=18 len=8 captured=8 data=8006000100001200
usb-packet seq=2 t=11 dir=control-in src=desc-device len=18 captured=18 dropped=0 reason=control-reply data=12010002FFFFFF405E048E02140101020301
usb-packet seq=3 t=12 dir=setup src=standard-control bm=0x00 req=0x05 value=0x0005 index=0x0000 wlen=0 len=8 captured=8 data=0005050000000000
usb-packet seq=4 t=13 dir=setup src=standard-control bm=0x80 req=0x06 value=0x0200 index=0x0000 wlen=32 len=8 captured=8 data=8006000200002000
usb-packet seq=5 t=14 dir=control-in src=desc-config len=9 captured=9 dropped=0 reason=control-reply data=09022000010100A032
usb-packet seq=6 t=15 dir=setup src=standard-control bm=0x00 req=0x09 value=0x0001 index=0x0000 wlen=0 len=8 captured=8 data=0009010000000000
usb-packet seq=7 t=16 dir=setup src=vendor-control bm=0xC0 req=0x20 value=0x0000 index=0x0007 wlen=38 len=8 captured=8 data=C020000007002600
usb-packet seq=8 t=17 dir=control-in src=ms-os-20 len=38 captured=38 dropped=0 reason=control-reply data=0A000000000003062600
usb-packet seq=9 t=18 dir=out src=vendor len=3 captured=3 dropped=0 reason=host-out data=010203
usb-event t=19 event=mount
";
    let out =
        enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    assert!(out.contains("# USB enumeration analysis"));
    assert!(out.contains("verdict=endpoint_traffic_observed"));
    assert!(out.contains("packet_lines=9"));
    assert!(out.contains("event_lines=1"));
    assert!(out.contains("mount_events=1"));
    assert!(out.contains("events=mount:1"));
    assert!(out.contains("device_descriptor_request=yes"));
    assert!(out.contains("device_descriptor_reply=yes"));
    assert!(out.contains("device_vid_pid=0x045E:0x028E"));
    assert!(out.contains("device_identity=couchlink_xinput_maple_debug_shape"));
    assert!(out.contains("device_class=class=0xFF,subclass=0xFF,protocol=0xFF"));
    assert!(out.contains("device_bcd_usb=0x0200"));
    assert!(out.contains("device_bcd_device=0x0114"));
    assert!(out.contains("device_max_packet=64"));
    assert!(out.contains("device_configurations=1"));
    assert!(out.contains("configuration_descriptor_request=yes"));
    assert!(out.contains("configuration_descriptor_reply=yes"));
    assert!(out.contains("configuration_interfaces=1"));
    assert!(out.contains("set_address_request=yes"));
    assert!(out.contains("set_configuration_request=yes"));
    assert!(out.contains("known_vendor_requests=ms-os-20-descriptor-set:1"));
    assert!(out.contains("control_payload_replies="));
    assert!(out.contains("ms-os-20-descriptor-set:1"));
}

#[test]
fn enumeration_analysis_distinguishes_harvest_only_evidence() {
    let text = "# harvest {\"status\":\"ok\",\"duration_ms\":14,\"packet_lines\":0}\n";
    let out =
        enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    assert!(out.contains("verdict=harvest_or_stats_only_no_raw_packets"));
    assert!(out.contains("packet_lines=0"));
    assert!(out.contains("harvest_lines=1"));
    assert!(out.contains("device_descriptor_request=no"));
}

#[test]
fn enumeration_analysis_distinguishes_lifecycle_only_evidence() {
    let text = "usb-event t=22 event=mount\nusb-event t=24 event=suspend remote_wakeup=1\n";
    let out =
        enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    assert!(out.contains("verdict=mounted_no_raw_packets"));
    assert!(out.contains("packet_lines=0"));
    assert!(out.contains("event_lines=2"));
    assert!(out.contains("mount_events=1"));
    assert!(out.contains("suspend_events=1"));
    assert!(out.contains("first_host_out_events=0"));
    assert!(out.contains("first_in_accepted_events=0"));
    assert!(out.contains("events=mount:1;suspend:1"));
    assert!(out.contains("device_descriptor_request=no"));
}

#[test]
fn enumeration_analysis_distinguishes_runtime_events_without_packets() {
    let text = "\
usb-event t=30 event=first-in-accepted src=xinput bytes=20
usb-event t=31 event=first-host-out src=vendor len=3
";
    let out =
        enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    assert!(out.contains("verdict=runtime_usb_events_no_raw_packets"));
    assert!(out.contains("packet_lines=0"));
    assert!(out.contains("event_lines=2"));
    assert!(out.contains("first_host_out_events=1"));
    assert!(out.contains("first_in_accepted_events=1"));
    assert!(out.contains("events=first-host-out:1;first-in-accepted:1"));
}

#[test]
fn enumeration_analysis_sources_omit_empty_sources() {
    let per_pico = [UsbPacketSummarySource {
        label: "02E22DA9".to_string(),
        path: "picos/02E22DA9/usb-packets.txt".to_string(),
        text: "not packet evidence\n",
    }];
    let retained = [UsbPacketSummarySource {
        label: "usb-packets.log".to_string(),
        path: "debug-packets/usb-packets.log".to_string(),
        text: "usb-event t=22 event=mount\n",
    }];
    let out = enumeration_analysis_text_for_sources(&per_pico, &retained);
    assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
    assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
    assert!(out.contains("verdict=mounted_no_raw_packets"));
}

#[test]
fn known_device_identity_names_couchlink_usb_shapes() {
    assert_eq!(
        known_device_identity(0x2E8A, 0xCAF0, 0xEF, 0x02, 0x01),
        "couchlink_setup_cdc_winusb"
    );
    assert_eq!(
        known_device_identity(0x045E, 0x028E, 0xFF, 0xFF, 0xFF),
        "couchlink_xinput_maple_debug_shape"
    );
    assert_eq!(
        known_device_identity(0x2E8A, 0xCAF1, 0x00, 0x00, 0x00),
        "couchlink_keyboard_hid_boot_shape"
    );
    assert_eq!(
        known_device_identity(0x2E8A, 0xCAF2, 0x00, 0x00, 0x00),
        "couchlink_generic_hid_gamepad_shape"
    );
    assert_eq!(
        known_device_identity(0x054C, 0x0268, 0x00, 0x00, 0x00),
        "couchlink_ps3_hid_shape"
    );
    assert_eq!(
        known_device_identity(0x054C, 0x09CC, 0x00, 0x00, 0x00),
        "couchlink_ps4_hid_shape"
    );
    assert_eq!(
        known_device_identity(0x0E6F, 0x02A4, 0xFF, 0xFF, 0xFF),
        "couchlink_xboxone_xgip_shape"
    );
    assert_eq!(
        known_device_identity(0x1234, 0x5678, 0x00, 0x00, 0x00),
        "unknown_usb_device_identity"
    );
}

#[test]
fn control_transfer_text_keeps_setup_and_control_in_rows() {
    let text = "\
usb-packet seq=7 t=10 dir=out src=vendor len=3 captured=3 dropped=0 reason=host-out data=010203
usb-packet seq=8 t=11 dir=setup src=vendor-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 data=C020020104030040
usb-packet seq=9 t=12 dir=control-in src=desc-device len=18 captured=18 dropped=0 suppressed=0 reason=control-reply data=12010002
usb-packet seq=10 t=13 dir=setup src=standard-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0x80 req=0x06 value=0x0301 index=0x0409 wlen=255 data=800601030904FF00
usb-packet-stats t=20 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9
";
    let out = control_transfers_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    let summary = summarize_text(text);
    assert_eq!(summary.setup_directions["device_to_host"], 2);
    assert_eq!(summary.setup_types["vendor"], 1);
    assert_eq!(summary.setup_types["standard"], 1);
    assert_eq!(summary.setup_recipients["device"], 2);
    assert_eq!(summary.setup_requests["vendor_0x20"], 1);
    assert_eq!(summary.setup_requests["get_descriptor"], 1);
    assert_eq!(summary.setup_descriptor_requests["string"], 1);
    assert_eq!(summary.control_payload_kinds["usb_descriptor"], 1);
    assert_eq!(summary.control_descriptor_replies["device"], 1);
    assert_eq!(
        summary.control_payload_summaries["descriptor=device,captured_len=4"],
        1
    );
    assert!(out.contains("# source_label=02E22DA9"));
    assert!(out.contains("setup line=2 seq=8 t=11 src=vendor-control bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 len=8 captured=8 decode=device_to_host/vendor/device request=vendor_0x20 descriptor=- descriptor_index=- language_id=- known=- data=C020020104030040"));
    assert!(out.contains("control-in line=3 seq=9 t=12 src=desc-device reason=control-reply len=18 captured=18 dropped=0 payload_kind=usb_descriptor payload_descriptor=device payload_summary=descriptor=device,captured_len=4 data=12010002"));
    assert!(out.contains("setup line=4 seq=10 t=13 src=standard-control bm=0x80 req=0x06 value=0x0301 index=0x0409 wlen=255 len=8 captured=8 decode=device_to_host/standard/device request=get_descriptor descriptor=string descriptor_index=1 language_id=0x0409 known=- data=800601030904FF00"));
    assert!(!out.contains("host-out"));
    assert!(!out.contains("usb-packet-stats"));
}

#[test]
fn control_transfer_sources_omit_empty_sources() {
    let per_pico = [UsbPacketSummarySource {
        label: "02E22DA9".to_string(),
        path: "picos/02E22DA9/usb-packets.txt".to_string(),
        text: "usb-packet seq=1 dir=out src=vendor reason=host-out\n",
    }];
    let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text: "usb-packet seq=8 t=11 dir=setup src=vendor-control bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 len=8 captured=8 data=C020020104030040\n",
        }];
    let out = control_transfers_text_for_sources(&per_pico, &retained);
    assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
    assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
    assert!(out.contains("setup line=1 seq=8"));
}

#[test]
fn hid_report_summary_and_transcript_extract_report_metadata() {
    let text = "\
usb-packet seq=1 t=10 dir=setup src=hid-get-report len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0xA1 req=0x01 value=0x03EF index=0x0002 wlen=64 data=A101EF0302004000
usb-packet seq=2 t=11 dir=setup src=hid-set-report len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0x21 req=0x09 value=0x0201 index=0x0002 wlen=4 data=2109010202000400
usb-packet seq=3 t=12 dir=out src=hid-output len=3 captured=3 dropped=0 suppressed=0 reason=host-out report_id=0x01 report_type=2 data=050607
usb-packet seq=4 t=13 dir=out src=hid-feature len=2 captured=2 dropped=0 suppressed=0 reason=host-out report_id=0xEF report_type=3 data=AABB
";
    let summary = summarize_text(text);
    assert_eq!(summary.hid_report_lines, 4);
    assert_eq!(summary.hid_report_types["feature"], 2);
    assert_eq!(summary.hid_report_types["output"], 2);
    assert_eq!(summary.hid_report_ids["0xEF"], 2);
    assert_eq!(summary.hid_report_ids["0x01"], 2);

    let out = hid_reports_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    assert!(out.contains("# USB HID report transcript"));
    assert!(out
        .contains("request=hid_get_report report_id=0xEF report_type=3 report_type_name=feature"));
    assert!(
        out.contains("request=hid_set_report report_id=0x01 report_type=2 report_type_name=output")
    );
    assert!(out.contains(
        "dir=out src=hid-output request=- report_id=0x01 report_type=2 report_type_name=output"
    ));
    assert!(out.contains(
        "dir=out src=hid-feature request=- report_id=0xEF report_type=3 report_type_name=feature"
    ));
}

#[test]
fn hid_report_sources_omit_empty_sources() {
    let per_pico = [UsbPacketSummarySource {
        label: "02E22DA9".to_string(),
        path: "picos/02E22DA9/usb-packets.txt".to_string(),
        text: "usb-packet seq=1 dir=out src=vendor reason=host-out\n",
    }];
    let retained = [UsbPacketSummarySource {
        label: "usb-packets.log".to_string(),
        path: "debug-packets/usb-packets.log".to_string(),
        text: "usb-packet seq=2 dir=out src=hid-output report_id=0x01 report_type=2 data=050607\n",
    }];
    let out = hid_reports_text_for_sources(&per_pico, &retained);
    assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
    assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
    assert!(out.contains("hid-report line=1 seq=2"));
}

#[test]
fn setup_decode_names_known_vendor_requests() {
    let text = "\
usb-packet seq=1 t=10 dir=setup src=vendor-control bm=0xC0 req=0x20 value=0x0000 index=0x0007 wlen=38 len=8 captured=8 data=C020000007002600
usb-packet seq=2 t=11 dir=setup src=vendor-control bm=0xC1 req=0x01 value=0x0000 index=0x0002 wlen=16388 len=8 captured=8 data=C101000002000440
usb-packet seq=3 t=12 dir=control-in src=ms-os-20 len=38 captured=38 dropped=0 reason=control-reply data=0A000000000003062600
";
    let out = control_transfers_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
    let summary = summarize_text(text);
    assert_eq!(summary.setup_requests["ms-os-20-descriptor-set"], 1);
    assert_eq!(summary.setup_requests["couchlink-setup-diag-log"], 1);
    assert_eq!(summary.setup_known_requests["ms-os-20-descriptor-set"], 1);
    assert_eq!(summary.setup_known_requests["couchlink-setup-diag-log"], 1);
    assert_eq!(summary.control_payload_kinds["known_vendor_payload"], 1);
    assert_eq!(
        summary.control_payload_summaries["ms-os-20-descriptor-set"],
        1
    );
    assert!(out.contains("request=ms-os-20-descriptor-set"));
    assert!(out.contains("known=ms-os-20-descriptor-set"));
    assert!(out.contains("request=couchlink-setup-diag-log"));
    assert!(out.contains("known=couchlink-setup-diag-log"));
    assert!(out.contains("payload_kind=known_vendor_payload payload_descriptor=- payload_summary=ms-os-20-descriptor-set"));
}

#[test]
fn records_jsonl_normalizes_packet_and_stats_lines() {
    let text = "\
usb-packet seq=7 t=10 dir=control-in src=desc-device len=18 captured=18 truncated=0 dropped=0 suppressed=0 reason=control-reply data=12010002
usb-packet seq=8 t=11 dir=setup src=vendor-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 data=C020020104030040
usb-packet seq=9 t=12 dir=out src=hid-output len=3 captured=3 truncated=0 dropped=0 suppressed=0 reason=host-out report_id=0x01 report_type=2 data=050607
usb-event t=13 event=suspend remote_wakeup=1
usb-event t=14 event=first-in-accepted src=xinput bytes=20
usb-event t=15 event=first-host-out src=vendor len=3
usb-packet-stats t=20 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":3,\"expected_chunks\":3,\"missing_chunk_count\":0,\"duplicate_chunk_count\":1,\"got_last\":true,\"chunk_complete\":true,\"lost_bytes\":4,\"diag_bytes\":512,\"diag_lines\":20,\"packet_lines\":8,\"raw_packet_lines\":6,\"stats_lines\":2,\"event_lines\":3,\"new_lines\":2,\"duplicate_lines\":6,\"total_packet_lines\":12}
";
    let jsonl = records_jsonl_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text).unwrap();
    let records: Vec<serde_json::Value> = jsonl
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 8);
    assert_eq!(records[0]["kind"], "packet");
    assert_eq!(records[0]["source_label"], "02E22DA9");
    assert_eq!(records[0]["line_number"], 1);
    assert_eq!(records[0]["seq"], 7);
    assert_eq!(records[0]["direction"], "control-in");
    assert_eq!(records[0]["packet_truncated_bytes"], 0);
    assert_eq!(records[0]["control_payload_kind"], "usb_descriptor");
    assert_eq!(records[0]["control_descriptor_type"], "device");
    assert_eq!(
        records[0]["control_payload_summary"],
        "descriptor=device,captured_len=4"
    );
    assert_eq!(records[0]["data_hex"], "12010002");
    assert_eq!(records[1]["kind"], "packet");
    assert_eq!(records[1]["direction"], "setup");
    assert_eq!(records[1]["setup_bm_request_type"], 192);
    assert_eq!(records[1]["setup_request"], 32);
    assert_eq!(records[1]["setup_value"], 258);
    assert_eq!(records[1]["setup_index"], 772);
    assert_eq!(records[1]["setup_length"], 16384);
    assert_eq!(records[1]["setup_direction"], "device_to_host");
    assert_eq!(records[1]["setup_type"], "vendor");
    assert_eq!(records[1]["setup_recipient"], "device");
    assert_eq!(records[1]["setup_request_name"], "vendor_0x20");
    assert!(records[1]["setup_descriptor_type"].is_null());
    assert!(records[1]["setup_descriptor_index"].is_null());
    assert!(records[1]["setup_language_id"].is_null());
    assert!(records[1]["setup_known_request"].is_null());
    assert_eq!(records[1]["data_hex"], "C020020104030040");
    assert_eq!(records[2]["kind"], "packet");
    assert_eq!(records[2]["direction"], "out");
    assert_eq!(records[2]["hid_report_id"], 1);
    assert_eq!(records[2]["hid_report_type"], 2);
    assert_eq!(records[2]["hid_report_type_name"], "output");
    assert_eq!(records[3]["kind"], "event");
    assert_eq!(records[3]["t_ms"], 13);
    assert_eq!(records[3]["event"], "suspend");
    assert_eq!(records[3]["remote_wakeup"], 1);
    assert_eq!(records[4]["kind"], "event");
    assert_eq!(records[4]["event"], "first-in-accepted");
    assert_eq!(records[4]["source"], "xinput");
    assert!(records[4]["len"].is_null());
    assert_eq!(records[4]["bytes"], 20);
    assert_eq!(records[5]["kind"], "event");
    assert_eq!(records[5]["event"], "first-host-out");
    assert_eq!(records[5]["source"], "vendor");
    assert_eq!(records[5]["len"], 3);
    assert!(records[5]["bytes"].is_null());
    assert_eq!(records[6]["kind"], "stats");
    assert_eq!(records[6]["total"], 64);
    assert_eq!(records[6]["in"], 4);
    assert_eq!(records[6]["truncated_packets"], 1);
    assert_eq!(records[6]["idle_in_suppressed"], 9);
    assert_eq!(records[7]["kind"], "harvest");
    assert_eq!(records[7]["status"], "ok");
    assert_eq!(records[7]["duration_ms"], 14);
    assert_eq!(records[7]["chunk_count"], 3);
    assert_eq!(records[7]["expected_chunks"], 3);
    assert_eq!(records[7]["missing_chunk_count"], 0);
    assert_eq!(records[7]["duplicate_chunk_count"], 1);
    assert_eq!(records[7]["got_last"], true);
    assert_eq!(records[7]["chunk_complete"], true);
    assert_eq!(records[7]["lost_bytes"], 4);
    assert_eq!(records[7]["diag_bytes"], 512);
    assert_eq!(records[7]["diag_lines"], 20);
    assert_eq!(records[7]["packet_lines"], 8);
    assert_eq!(records[7]["raw_packet_lines"], 6);
    assert_eq!(records[7]["stats_lines"], 2);
    assert_eq!(records[7]["event_lines"], 3);
    assert_eq!(records[7]["new_lines"], 2);
    assert_eq!(records[7]["duplicate_lines"], 6);
    assert_eq!(records[7]["total_packet_lines"], 12);
}
