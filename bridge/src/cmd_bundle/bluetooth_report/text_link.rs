use super::BluetoothReport;
use super::text::{
    format_option_bool, format_option_hex_u8, format_option_hex_u16, format_option_u8,
    format_option_u32,
};
use std::fmt::Write as _;

pub(super) fn write_bt_security(out: &mut String, report: &BluetoothReport) {
    out.push_str("bt_security=\n");
    let _ = writeln!(
        out,
        "- security_event_source={}",
        report.bt_security_event_source
    );
    let _ = writeln!(
        out,
        "- pin_code_request_count={}",
        format_option_u32(report.bt_pin_code_request_count)
    );
    let _ = writeln!(
        out,
        "- pin_code_response_count={}",
        format_option_u32(report.bt_pin_code_response_count)
    );
    let _ = writeln!(
        out,
        "- user_confirmation_request_count={}",
        format_option_u32(report.bt_user_confirmation_request_count)
    );
    let _ = writeln!(
        out,
        "- user_confirmation_response_count={}",
        format_option_u32(report.bt_user_confirmation_response_count)
    );
    let _ = writeln!(
        out,
        "- simple_pairing_complete_count={}",
        format_option_u32(report.bt_simple_pairing_complete_count)
    );
    let _ = writeln!(
        out,
        "- authentication_complete_count={}",
        format_option_u32(report.bt_authentication_complete_count)
    );
    let _ = writeln!(
        out,
        "- link_key_notification_count={}",
        format_option_u32(report.bt_link_key_notification_count)
    );
    let _ = writeln!(
        out,
        "- encryption_change_count={}",
        format_option_u32(report.bt_encryption_change_count)
    );
    let _ = writeln!(
        out,
        "- disconnection_complete_count={}",
        format_option_u32(report.bt_disconnection_complete_count)
    );
    let _ = writeln!(
        out,
        "- hid_open_failed_count={}",
        format_option_u32(report.bt_hid_open_failed_count)
    );
    let _ = writeln!(
        out,
        "- last_security_event_ms={}",
        format_option_u32(report.bt_last_security_event_ms)
    );
    let _ = writeln!(
        out,
        "- last_simple_pairing_status={}",
        format_option_hex_u8(report.bt_last_simple_pairing_status)
    );
    let _ = writeln!(
        out,
        "- last_authentication_status={}",
        format_option_hex_u8(report.bt_last_authentication_status)
    );
    let _ = writeln!(
        out,
        "- last_encryption_status={}",
        format_option_hex_u8(report.bt_last_encryption_status)
    );
    let _ = writeln!(
        out,
        "- last_encryption_enabled={}",
        format_option_u8(report.bt_last_encryption_enabled)
    );
    let _ = writeln!(
        out,
        "- last_disconnection_reason={}",
        format_option_hex_u8(report.bt_last_disconnection_reason)
    );
    let _ = writeln!(
        out,
        "- last_hid_open_status={}",
        format_option_hex_u8(report.bt_last_hid_open_status)
    );
    let _ = writeln!(out);
}

pub(super) fn write_bt_reconnect(out: &mut String, report: &BluetoothReport) {
    out.push_str("bt_reconnect=\n");
    let _ = writeln!(
        out,
        "- reconnect_state={}",
        format_option_hex_u8(report.bt_reconnect_state)
    );
    let _ = writeln!(
        out,
        "- reconnect_cycle_attempts={}",
        format_option_u8(report.bt_reconnect_cycle_attempts)
    );
    let _ = writeln!(
        out,
        "- last_reconnect_status={}",
        format_option_hex_u8(report.bt_last_reconnect_status)
    );
    let _ = writeln!(
        out,
        "- last_reconnect_reason={}",
        format_option_u8(report.bt_last_reconnect_reason)
    );
    let _ = writeln!(
        out,
        "- reconnect_schedule_count={}",
        format_option_u32(report.bt_reconnect_schedule_count)
    );
    let _ = writeln!(
        out,
        "- reconnect_attempt_count={}",
        format_option_u32(report.bt_reconnect_attempt_count)
    );
    let _ = writeln!(
        out,
        "- reconnect_success_count={}",
        format_option_u32(report.bt_reconnect_success_count)
    );
    let _ = writeln!(
        out,
        "- reconnect_failed_count={}",
        format_option_u32(report.bt_reconnect_failed_count)
    );
    let _ = writeln!(
        out,
        "- reconnect_blocked_count={}",
        format_option_u32(report.bt_reconnect_blocked_count)
    );
    let _ = writeln!(
        out,
        "- last_reconnect_ms={}",
        format_option_u32(report.bt_last_reconnect_ms)
    );
    let _ = writeln!(out);
}

pub(super) fn write_bt_acl_l2cap(out: &mut String, report: &BluetoothReport) {
    out.push_str("bt_acl_l2cap=\n");
    let _ = writeln!(
        out,
        "- connection_complete_count={}",
        format_option_u32(report.bt_connection_complete_count)
    );
    let _ = writeln!(
        out,
        "- last_connection_complete_status={}",
        format_option_hex_u8(report.bt_last_connection_complete_status)
    );
    let _ = writeln!(
        out,
        "- last_connection_complete_link_type={}",
        format_option_u8(report.bt_last_connection_complete_link_type)
    );
    let _ = writeln!(
        out,
        "- last_connection_complete_ms={}",
        format_option_u32(report.bt_last_connection_complete_ms)
    );
    let _ = writeln!(
        out,
        "- incoming_l2cap_connection_count={}",
        format_option_u32(report.bt_incoming_l2cap_connection_count)
    );
    let _ = writeln!(
        out,
        "- incoming_l2cap_hid_control_count={}",
        format_option_u32(report.bt_incoming_l2cap_hid_control_count)
    );
    let _ = writeln!(
        out,
        "- incoming_l2cap_hid_interrupt_count={}",
        format_option_u32(report.bt_incoming_l2cap_hid_interrupt_count)
    );
    let _ = writeln!(
        out,
        "- last_incoming_l2cap_psm={}",
        format_option_hex_u16(report.bt_last_incoming_l2cap_psm)
    );
    let _ = writeln!(
        out,
        "- last_incoming_l2cap_local_cid={}",
        format_option_hex_u16(report.bt_last_incoming_l2cap_local_cid)
    );
    let _ = writeln!(
        out,
        "- last_incoming_l2cap_ms={}",
        format_option_u32(report.bt_last_incoming_l2cap_ms)
    );
    let _ = writeln!(out);
}

pub(super) fn write_pc_usb_input(out: &mut String, report: &BluetoothReport) {
    out.push_str("pc_usb_input=\n");
    let _ = writeln!(out, "- mounted={}", format_option_bool(report.usb_mounted));
    let _ = writeln!(
        out,
        "- suspended={}",
        format_option_bool(report.usb_suspended)
    );
    let _ = writeln!(
        out,
        "- mount_count={}",
        format_option_u32(report.usb_mount_count)
    );
    let _ = writeln!(
        out,
        "- umount_count={}",
        format_option_u32(report.usb_umount_count)
    );
    let _ = writeln!(
        out,
        "- device_desc_count={}",
        format_option_u32(report.usb_device_desc_count)
    );
    let _ = writeln!(
        out,
        "- config_desc_count={}",
        format_option_u32(report.usb_config_desc_count)
    );
    let _ = writeln!(
        out,
        "- input_queued_count={}",
        format_option_u32(report.usb_input_queued_count)
    );
    let _ = writeln!(
        out,
        "- input_sent_count={}",
        format_option_u32(report.usb_input_sent_count)
    );
    let _ = writeln!(
        out,
        "- host_out_count={}",
        format_option_u32(report.usb_host_out_count)
    );
    let _ = writeln!(out);
}

pub(super) fn write_diag_lines_steps_and_notes(out: &mut String, report: &BluetoothReport) {
    out.push_str("relevant_diag_lines=\n");
    if report.relevant_diag_lines.is_empty() {
        out.push_str("- none\n");
    } else {
        for line in &report.relevant_diag_lines {
            let _ = writeln!(out, "- {line}");
        }
    }
    let _ = writeln!(out);

    out.push_str("next_steps=\n");
    for step in &report.next_steps {
        let _ = writeln!(out, "- {step}");
    }
    let _ = writeln!(out);

    out.push_str("notes=\n");
    for note in &report.notes {
        let _ = writeln!(out, "- {note}");
    }
}
