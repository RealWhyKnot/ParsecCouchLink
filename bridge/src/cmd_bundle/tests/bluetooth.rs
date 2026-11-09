use super::{
    bluetooth_cdc_status, bluetooth_pico_state, bluetooth_target, bluetooth_usb_diag,
    bt_report_input,
};
use crate::cmd_bundle::{
    build_bluetooth_report, format_bluetooth_report_json, format_bluetooth_report_text,
};
use crate::{cdc, protocol};

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
    assert!(report.radio.started);
    assert!(report.radio.connected);
    assert_eq!(report.bt_report_send_count, 3);
    assert!(report.pc_input.pc_input_observed);
    assert_eq!(report.pc_input.pc_input_evidence, "pico_state_input");
    assert_eq!(report.bt_receiver_contact, "hid_receiver_contact_seen");
    assert!(!report.bt_status_cdc_captured.is_captured());
    assert_eq!(report.usb_mounted, Some(true));
    assert_eq!(report.usb_device_desc_count, Some(2));
    assert_eq!(report.relevant_diag_lines.len(), 3);

    let text = format_bluetooth_report_text(&report);
    assert!(text.contains("expected_connection=pc_usb_input_bluetooth_output"));
    assert!(text.contains("usb_transport=cdc_framed_controller_state"));
    assert!(text.contains("bt_receiver_contact=hid_receiver_contact_seen"));
    assert!(text.contains("- pc_input_observed=true"));
    assert!(text.contains("- pc_input_evidence=pico_state_input"));
    assert!(text.contains("bt_cdc_input="));
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
    assert_eq!(value["pc_input_observed"], true);
    assert_eq!(value["pc_input_evidence"], "pico_state_input");
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
    status.user_confirmation_request_count = 2;
    status.user_confirmation_response_count = 2;
    status.authentication_complete_count = 1;
    status.last_security_event_ms = 700;
    status.bt_cdc_state_count = 4;
    status.bt_cdc_heartbeat_count = 12;
    status.bt_cdc_last_frame_ms = 950;
    status.bt_cdc_last_state_ms = 940;
    status.bt_cdc_last_heartbeat_ms = 950;
    status.bt_cdc_last_seq = 22;
    status.bt_cdc_last_command = cdc::CMD_BT_HEARTBEAT;
    status.bt_cdc_last_flags = protocol::FLAG_PARSEC_CONNECTED;

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
    assert!(report.bt_status_cdc_captured.is_captured());
    assert_eq!(report.bt_receiver_contact, "hid_receiver_contact_seen");
    assert!(report.radio.connected);
    assert_eq!(report.bt_report_send_count, 5);
    assert!(report.pc_input.pc_input_observed);
    assert_eq!(report.pc_input.pc_input_evidence, "bt_status_cdc_state");
    assert_eq!(report.bt_cdc_input_status, "state_frames");
    assert_eq!(
        report.bt_reported_local_name.as_deref(),
        Some("Xbox Wireless Controller")
    );
    assert_eq!(report.bt_get_report_count, Some(2));
    assert_eq!(report.bt_set_report_count, Some(1));
    assert_eq!(report.bt_out_report_count, Some(1));
    assert_eq!(report.bt_security_event_source, "cdc_status");
    assert_eq!(report.bt_user_confirmation_request_count, Some(2));
    assert_eq!(report.bt_cdc_state_count, Some(4));
    assert_eq!(report.bt_cdc_heartbeat_count, Some(12));

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
    assert!(text.contains("bt_security="));
    assert!(text.contains("- security_event_source=cdc_status"));
    assert!(text.contains("- user_confirmation_request_count=2"));
    assert!(text.contains("bt_cdc_input="));
    assert!(text.contains("- status=state_frames"));
    assert!(text.contains("- state_count=4"));
    assert!(text.contains("- heartbeat_count=12"));
    assert!(text.contains("- last_command=0x0D"));
}

#[test]
fn bluetooth_report_statuses_before_contact() {
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
}

#[test]
fn bluetooth_report_flags_pairing_contact() {
    let target = bluetooth_target(protocol::Persona::BluetoothXbox);
    let pairing_contact = build_bluetooth_report(
        "02E22DA9",
        "picos/02E22DA9",
        &target,
        bt_report_input(
            Some(&bluetooth_pico_state(protocol::BT_HID_STATUS_STARTED, 0)),
            None,
            None,
            "bt_hid: init target=bluetooth-xbox\nbt_hid: user_confirmation_request\n",
        ),
    );
    assert_eq!(pairing_contact.status, "waiting_for_receiver");
    assert_eq!(
        pairing_contact.bt_receiver_contact,
        "pairing_security_contact_no_hid_open"
    );
    assert_eq!(pairing_contact.bt_security_event_source, "diag_log");
    assert!(
        pairing_contact
            .next_steps
            .iter()
            .any(|step| step.contains("BlueRetro"))
    );
    let text = format_bluetooth_report_text(&pairing_contact);
    assert!(text.contains("bt_security="));
    assert!(text.contains("- security_event_source=diag_log"));
    assert!(text.contains("pairing/security"));
    let value: serde_json::Value =
        serde_json::from_str(&format_bluetooth_report_json(&pairing_contact)).unwrap();
    assert_eq!(
        value["bt_receiver_contact"],
        "pairing_security_contact_no_hid_open"
    );
    assert_eq!(value["bt_security_event_source"], "diag_log");
}
