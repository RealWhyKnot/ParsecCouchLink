use super::{
    BluetoothReport, write_bt_acl_l2cap, write_bt_reconnect, write_bt_security,
    write_diag_lines_steps_and_notes, write_pc_usb_input,
};
use std::fmt::Write as _;

pub(in crate::cmd_bundle) fn format_bluetooth_report_text(report: &BluetoothReport) -> String {
    let mut out = String::from("Bluetooth output report\n\n");
    let _ = writeln!(out, "uid={}", report.uid);
    let _ = writeln!(out, "path={}", report.path);
    let _ = writeln!(out, "peer={}", report.peer.as_deref().unwrap_or("none"));
    let _ = writeln!(out, "persona={}", report.persona);
    let _ = writeln!(out, "target_label={}", report.target_label);
    let _ = writeln!(out, "advertised_name={}", report.advertised_name);
    let _ = writeln!(
        out,
        "expected_connection={}",
        report.plan.expected_connection
    );
    let _ = writeln!(out, "usb_input_required={}", report.plan.usb_input_required);
    let _ = writeln!(out, "usb_transport={}", report.plan.usb_transport);
    let _ = writeln!(out, "bluetooth_output={}", report.plan.bluetooth_output);
    let _ = writeln!(out, "status={}", report.status);
    let _ = writeln!(out, "warning={}", report.warning);
    let _ = writeln!(
        out,
        "pico_state_captured={}",
        report.pico_state_captured.is_captured()
    );
    let _ = writeln!(
        out,
        "usb_diag_captured={}",
        report.usb_diag_captured.is_captured()
    );
    let _ = writeln!(
        out,
        "bt_status_cdc_captured={}",
        report.bt_status_cdc_captured.is_captured()
    );
    if let Some(error) = &report.bt_status_cdc_error {
        let _ = writeln!(out, "bt_status_cdc_error={error}");
    }
    let _ = writeln!(
        out,
        "bt_status_version={}",
        format_option_u8(report.bt_status_version)
    );
    let _ = writeln!(
        out,
        "bt_decoded_status_version={}",
        format_option_u8(report.bt_decoded_status_version)
    );
    let _ = writeln!(
        out,
        "bt_status_newer_than_host={}",
        report.bt_status_newer_than_host
    );
    let _ = writeln!(
        out,
        "pico_diag_captured={}",
        report.pico_diag_captured.is_captured()
    );
    let _ = writeln!(out, "bt_receiver_contact={}", report.bt_receiver_contact);
    let _ = writeln!(out);
    write_bluetooth_state(&mut out, report);
    write_bt_cdc_input(&mut out, report);
    write_bt_control_plane(&mut out, report);
    write_bt_last_reports(&mut out, report);
    write_bt_security(&mut out, report);
    write_bt_reconnect(&mut out, report);
    write_bt_acl_l2cap(&mut out, report);
    write_pc_usb_input(&mut out, report);
    write_diag_lines_steps_and_notes(&mut out, report);
    out
}

pub(super) fn write_bluetooth_state(out: &mut String, report: &BluetoothReport) {
    out.push_str("bluetooth_state=\n");
    let _ = writeln!(out, "- bt_flags=0x{:02X}", report.radio.flags);
    let _ = writeln!(out, "- bt_started={}", report.radio.started);
    let _ = writeln!(out, "- bt_connected={}", report.radio.connected);
    let _ = writeln!(out, "- bt_send_requested={}", report.radio.send_requested);
    let _ = writeln!(out, "- bt_target={}", report.bt_target);
    let _ = writeln!(out, "- bt_last_status=0x{:02X}", report.bt_last_status);
    let _ = writeln!(out, "- bt_report_len={}", report.bt_report_len);
    let _ = writeln!(out, "- bt_cid={}", report.bt_cid);
    let _ = writeln!(out, "- bt_init_count={}", report.bt_init_count);
    let _ = writeln!(out, "- bt_ready_count={}", report.bt_ready_count);
    let _ = writeln!(out, "- bt_open_count={}", report.bt_open_count);
    let _ = writeln!(out, "- bt_close_count={}", report.bt_close_count);
    let _ = writeln!(out, "- bt_can_send_count={}", report.bt_can_send_count);
    let _ = writeln!(
        out,
        "- bt_report_build_count={}",
        report.bt_report_build_count
    );
    let _ = writeln!(
        out,
        "- bt_report_send_count={}",
        report.bt_report_send_count
    );
    let _ = writeln!(
        out,
        "- bt_send_request_count={}",
        report.bt_send_request_count
    );
    let _ = writeln!(out, "- bt_last_event_ms={}", report.bt_last_event_ms);
    let _ = writeln!(out, "- bt_last_send_ms={}", report.bt_last_send_ms);
    let _ = writeln!(
        out,
        "- pc_input_observed={}",
        report.pc_input.pc_input_observed
    );
    let _ = writeln!(
        out,
        "- pc_input_evidence={}",
        report.pc_input.pc_input_evidence
    );
    let _ = writeln!(out);
}

pub(super) fn write_bt_cdc_input(out: &mut String, report: &BluetoothReport) {
    out.push_str("bt_cdc_input=\n");
    let _ = writeln!(out, "- status={}", report.bt_cdc_input_status);
    let _ = writeln!(
        out,
        "- state_count={}",
        format_option_u32(report.bt_cdc_state_count)
    );
    let _ = writeln!(
        out,
        "- heartbeat_count={}",
        format_option_u32(report.bt_cdc_heartbeat_count)
    );
    let _ = writeln!(
        out,
        "- bad_length_count={}",
        format_option_u32(report.bt_cdc_bad_length_count)
    );
    let _ = writeln!(
        out,
        "- rejected_count={}",
        format_option_u32(report.bt_cdc_rejected_count)
    );
    let _ = writeln!(
        out,
        "- last_frame_ms={}",
        format_option_u32(report.bt_cdc_last_frame_ms)
    );
    let _ = writeln!(
        out,
        "- last_state_ms={}",
        format_option_u32(report.bt_cdc_last_state_ms)
    );
    let _ = writeln!(
        out,
        "- last_heartbeat_ms={}",
        format_option_u32(report.bt_cdc_last_heartbeat_ms)
    );
    let _ = writeln!(
        out,
        "- last_seq={}",
        format_option_u8(report.bt_cdc_last_seq)
    );
    let _ = writeln!(
        out,
        "- last_command={}",
        format_option_hex_u8(report.bt_cdc_last_command)
    );
    let _ = writeln!(
        out,
        "- last_flags={}",
        format_option_hex_u8(report.bt_cdc_last_flags)
    );
    let _ = writeln!(out);
}

pub(super) fn write_bt_control_plane(out: &mut String, report: &BluetoothReport) {
    out.push_str("bt_control_plane=\n");
    let _ = writeln!(
        out,
        "- reported_local_name={}",
        report
            .bt_reported_local_name
            .as_deref()
            .unwrap_or("not_captured")
    );
    let _ = writeln!(
        out,
        "- get_report_count={}",
        format_option_u32(report.bt_get_report_count)
    );
    let _ = writeln!(
        out,
        "- get_report_success_count={}",
        format_option_u32(report.bt_get_report_success_count)
    );
    let _ = writeln!(
        out,
        "- get_report_unsupported_count={}",
        format_option_u32(report.bt_get_report_unsupported_count)
    );
    let _ = writeln!(
        out,
        "- set_report_count={}",
        format_option_u32(report.bt_set_report_count)
    );
    let _ = writeln!(
        out,
        "- set_report_accepted_count={}",
        format_option_u32(report.bt_set_report_accepted_count)
    );
    let _ = writeln!(
        out,
        "- set_report_unsupported_count={}",
        format_option_u32(report.bt_set_report_unsupported_count)
    );
    let _ = writeln!(
        out,
        "- out_report_count={}",
        format_option_u32(report.bt_out_report_count)
    );
    let _ = writeln!(
        out,
        "- out_report_accepted_count={}",
        format_option_u32(report.bt_out_report_accepted_count)
    );
    let _ = writeln!(
        out,
        "- out_report_unsupported_count={}",
        format_option_u32(report.bt_out_report_unsupported_count)
    );
}

pub(super) fn write_bt_last_reports(out: &mut String, report: &BluetoothReport) {
    let _ = writeln!(
        out,
        "- last_get_report_id={}",
        format_option_hex_u8(report.bt_last_get_report_id)
    );
    let _ = writeln!(
        out,
        "- last_get_report_type={}",
        format_option_hid_report_type(report.bt_last_get_report_type)
    );
    let _ = writeln!(
        out,
        "- last_get_report_len={}",
        format_option_u16(report.bt_last_get_report_len)
    );
    let _ = writeln!(
        out,
        "- last_set_report_id={}",
        format_option_hex_u8(report.bt_last_set_report_id)
    );
    let _ = writeln!(
        out,
        "- last_set_report_type={}",
        format_option_hid_report_type(report.bt_last_set_report_type)
    );
    let _ = writeln!(
        out,
        "- last_set_report_len={}",
        format_option_u16(report.bt_last_set_report_len)
    );
    let _ = writeln!(
        out,
        "- last_out_report_id={}",
        format_option_hex_u8(report.bt_last_out_report_id)
    );
    let _ = writeln!(
        out,
        "- last_out_report_type={}",
        format_option_hid_report_type(report.bt_last_out_report_type)
    );
    let _ = writeln!(
        out,
        "- last_out_report_len={}",
        format_option_u16(report.bt_last_out_report_len)
    );
    let _ = writeln!(out);
}

pub(in crate::cmd_bundle) fn format_option_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "not_captured".to_string(), |v| v.to_string())
}

pub(in crate::cmd_bundle) fn format_option_u32(value: Option<u32>) -> String {
    value.map_or_else(|| "not_captured".to_string(), |v| v.to_string())
}

pub(in crate::cmd_bundle) fn format_option_u16(value: Option<u16>) -> String {
    value.map_or_else(|| "not_captured".to_string(), |v| v.to_string())
}

pub(in crate::cmd_bundle) fn format_option_u8(value: Option<u8>) -> String {
    value.map_or_else(|| "not_captured".to_string(), |v| v.to_string())
}

pub(in crate::cmd_bundle) fn format_option_hex_u8(value: Option<u8>) -> String {
    value.map_or_else(|| "not_captured".to_string(), |v| format!("0x{v:02X}"))
}

pub(in crate::cmd_bundle) fn format_option_hex_u16(value: Option<u16>) -> String {
    value.map_or_else(|| "not_captured".to_string(), |v| format!("0x{v:04X}"))
}

pub(in crate::cmd_bundle) fn format_option_hid_report_type(value: Option<u8>) -> String {
    value
        .map_or("not_captured", hid_report_type_name)
        .to_string()
}

pub(super) fn hid_report_type_name(report_type: u8) -> &'static str {
    match report_type {
        1 => "input",
        2 => "output",
        3 => "feature",
        _ => "unknown",
    }
}
