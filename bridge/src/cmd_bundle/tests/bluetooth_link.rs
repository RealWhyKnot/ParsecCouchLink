use super::{
    bluetooth_cdc_status, bluetooth_pico_state, bluetooth_target, bluetooth_usb_diag,
    bt_report_input, pico_capture,
};
use crate::cmd_bundle::{
    aggregate_bluetooth_report_text, bluetooth_report_bundle_json, bluetooth_usb_packets_stub,
    build_bluetooth_report, count_usb_packet_lines, format_bluetooth_report_json,
    format_bluetooth_report_text,
};
use crate::{cdc, protocol};

#[test]
fn bluetooth_report_tracks_reconnect_and_l2cap() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let reconnect_pending_status = reconnect_pending_status();
    let reconnect_pending = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0)),
            Some(&reconnect_pending_status),
            None,
            "bt_hid: reconnect scheduled reason=1 attempt=1/6 delay_ms=600\n",
        ),
    );
    assert_eq!(reconnect_pending.status, "waiting_for_receiver");
    assert_eq!(
        reconnect_pending.bt_receiver_contact,
        "hid_reconnect_pending"
    );
    assert_eq!(reconnect_pending.bt_reconnect_schedule_count, Some(1));
    let text = format_bluetooth_report_text(&reconnect_pending);
    assert!(text.contains("bt_reconnect="));
    assert!(text.contains("- reconnect_schedule_count=1"));
    assert!(
        reconnect_pending
            .next_steps
            .iter()
            .any(|step| step.contains("scheduled an active Classic HID reconnect"))
    );

    let mut reconnect_attempted_status = reconnect_pending_status.clone();
    reconnect_attempted_status.reconnect_state = 0x01;
    reconnect_attempted_status.reconnect_cycle_attempts = 2;
    reconnect_attempted_status.reconnect_attempt_count = 2;
    reconnect_attempted_status.reconnect_failed_count = 2;
    reconnect_attempted_status.last_reconnect_status = 0x04;
    reconnect_attempted_status.connection_complete_count = 2;
    reconnect_attempted_status.last_connection_complete_status = 0x04;
    let reconnect_attempted = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0)),
            Some(&reconnect_attempted_status),
            None,
            "bt_hid: reconnect attempt returned status=0x04 attempt=2/6\n",
        ),
    );
    assert_eq!(
        reconnect_attempted.bt_receiver_contact,
        "hid_reconnect_attempted_no_hid_open"
    );
    assert_eq!(reconnect_attempted.bt_connection_complete_count, Some(2));
    let text = format_bluetooth_report_text(&reconnect_attempted);
    assert!(text.contains("bt_acl_l2cap="));
    assert!(text.contains("- connection_complete_count=2"));
    let value: serde_json::Value =
        serde_json::from_str(&format_bluetooth_report_json(&reconnect_attempted)).unwrap();
    assert_eq!(
        value["bt_receiver_contact"],
        "hid_reconnect_attempted_no_hid_open"
    );
    assert_eq!(value["bt_reconnect_attempt_count"], 2);

    let mut l2cap_status = reconnect_pending_status;
    l2cap_status.reconnect_state = 0x01;
    l2cap_status.incoming_l2cap_connection_count = 1;
    l2cap_status.incoming_l2cap_hid_control_count = 1;
    l2cap_status.last_incoming_l2cap_psm = 0x0011;
    l2cap_status.last_incoming_l2cap_local_cid = 0x0040;
    let l2cap_contact = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0)),
            Some(&l2cap_status),
            None,
            "bt_hid: incoming_l2cap psm=0x0011 local_cid=0x0040\n",
        ),
    );
    assert_eq!(
        l2cap_contact.bt_receiver_contact,
        "hid_l2cap_incoming_no_hid_open"
    );
}

#[test]
fn bluetooth_report_connected_without_pc_input() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
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
    assert!(
        connected_no_reports
            .next_steps
            .iter()
            .any(|step| step.contains("source controller"))
    );

    let no_pc_input_state = no_pc_input_state();
    let no_pc_input_usb = no_pc_input_usb();
    let no_pc_input = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&no_pc_input_state),
            Some(&bluetooth_cdc_status(
                cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED,
                42,
            )),
            Some(&no_pc_input_usb),
            "bt_hid: connected\n",
        ),
    );
    assert_eq!(no_pc_input.status, "receiver_reports_sent_no_pc_input");
    assert!(no_pc_input.warning);
    assert!(!no_pc_input.pc_input.pc_input_observed);
    assert_eq!(no_pc_input.pc_input.pc_input_evidence, "none");
    assert_eq!(no_pc_input.bt_cdc_input_status, "no_cdc_frames");
    assert!(
        no_pc_input
            .next_steps
            .iter()
            .any(|step| step.contains("no PC controller input frames"))
    );
    let text = format_bluetooth_report_text(&no_pc_input);
    assert!(text.contains("status=receiver_reports_sent_no_pc_input"));
    assert!(text.contains("- pc_input_observed=false"));
    assert!(text.contains("- status=no_cdc_frames"));
    assert!(text.contains("- state_count=0"));
}

#[test]
fn bluetooth_report_legacy_status_without_pc_input() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let no_pc_input_state = no_pc_input_state();
    let no_pc_input_usb = no_pc_input_usb();
    let mut legacy_status = bluetooth_cdc_status(
        cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED,
        42,
    );
    legacy_status.status_version = cdc::BT_STATUS_V4_VERSION;
    legacy_status.decoded_status_version = cdc::BT_STATUS_V4_VERSION;
    let legacy_no_pc_input = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&no_pc_input_state),
            Some(&legacy_status),
            Some(&no_pc_input_usb),
            "bt_hid: connected\n",
        ),
    );
    assert_eq!(
        legacy_no_pc_input.status,
        "receiver_reports_sent_stream_not_active"
    );
    assert_eq!(
        legacy_no_pc_input.bt_cdc_input_status,
        "host_stream_not_active"
    );
    assert_eq!(
        legacy_no_pc_input.pc_input.pc_input_evidence,
        "legacy_stream_not_active"
    );
    assert_eq!(legacy_no_pc_input.bt_cdc_state_count, None);
    assert!(
        legacy_no_pc_input
            .next_steps
            .iter()
            .any(|step| step.contains("keep it running"))
    );
    let text = format_bluetooth_report_text(&legacy_no_pc_input);
    assert!(text.contains("status=receiver_reports_sent_stream_not_active"));
    assert!(text.contains("- status=host_stream_not_active"));
    assert!(text.contains("- state_count=not_captured"));

    let mut legacy_with_peer_usb = no_pc_input_usb;
    legacy_with_peer_usb.activity_flags = protocol::USB_DIAG_ACTIVITY_PEER;
    let legacy_source_missing = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&no_pc_input_state),
            Some(&legacy_status),
            Some(&legacy_with_peer_usb),
            "bt_hid: connected\n",
        ),
    );
    assert_eq!(
        legacy_source_missing.status,
        "receiver_reports_sent_source_never_connected"
    );
    assert_eq!(
        legacy_source_missing.pc_input.pc_input_evidence,
        "legacy_stream_source_disconnected"
    );
}

#[test]
fn bluetooth_report_cdc_source_states() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let no_pc_input_state = no_pc_input_state();
    let no_pc_input_usb = no_pc_input_usb();
    let mut source_missing_status = bluetooth_cdc_status(
        cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED,
        42,
    );
    source_missing_status.bt_cdc_heartbeat_count = 8;
    source_missing_status.bt_cdc_last_flags = 0;
    let source_missing = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&no_pc_input_state),
            Some(&source_missing_status),
            Some(&no_pc_input_usb),
            "bt_hid: connected\n",
        ),
    );
    assert_eq!(
        source_missing.status,
        "receiver_reports_sent_source_never_connected"
    );
    assert_eq!(source_missing.bt_cdc_input_status, "source_never_connected");
    assert_eq!(
        source_missing.pc_input.pc_input_evidence,
        "bt_status_cdc_heartbeat_source_disconnected"
    );
    assert!(
        source_missing
            .next_steps
            .iter()
            .any(|step| step.contains("source controller was not connected"))
    );

    let mut source_idle_status = bluetooth_cdc_status(
        cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED,
        42,
    );
    source_idle_status.bt_cdc_heartbeat_count = 8;
    source_idle_status.bt_cdc_last_flags = protocol::FLAG_PARSEC_CONNECTED;
    let source_idle = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&no_pc_input_state),
            Some(&source_idle_status),
            Some(&no_pc_input_usb),
            "bt_hid: connected\n",
        ),
    );
    assert_eq!(source_idle.status, "receiver_reports_sent_source_idle");
    assert_eq!(source_idle.bt_cdc_input_status, "source_idle");
    assert_eq!(
        source_idle.pc_input.pc_input_evidence,
        "bt_status_cdc_heartbeat"
    );
    assert!(
        source_idle
            .next_steps
            .iter()
            .any(|step| step.contains("no changed controller state"))
    );
}

#[test]
fn bluetooth_report_flags_cdc_input_errors() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let no_pc_input_state = no_pc_input_state();
    let no_pc_input_usb = no_pc_input_usb();
    let mut cdc_error_status = bluetooth_cdc_status(
        cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED,
        42,
    );
    cdc_error_status.bt_cdc_bad_length_count = 2;
    cdc_error_status.bt_cdc_rejected_count = 1;
    let cdc_errors = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&no_pc_input_state),
            Some(&cdc_error_status),
            Some(&no_pc_input_usb),
            "bt_hid: connected\n",
        ),
    );
    assert_eq!(cdc_errors.status, "receiver_reports_sent_cdc_input_errors");
    assert_eq!(cdc_errors.bt_cdc_input_status, "cdc_input_errors");
    assert_eq!(
        cdc_errors.pc_input.pc_input_evidence,
        "bt_status_cdc_input_errors"
    );
    assert_eq!(cdc_errors.bt_cdc_bad_length_count, Some(2));
    assert_eq!(cdc_errors.bt_cdc_rejected_count, Some(1));
    assert!(
        cdc_errors
            .next_steps
            .iter()
            .any(|step| step.contains("malformed or rejected"))
    );
}

pub(super) fn reconnect_pending_status() -> cdc::BtStatus {
    let mut reconnect_pending_status = bluetooth_cdc_status(cdc::BT_STATUS_FLAG_STARTED, 0);
    reconnect_pending_status.target = 1;
    reconnect_pending_status.local_name = "Xbox Wireless Controller".to_string();
    reconnect_pending_status.user_confirmation_request_count = 1;
    reconnect_pending_status.user_confirmation_response_count = 1;
    reconnect_pending_status.link_key_notification_count = 1;
    reconnect_pending_status.reconnect_state = 0x03;
    reconnect_pending_status.reconnect_cycle_attempts = 0;
    reconnect_pending_status.reconnect_schedule_count = 1;
    reconnect_pending_status
}

pub(super) fn no_pc_input_state() -> protocol::PicoStateDiag {
    let mut no_pc_input_state = bluetooth_pico_state(
        protocol::BT_HID_STATUS_STARTED | protocol::BT_HID_STATUS_CONNECTED,
        42,
    );
    no_pc_input_state.last_bridge_packet_ms = 0;
    no_pc_input_state.xinput_in_queued_count = 0;
    no_pc_input_state.xinput_in_sent_count = 0;
    no_pc_input_state.activity_flags = 0;
    no_pc_input_state.last_in_queued_ms = 0;
    no_pc_input_state.last_in_sent_ms = 0;
    no_pc_input_state
}

pub(super) fn no_pc_input_usb() -> protocol::UsbDiag {
    let mut no_pc_input_usb = bluetooth_usb_diag();
    no_pc_input_usb.last_bridge_packet_ms = 0;
    no_pc_input_usb.xinput_in_queued_count = 0;
    no_pc_input_usb.xinput_in_sent_count = 0;
    no_pc_input_usb.activity_flags = 0;
    no_pc_input_usb.last_in_queued_ms = 0;
    no_pc_input_usb.last_in_sent_ms = 0;
    no_pc_input_usb
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
