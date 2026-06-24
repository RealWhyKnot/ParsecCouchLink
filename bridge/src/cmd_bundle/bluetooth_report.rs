//! Bluetooth-mode bundle reports.

use std::fmt::Write as _;

use anyhow::Result;
use serde::Serialize;

use crate::{cdc, cmd_run, protocol};

use super::PicoBundleCapture;

#[derive(Clone, Debug, Serialize)]
pub(super) struct BluetoothReport {
    pub(super) artifact_schema_version: u8,
    pub(super) uid: String,
    pub(super) path: String,
    pub(super) peer: Option<String>,
    pub(super) live: bool,
    pub(super) persona: String,
    pub(super) target_label: String,
    pub(super) advertised_name: &'static str,
    pub(super) expected_connection: &'static str,
    pub(super) usb_input_required: bool,
    pub(super) usb_transport: &'static str,
    pub(super) bluetooth_output: &'static str,
    pub(super) pico_state_captured: bool,
    pub(super) usb_diag_captured: bool,
    pub(super) bt_status_cdc_captured: bool,
    pub(super) bt_status_cdc_error: Option<String>,
    pub(super) bt_status_version: Option<u8>,
    pub(super) bt_decoded_status_version: Option<u8>,
    pub(super) bt_status_newer_than_host: bool,
    pub(super) pico_diag_captured: bool,
    pub(super) status: &'static str,
    pub(super) warning: bool,
    pub(super) bt_receiver_contact: &'static str,
    pub(super) bt_flags: u8,
    pub(super) bt_started: bool,
    pub(super) bt_connected: bool,
    pub(super) bt_send_requested: bool,
    pub(super) bt_target: u8,
    pub(super) bt_last_status: u8,
    pub(super) bt_report_len: u8,
    pub(super) bt_cid: u16,
    pub(super) bt_init_count: u32,
    pub(super) bt_ready_count: u32,
    pub(super) bt_open_count: u32,
    pub(super) bt_close_count: u32,
    pub(super) bt_can_send_count: u32,
    pub(super) bt_report_build_count: u32,
    pub(super) bt_report_send_count: u32,
    pub(super) bt_send_request_count: u32,
    pub(super) bt_last_event_ms: u32,
    pub(super) bt_last_send_ms: u32,
    pub(super) pc_input_observed: bool,
    pub(super) pc_input_evidence: &'static str,
    pub(super) bt_reported_local_name: Option<String>,
    pub(super) bt_get_report_count: Option<u32>,
    pub(super) bt_get_report_success_count: Option<u32>,
    pub(super) bt_get_report_unsupported_count: Option<u32>,
    pub(super) bt_set_report_count: Option<u32>,
    pub(super) bt_set_report_accepted_count: Option<u32>,
    pub(super) bt_set_report_unsupported_count: Option<u32>,
    pub(super) bt_out_report_count: Option<u32>,
    pub(super) bt_out_report_accepted_count: Option<u32>,
    pub(super) bt_out_report_unsupported_count: Option<u32>,
    pub(super) bt_last_get_report_id: Option<u8>,
    pub(super) bt_last_get_report_type: Option<u8>,
    pub(super) bt_last_set_report_id: Option<u8>,
    pub(super) bt_last_set_report_type: Option<u8>,
    pub(super) bt_last_out_report_id: Option<u8>,
    pub(super) bt_last_out_report_type: Option<u8>,
    pub(super) bt_last_get_report_len: Option<u16>,
    pub(super) bt_last_set_report_len: Option<u16>,
    pub(super) bt_last_out_report_len: Option<u16>,
    pub(super) bt_security_event_source: &'static str,
    pub(super) bt_pin_code_request_count: Option<u32>,
    pub(super) bt_pin_code_response_count: Option<u32>,
    pub(super) bt_user_confirmation_request_count: Option<u32>,
    pub(super) bt_user_confirmation_response_count: Option<u32>,
    pub(super) bt_simple_pairing_complete_count: Option<u32>,
    pub(super) bt_authentication_complete_count: Option<u32>,
    pub(super) bt_link_key_notification_count: Option<u32>,
    pub(super) bt_encryption_change_count: Option<u32>,
    pub(super) bt_disconnection_complete_count: Option<u32>,
    pub(super) bt_hid_open_failed_count: Option<u32>,
    pub(super) bt_last_security_event_ms: Option<u32>,
    pub(super) bt_last_simple_pairing_status: Option<u8>,
    pub(super) bt_last_authentication_status: Option<u8>,
    pub(super) bt_last_encryption_status: Option<u8>,
    pub(super) bt_last_encryption_enabled: Option<u8>,
    pub(super) bt_last_disconnection_reason: Option<u8>,
    pub(super) bt_last_hid_open_status: Option<u8>,
    pub(super) bt_reconnect_state: Option<u8>,
    pub(super) bt_reconnect_cycle_attempts: Option<u8>,
    pub(super) bt_last_reconnect_status: Option<u8>,
    pub(super) bt_last_reconnect_reason: Option<u8>,
    pub(super) bt_reconnect_schedule_count: Option<u32>,
    pub(super) bt_reconnect_attempt_count: Option<u32>,
    pub(super) bt_reconnect_success_count: Option<u32>,
    pub(super) bt_reconnect_failed_count: Option<u32>,
    pub(super) bt_reconnect_blocked_count: Option<u32>,
    pub(super) bt_last_reconnect_ms: Option<u32>,
    pub(super) bt_connection_complete_count: Option<u32>,
    pub(super) bt_last_connection_complete_status: Option<u8>,
    pub(super) bt_last_connection_complete_link_type: Option<u8>,
    pub(super) bt_last_connection_complete_ms: Option<u32>,
    pub(super) bt_incoming_l2cap_connection_count: Option<u32>,
    pub(super) bt_incoming_l2cap_hid_control_count: Option<u32>,
    pub(super) bt_incoming_l2cap_hid_interrupt_count: Option<u32>,
    pub(super) bt_last_incoming_l2cap_psm: Option<u16>,
    pub(super) bt_last_incoming_l2cap_local_cid: Option<u16>,
    pub(super) bt_last_incoming_l2cap_ms: Option<u32>,
    pub(super) bt_cdc_input_status: &'static str,
    pub(super) bt_cdc_state_count: Option<u32>,
    pub(super) bt_cdc_heartbeat_count: Option<u32>,
    pub(super) bt_cdc_bad_length_count: Option<u32>,
    pub(super) bt_cdc_rejected_count: Option<u32>,
    pub(super) bt_cdc_last_frame_ms: Option<u32>,
    pub(super) bt_cdc_last_state_ms: Option<u32>,
    pub(super) bt_cdc_last_heartbeat_ms: Option<u32>,
    pub(super) bt_cdc_last_seq: Option<u8>,
    pub(super) bt_cdc_last_command: Option<u8>,
    pub(super) bt_cdc_last_flags: Option<u8>,
    pub(super) usb_mounted: Option<bool>,
    pub(super) usb_suspended: Option<bool>,
    pub(super) usb_mount_count: Option<u32>,
    pub(super) usb_umount_count: Option<u32>,
    pub(super) usb_device_desc_count: Option<u32>,
    pub(super) usb_config_desc_count: Option<u32>,
    pub(super) usb_input_queued_count: Option<u32>,
    pub(super) usb_input_sent_count: Option<u32>,
    pub(super) usb_host_out_count: Option<u32>,
    pub(super) relevant_diag_lines: Vec<String>,
    pub(super) next_steps: Vec<&'static str>,
    pub(super) notes: Vec<&'static str>,
}

pub(super) struct BluetoothReportInput<'a> {
    pub(super) pico_state: Option<&'a protocol::PicoStateDiag>,
    pub(super) bt_status: Option<&'a cdc::BtStatus>,
    pub(super) bt_status_error: Option<String>,
    pub(super) usb_diag: Option<&'a protocol::UsbDiag>,
    pub(super) pico_diag_text: &'a str,
}

pub(super) fn build_bluetooth_report(
    uid: &str,
    path: &str,
    target: &cmd_run::PicoTarget,
    input: BluetoothReportInput<'_>,
) -> BluetoothReport {
    let pico_state = input.pico_state;
    let bt_status = input.bt_status;
    let usb_diag = input.usb_diag;
    let bt_flags = bt_status
        .map(|status| status.flags)
        .or_else(|| pico_state.map(|state| state.bt_flags))
        .unwrap_or(0);
    let bt_started = bt_flags & protocol::BT_HID_STATUS_STARTED != 0;
    let bt_connected = bt_flags & protocol::BT_HID_STATUS_CONNECTED != 0;
    let bt_send_requested = bt_flags & protocol::BT_HID_STATUS_SEND_REQUESTED != 0;
    let bt_report_send_count = bt_status
        .map(|status| status.report_send_count)
        .or_else(|| pico_state.map(|state| state.bt_report_send_count))
        .unwrap_or(0);
    let pc_input = bluetooth_pc_input_evidence(pico_state, bt_status, usb_diag);
    let bt_target = bt_status
        .map(|status| status.target)
        .or_else(|| pico_state.map(|state| state.bt_target))
        .unwrap_or_else(|| bluetooth_target_from_persona(target.persona));
    let bt_status_cdc_captured = bt_status.is_some();
    let status = bluetooth_report_status(
        pico_state.is_some() || bt_status_cdc_captured,
        bt_started,
        bt_connected,
        bt_report_send_count,
        pc_input.observed,
        pc_input.bt_cdc_input_status,
    );
    let bt_last_status = bt_status
        .map(|status| status.last_status)
        .or_else(|| pico_state.map(|state| state.bt_last_status))
        .unwrap_or(0);
    let bt_open_count = bt_status
        .map(|status| status.open_count)
        .or_else(|| pico_state.map(|state| state.bt_open_count))
        .unwrap_or(0);
    let bt_close_count = bt_status
        .map(|status| status.close_count)
        .or_else(|| pico_state.map(|state| state.bt_close_count))
        .unwrap_or(0);
    let bt_get_report_count = bt_status.map(|status| status.get_report_count);
    let bt_set_report_count = bt_status.map(|status| status.set_report_count);
    let bt_out_report_count = bt_status.map(|status| status.out_report_count);
    let bt_security_contact_from_cdc = bt_status
        .map(|status| status.pairing_security_contact_seen())
        .unwrap_or(false);
    let bt_security_contact_from_diag = bluetooth_diag_security_contact_seen(input.pico_diag_text);
    let bt_security_event_source = if bt_security_contact_from_cdc {
        "cdc_status"
    } else if bt_security_contact_from_diag {
        "diag_log"
    } else {
        "none"
    };
    let bt_reconnect_schedule_count = bt_status.map(|status| status.reconnect_schedule_count);
    let bt_reconnect_attempt_count = bt_status.map(|status| status.reconnect_attempt_count);
    let bt_reconnect_failed_count = bt_status.map(|status| status.reconnect_failed_count);
    let bt_reconnect_blocked_count = bt_status.map(|status| status.reconnect_blocked_count);
    let bt_connection_complete_count = bt_status.map(|status| status.connection_complete_count);
    let bt_incoming_l2cap_connection_count =
        bt_status.map(|status| status.incoming_l2cap_connection_count);
    let bt_receiver_contact = bluetooth_receiver_contact(BluetoothContactState {
        bt_started,
        bt_connected,
        bt_last_status,
        bt_open_count,
        bt_close_count,
        bt_hid_open_failed_count: bt_status
            .map(|status| status.hid_open_failed_count)
            .unwrap_or(0),
        bt_security_contact_seen: bt_security_contact_from_cdc || bt_security_contact_from_diag,
        bt_reconnect_pending: bt_status
            .map(|status| status.reconnect_pending() || status.reconnect_in_progress())
            .unwrap_or(false),
        bt_reconnect_activity_seen: bt_status
            .map(|status| status.reconnect_activity_seen())
            .unwrap_or(false),
        bt_reconnect_attempt_count: bt_reconnect_attempt_count.unwrap_or(0),
        bt_reconnect_failed_count: bt_reconnect_failed_count.unwrap_or(0),
        bt_reconnect_blocked_count: bt_reconnect_blocked_count.unwrap_or(0),
        bt_incoming_l2cap_connection_count: bt_incoming_l2cap_connection_count.unwrap_or(0),
        bt_get_report_count: bt_get_report_count.unwrap_or(0),
        bt_set_report_count: bt_set_report_count.unwrap_or(0),
        bt_out_report_count: bt_out_report_count.unwrap_or(0),
        bt_ready_count: bt_status
            .map(|status| status.ready_count)
            .or_else(|| pico_state.map(|state| state.bt_ready_count))
            .unwrap_or(0),
    });
    BluetoothReport {
        artifact_schema_version: 6,
        uid: uid.to_string(),
        path: path.to_string(),
        peer: Some(target.peer.to_string()),
        live: true,
        persona: target.persona.label().to_string(),
        target_label: protocol::bt_hid_target_label(bt_target).to_string(),
        advertised_name: bluetooth_advertised_name(bt_target),
        expected_connection: "pc_usb_input_bluetooth_output",
        usb_input_required: true,
        usb_transport: "cdc_framed_controller_state",
        bluetooth_output: "classic_bluetooth_hid_gamepad",
        pico_state_captured: pico_state.is_some(),
        usb_diag_captured: usb_diag.is_some(),
        bt_status_cdc_captured,
        bt_status_cdc_error: input.bt_status_error,
        bt_status_version: bt_status.map(|status| status.status_version),
        bt_decoded_status_version: bt_status.map(|status| status.decoded_status_version),
        bt_status_newer_than_host: bt_status
            .map(|status| status.newer_status_version())
            .unwrap_or(false),
        pico_diag_captured: !input.pico_diag_text.trim().is_empty(),
        status,
        warning: status != "reports_sent",
        bt_receiver_contact,
        bt_flags,
        bt_started,
        bt_connected,
        bt_send_requested,
        bt_target,
        bt_last_status,
        bt_report_len: bt_status
            .map(|status| status.report_len)
            .or_else(|| pico_state.map(|state| state.bt_report_len))
            .unwrap_or(0),
        bt_cid: bt_status
            .map(|status| status.cid)
            .or_else(|| pico_state.map(|state| state.bt_cid))
            .unwrap_or(0),
        bt_init_count: bt_status
            .map(|status| status.init_count)
            .or_else(|| pico_state.map(|state| state.bt_init_count))
            .unwrap_or(0),
        bt_ready_count: bt_status
            .map(|status| status.ready_count)
            .or_else(|| pico_state.map(|state| state.bt_ready_count))
            .unwrap_or(0),
        bt_open_count,
        bt_close_count,
        bt_can_send_count: bt_status
            .map(|status| status.can_send_count)
            .or_else(|| pico_state.map(|state| state.bt_can_send_count))
            .unwrap_or(0),
        bt_report_build_count: bt_status
            .map(|status| status.report_build_count)
            .or_else(|| pico_state.map(|state| state.bt_report_build_count))
            .unwrap_or(0),
        bt_report_send_count,
        bt_send_request_count: bt_status
            .map(|status| status.send_request_count)
            .or_else(|| pico_state.map(|state| state.bt_send_request_count))
            .unwrap_or(0),
        bt_last_event_ms: bt_status
            .map(|status| status.last_event_ms)
            .or_else(|| pico_state.map(|state| state.bt_last_event_ms))
            .unwrap_or(0),
        bt_last_send_ms: bt_status
            .map(|status| status.last_send_ms)
            .or_else(|| pico_state.map(|state| state.bt_last_send_ms))
            .unwrap_or(0),
        pc_input_observed: pc_input.observed,
        pc_input_evidence: pc_input.evidence,
        bt_reported_local_name: bt_status
            .map(|status| status.local_name.trim())
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string()),
        bt_get_report_count,
        bt_get_report_success_count: bt_status.map(|status| status.get_report_success_count),
        bt_get_report_unsupported_count: bt_status
            .map(|status| status.get_report_unsupported_count),
        bt_set_report_count,
        bt_set_report_accepted_count: bt_status.map(|status| status.set_report_accepted_count),
        bt_set_report_unsupported_count: bt_status
            .map(|status| status.set_report_unsupported_count),
        bt_out_report_count,
        bt_out_report_accepted_count: bt_status.map(|status| status.out_report_accepted_count),
        bt_out_report_unsupported_count: bt_status
            .map(|status| status.out_report_unsupported_count),
        bt_last_get_report_id: bt_status.map(|status| status.last_get_report_id),
        bt_last_get_report_type: bt_status.map(|status| status.last_get_report_type),
        bt_last_set_report_id: bt_status.map(|status| status.last_set_report_id),
        bt_last_set_report_type: bt_status.map(|status| status.last_set_report_type),
        bt_last_out_report_id: bt_status.map(|status| status.last_out_report_id),
        bt_last_out_report_type: bt_status.map(|status| status.last_out_report_type),
        bt_last_get_report_len: bt_status.map(|status| status.last_get_report_len),
        bt_last_set_report_len: bt_status.map(|status| status.last_set_report_len),
        bt_last_out_report_len: bt_status.map(|status| status.last_out_report_len),
        bt_security_event_source,
        bt_pin_code_request_count: bt_status.map(|status| status.pin_code_request_count),
        bt_pin_code_response_count: bt_status.map(|status| status.pin_code_response_count),
        bt_user_confirmation_request_count: bt_status
            .map(|status| status.user_confirmation_request_count),
        bt_user_confirmation_response_count: bt_status
            .map(|status| status.user_confirmation_response_count),
        bt_simple_pairing_complete_count: bt_status
            .map(|status| status.simple_pairing_complete_count),
        bt_authentication_complete_count: bt_status
            .map(|status| status.authentication_complete_count),
        bt_link_key_notification_count: bt_status.map(|status| status.link_key_notification_count),
        bt_encryption_change_count: bt_status.map(|status| status.encryption_change_count),
        bt_disconnection_complete_count: bt_status
            .map(|status| status.disconnection_complete_count),
        bt_hid_open_failed_count: bt_status.map(|status| status.hid_open_failed_count),
        bt_last_security_event_ms: bt_status.map(|status| status.last_security_event_ms),
        bt_last_simple_pairing_status: bt_status
            .map(|status| status.last_simple_pairing_status),
        bt_last_authentication_status: bt_status.map(|status| status.last_authentication_status),
        bt_last_encryption_status: bt_status.map(|status| status.last_encryption_status),
        bt_last_encryption_enabled: bt_status.map(|status| status.last_encryption_enabled),
        bt_last_disconnection_reason: bt_status.map(|status| status.last_disconnection_reason),
        bt_last_hid_open_status: bt_status.map(|status| status.last_hid_open_status),
        bt_reconnect_state: bt_status.map(|status| status.reconnect_state),
        bt_reconnect_cycle_attempts: bt_status.map(|status| status.reconnect_cycle_attempts),
        bt_last_reconnect_status: bt_status.map(|status| status.last_reconnect_status),
        bt_last_reconnect_reason: bt_status.map(|status| status.last_reconnect_reason),
        bt_reconnect_schedule_count,
        bt_reconnect_attempt_count,
        bt_reconnect_success_count: bt_status.map(|status| status.reconnect_success_count),
        bt_reconnect_failed_count,
        bt_reconnect_blocked_count,
        bt_last_reconnect_ms: bt_status.map(|status| status.last_reconnect_ms),
        bt_connection_complete_count,
        bt_last_connection_complete_status: bt_status
            .map(|status| status.last_connection_complete_status),
        bt_last_connection_complete_link_type: bt_status
            .map(|status| status.last_connection_complete_link_type),
        bt_last_connection_complete_ms: bt_status.map(|status| status.last_connection_complete_ms),
        bt_incoming_l2cap_connection_count,
        bt_incoming_l2cap_hid_control_count: bt_status
            .map(|status| status.incoming_l2cap_hid_control_count),
        bt_incoming_l2cap_hid_interrupt_count: bt_status
            .map(|status| status.incoming_l2cap_hid_interrupt_count),
        bt_last_incoming_l2cap_psm: bt_status.map(|status| status.last_incoming_l2cap_psm),
        bt_last_incoming_l2cap_local_cid: bt_status
            .map(|status| status.last_incoming_l2cap_local_cid),
        bt_last_incoming_l2cap_ms: bt_status.map(|status| status.last_incoming_l2cap_ms),
        bt_cdc_input_status: pc_input.bt_cdc_input_status,
        bt_cdc_state_count: bt_cdc_u32(bt_status, |status| status.bt_cdc_state_count),
        bt_cdc_heartbeat_count: bt_cdc_u32(bt_status, |status| status.bt_cdc_heartbeat_count),
        bt_cdc_bad_length_count: bt_cdc_u32(bt_status, |status| status.bt_cdc_bad_length_count),
        bt_cdc_rejected_count: bt_cdc_u32(bt_status, |status| status.bt_cdc_rejected_count),
        bt_cdc_last_frame_ms: bt_cdc_u32(bt_status, |status| status.bt_cdc_last_frame_ms),
        bt_cdc_last_state_ms: bt_cdc_u32(bt_status, |status| status.bt_cdc_last_state_ms),
        bt_cdc_last_heartbeat_ms: bt_cdc_u32(bt_status, |status| {
            status.bt_cdc_last_heartbeat_ms
        }),
        bt_cdc_last_seq: bt_cdc_u8(bt_status, |status| status.bt_cdc_last_seq),
        bt_cdc_last_command: bt_cdc_u8(bt_status, |status| status.bt_cdc_last_command),
        bt_cdc_last_flags: bt_cdc_u8(bt_status, |status| status.bt_cdc_last_flags),
        usb_mounted: usb_diag.map(|diag| diag.mounted()),
        usb_suspended: usb_diag.map(|diag| diag.suspended()),
        usb_mount_count: usb_diag.map(|diag| diag.mount_count),
        usb_umount_count: usb_diag.map(|diag| diag.umount_count),
        usb_device_desc_count: usb_diag.map(|diag| diag.device_desc_count),
        usb_config_desc_count: usb_diag.map(|diag| diag.config_desc_count),
        usb_input_queued_count: usb_diag.map(|diag| diag.xinput_in_queued_count),
        usb_input_sent_count: usb_diag.map(|diag| diag.xinput_in_sent_count),
        usb_host_out_count: usb_diag.map(|diag| diag.xinput_out_count),
        relevant_diag_lines: bluetooth_relevant_diag_lines(input.pico_diag_text),
        next_steps: bluetooth_report_next_steps(
            status,
            bt_receiver_contact,
            pc_input.bt_cdc_input_status,
        ),
        notes: vec![
            "Bluetooth mode streams controller input from this PC to the Pico over USB CDC.",
            "The Pico then emits a Classic Bluetooth HID gamepad report to the paired receiver.",
            "The advertised_name field is the exact Bluetooth name the receiver should see.",
            "bt_receiver_contact separates pairing/security contact, HID channel contact, and input-stream failures.",
            "host/xinput-sources.txt lists the Windows controller slots visible when the bundle was captured.",
            "For BITFUNX/BlueRetro N64, try couchlink blueretro-playstation first, then couchlink blueretro; use blueretro-xbox only as a diagnostic.",
            "Wi-Fi discovery may still appear in logs, but live Bluetooth controller packets are not sent over Wi-Fi.",
            "USB adapter survey is skipped for Bluetooth mode because the Pico USB connector stays on the PC.",
            "The live Bluetooth run command is a continuous streaming loop; stop it manually after pairing or after capturing enough status.",
        ],
    }
}

pub(super) fn bluetooth_target_from_persona(persona: protocol::Persona) -> u8 {
    match persona {
        protocol::Persona::BluetoothXbox => 1,
        protocol::Persona::BluetoothPlaystation => 2,
        _ => 0,
    }
}

pub(super) fn bluetooth_advertised_name(target: u8) -> &'static str {
    match target {
        1 => "Xbox Wireless Controller",
        2 => "Wireless Controller",
        _ => "CouchLink BT HID",
    }
}

struct BluetoothContactState {
    bt_started: bool,
    bt_connected: bool,
    bt_last_status: u8,
    bt_open_count: u32,
    bt_close_count: u32,
    bt_hid_open_failed_count: u32,
    bt_security_contact_seen: bool,
    bt_reconnect_pending: bool,
    bt_reconnect_activity_seen: bool,
    bt_reconnect_attempt_count: u32,
    bt_reconnect_failed_count: u32,
    bt_reconnect_blocked_count: u32,
    bt_incoming_l2cap_connection_count: u32,
    bt_get_report_count: u32,
    bt_set_report_count: u32,
    bt_out_report_count: u32,
    bt_ready_count: u32,
}

fn bluetooth_receiver_contact(state: BluetoothContactState) -> &'static str {
    if !state.bt_started {
        "radio_not_started"
    } else if state.bt_connected
        || state.bt_open_count > 0
        || state.bt_close_count > 0
        || state.bt_get_report_count > 0
        || state.bt_set_report_count > 0
        || state.bt_out_report_count > 0
    {
        "hid_receiver_contact_seen"
    } else if state.bt_hid_open_failed_count > 0 || state.bt_last_status != 0 {
        "hid_open_failed"
    } else if state.bt_incoming_l2cap_connection_count > 0 {
        "hid_l2cap_incoming_no_hid_open"
    } else if state.bt_reconnect_attempt_count > 0
        || state.bt_reconnect_failed_count > 0
        || state.bt_reconnect_blocked_count > 0
    {
        "hid_reconnect_attempted_no_hid_open"
    } else if state.bt_reconnect_pending || state.bt_reconnect_activity_seen {
        "hid_reconnect_pending"
    } else if state.bt_security_contact_seen {
        "pairing_security_contact_no_hid_open"
    } else if state.bt_ready_count > 0 {
        "discoverable_no_hid_contact"
    } else {
        "stack_started_no_ready_event"
    }
}

fn bluetooth_diag_security_contact_seen(text: &str) -> bool {
    text.lines().any(|line| {
        line.contains("bt_hid: pin_code_request")
            || line.contains("bt_hid: user_confirmation_request")
            || line.contains("bt_hid: simple_pairing_complete")
            || line.contains("bt_hid: authentication_complete")
            || line.contains("bt_hid: link_key_notification")
            || line.contains("bt_hid: encryption_change")
            || line.contains("bt_hid: disconnect_complete")
            || line.contains("bt_hid: connection failed")
    })
}

pub(super) fn bluetooth_report_status(
    state_captured: bool,
    bt_started: bool,
    bt_connected: bool,
    bt_report_send_count: u32,
    pc_input_observed: bool,
    bt_cdc_input_status: &str,
) -> &'static str {
    if !state_captured {
        "pico_state_missing"
    } else if !bt_started {
        "bluetooth_stack_not_started"
    } else if !bt_connected {
        "waiting_for_receiver"
    } else if bt_report_send_count == 0 {
        "connected_waiting_for_input"
    } else if !pc_input_observed {
        match bt_cdc_input_status {
            "host_stream_not_active" => "receiver_reports_sent_stream_not_active",
            "cdc_input_errors" => "receiver_reports_sent_cdc_input_errors",
            "source_never_connected" => "receiver_reports_sent_source_never_connected",
            "source_idle" => "receiver_reports_sent_source_idle",
            _ => "receiver_reports_sent_no_pc_input",
        }
    } else {
        "reports_sent"
    }
}

struct BluetoothPcInputEvidence {
    observed: bool,
    evidence: &'static str,
    bt_cdc_input_status: &'static str,
}

fn bluetooth_pc_input_evidence(
    pico_state: Option<&protocol::PicoStateDiag>,
    bt_status: Option<&cdc::BtStatus>,
    usb_diag: Option<&protocol::UsbDiag>,
) -> BluetoothPcInputEvidence {
    let mut bt_cdc_input_status = "not_captured";
    if let Some(status) = bt_status {
        if status.bt_cdc_counters_captured() {
            bt_cdc_input_status = if status.bt_cdc_state_count > 0 {
                "state_frames"
            } else if status.bt_cdc_bad_length_count > 0 || status.bt_cdc_rejected_count > 0 {
                "cdc_input_errors"
            } else if status.bt_cdc_heartbeat_count > 0
                && status.bt_cdc_last_flags & protocol::FLAG_PARSEC_CONNECTED != 0
            {
                "source_idle"
            } else if status.bt_cdc_heartbeat_count > 0 {
                "source_never_connected"
            } else {
                "no_cdc_frames"
            };
            if status.bt_cdc_state_count > 0 {
                return BluetoothPcInputEvidence {
                    observed: true,
                    evidence: "bt_status_cdc_state",
                    bt_cdc_input_status,
                };
            }
            if status.bt_cdc_bad_length_count > 0 || status.bt_cdc_rejected_count > 0 {
                return BluetoothPcInputEvidence {
                    observed: false,
                    evidence: "bt_status_cdc_input_errors",
                    bt_cdc_input_status,
                };
            }
            if status.bt_cdc_heartbeat_count > 0 {
                return BluetoothPcInputEvidence {
                    observed: false,
                    evidence: if status.bt_cdc_last_flags & protocol::FLAG_PARSEC_CONNECTED != 0 {
                        "bt_status_cdc_heartbeat"
                    } else {
                        "bt_status_cdc_heartbeat_source_disconnected"
                    },
                    bt_cdc_input_status,
                };
            }
        } else {
            bt_cdc_input_status = "not_captured_pre_v5";
        }
    }
    if pico_state
        .map(|state| {
            state.last_bridge_packet_ms > 0
                || state.xinput_in_queued_count > 0
                || state.xinput_in_sent_count > 0
        })
        .unwrap_or(false)
    {
        return BluetoothPcInputEvidence {
            observed: true,
            evidence: "pico_state_input",
            bt_cdc_input_status,
        };
    }
    if usb_diag
        .map(|diag| {
            diag.last_bridge_packet_ms > 0
                || diag.xinput_in_queued_count > 0
                || diag.xinput_in_sent_count > 0
        })
        .unwrap_or(false)
    {
        return BluetoothPcInputEvidence {
            observed: true,
            evidence: "usb_diag_input",
            bt_cdc_input_status,
        };
    }
    if matches!(bt_cdc_input_status, "not_captured" | "not_captured_pre_v5") {
        if let Some(status) = legacy_stream_status(pico_state, usb_diag) {
            return BluetoothPcInputEvidence {
                observed: false,
                evidence: match status {
                    "host_stream_not_active" => "legacy_stream_not_active",
                    "source_never_connected" => "legacy_stream_source_disconnected",
                    _ => "legacy_stream_source_idle",
                },
                bt_cdc_input_status: status,
            };
        }
    }
    BluetoothPcInputEvidence {
        observed: false,
        evidence: "none",
        bt_cdc_input_status,
    }
}

fn legacy_stream_status(
    pico_state: Option<&protocol::PicoStateDiag>,
    usb_diag: Option<&protocol::UsbDiag>,
) -> Option<&'static str> {
    if pico_state.is_none() && usb_diag.is_none() {
        return None;
    }
    let bridge_peer = pico_state
        .map(|state| state.activity_flags & protocol::USB_DIAG_ACTIVITY_PEER != 0)
        .unwrap_or(false)
        || usb_diag
            .map(|diag| diag.bridge_peer_latched())
            .unwrap_or(false);
    if !bridge_peer {
        return Some("host_stream_not_active");
    }
    let source_connected = pico_state
        .map(|state| state.activity_flags & protocol::USB_DIAG_ACTIVITY_PARSEC != 0)
        .unwrap_or(false)
        || usb_diag
            .map(|diag| diag.parsec_connected())
            .unwrap_or(false);
    Some(if source_connected {
        "source_idle"
    } else {
        "source_never_connected"
    })
}

fn bt_cdc_u32(
    bt_status: Option<&cdc::BtStatus>,
    read: impl FnOnce(&cdc::BtStatus) -> u32,
) -> Option<u32> {
    bt_status
        .filter(|status| status.bt_cdc_counters_captured())
        .map(read)
}

fn bt_cdc_u8(
    bt_status: Option<&cdc::BtStatus>,
    read: impl FnOnce(&cdc::BtStatus) -> u8,
) -> Option<u8> {
    bt_status
        .filter(|status| status.bt_cdc_counters_captured())
        .map(read)
}

pub(super) fn bluetooth_report_next_steps(
    status: &str,
    bt_receiver_contact: &str,
    bt_cdc_input_status: &str,
) -> Vec<&'static str> {
    match status {
        "pico_state_missing" => vec![
            "Keep the Pico plugged into this PC over USB and rerun couchlink bundle while Bluetooth mode is active.",
            "If Wi-Fi discovery is unavailable, use the USB diagnostic device identity VID 0x2E8A PID 0xCAF0 to confirm the Pico is attached.",
        ],
        "bluetooth_stack_not_started" => vec![
            "Check pico-diag.txt for cyw43 or Bluetooth initialization failures.",
            "Reflash the Pico if the firmware log never reaches a bt_hid init line.",
        ],
        "waiting_for_receiver" if bt_receiver_contact == "pairing_security_contact_no_hid_open" => {
            vec![
                "The receiver reached Bluetooth pairing/security but did not open a Classic HID control or interrupt channel.",
                "Clear the receiver-side pairing entry, put the adapter back into pairing mode, and pair again.",
                "For BITFUNX/BlueRetro N64, try couchlink blueretro-playstation first, then couchlink blueretro; use blueretro-xbox only as a diagnostic.",
            ]
        }
        "waiting_for_receiver" if bt_receiver_contact == "hid_reconnect_pending" => {
            vec![
                "Pairing/security completed and the Pico scheduled an active Classic HID reconnect to the paired receiver.",
                "Keep the receiver powered and in range, wait a few seconds, then rerun couchlink bundle if HID does not open.",
                "If this repeats on BITFUNX/BlueRetro N64, clear adapter pairing and try couchlink blueretro-playstation first, then couchlink blueretro.",
            ]
        }
        "waiting_for_receiver"
            if bt_receiver_contact == "hid_reconnect_attempted_no_hid_open" =>
        {
            vec![
                "The Pico actively tried to reconnect the paired receiver, but no Classic HID control or interrupt channel opened.",
                "Keep this bundle with bt_reconnect and bt_acl_l2cap counters; they show whether paging failed, ACL connected, or a HID channel was attempted.",
                "For BITFUNX/BlueRetro N64, clear receiver pairing and try couchlink blueretro-playstation first, then couchlink blueretro.",
            ]
        }
        "waiting_for_receiver" if bt_receiver_contact == "hid_l2cap_incoming_no_hid_open" => {
            vec![
                "The receiver reached a Classic HID L2CAP channel, but BTstack did not report a completed HID open.",
                "Keep this bundle and inspect the bt_acl_l2cap counters with receiver-side or HCI logs if available.",
                "Clear receiver pairing and retry couchlink blueretro-playstation, then couchlink blueretro if the Xbox-named Classic HID mimic still does not open.",
            ]
        }
        "waiting_for_receiver" => {
            vec![
                "Put the target Bluetooth receiver or adapter into pairing mode and pair with the CouchLink Bluetooth device.",
                "If it was previously paired, remove the old pairing on the receiver side and pair again.",
                "If bt_receiver_contact stays discoverable_no_hid_contact during pairing, the receiver has not reached pairing/security or opened a HID channel to the Pico.",
            ]
        }
        "connected_waiting_for_input" => vec![
            "Start couchlink bluetooth with the Pico plugged into this PC over USB.",
            "Move or press the source controller and rerun bundle if report_send_count stays at zero.",
        ],
        "receiver_reports_sent_stream_not_active" => vec![
            "The receiver opened Classic HID and the Pico sent Bluetooth reports, but this bundle did not see the host Bluetooth stream feed USB CDC input frames.",
            "Start couchlink bluetooth with the Pico plugged into this PC over USB, keep it running, then move or press the source controller before capturing a bundle.",
            "Keep host/xinput-sources.txt and the stream-status lines; they show whether the selected Windows controller slot was live.",
        ],
        "receiver_reports_sent_cdc_input_errors" => vec![
            "The host Bluetooth stream reached the Pico, but BT_STATUS v5 recorded malformed or rejected USB CDC input frames.",
            "Update both host and firmware from the same build, restart couchlink bluetooth, and capture a fresh bundle if the rejected counters keep rising.",
            "Keep the bt_cdc_input counters with the host logs; they show whether frames were too short, had a bad command, or failed validation before becoming controller state.",
        ],
        "receiver_reports_sent_source_never_connected" => vec![
            "The host Bluetooth stream reached the Pico, but the selected Windows source controller was not connected.",
            "Choose the live Windows controller slot in guided mode or run couchlink bluetooth again after connecting the Parsec or local controller.",
            "Keep host/xinput-sources.txt and the stream-status lines; they show which Windows controller slots were live.",
        ],
        "receiver_reports_sent_source_idle" => vec![
            "The host Bluetooth stream reached the Pico and the source controller was connected, but no changed controller state was captured.",
            "Press or move the source controller during the stream, then capture a new bundle before changing modes or unplugging the Pico.",
            "If the receiver still ignores changed input, keep this bundle and inspect receiver-side controller-mimic compatibility next.",
        ],
        "receiver_reports_sent_no_pc_input" => vec![
            "The receiver opened Classic HID and the Pico sent reports, but this bundle saw no PC controller input frames reach the Pico.",
            if bt_cdc_input_status == "not_captured_pre_v5" {
                "This bundle predates BT_STATUS v5 CDC counters; update the host and firmware, then recapture while Bluetooth streaming is active."
            } else {
                "BT_STATUS v5 saw no Bluetooth CDC state or heartbeat frames from the host during this capture."
            },
            "Start Bluetooth streaming with a live Windows source controller; if the saved route points at a waiting slot, choose the live source or rerun guided mode.",
            "Keep host/xinput-sources.txt plus the stream-status lines from this bundle; they show which Windows controller slots were live.",
        ],
        _ => vec![
            "Bluetooth reports were sent. If the receiver still does not react, keep the full bundle and inspect receiver-side pairing or controller-mimic compatibility.",
        ],
    }
}

pub(super) fn bluetooth_relevant_diag_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            line.contains("bt_hid:")
                || line.contains("Bluetooth")
                || line.contains("CDC")
                || line.contains("cdc:")
                || line.contains("usb_init:")
                || line.contains("run:")
        })
        .map(|line| line.to_string())
        .collect()
}

pub(super) fn format_bluetooth_report_text(report: &BluetoothReport) -> String {
    let mut out = String::from("Bluetooth output report\n\n");
    let _ = writeln!(out, "uid={}", report.uid);
    let _ = writeln!(out, "path={}", report.path);
    let _ = writeln!(out, "peer={}", report.peer.as_deref().unwrap_or("none"));
    let _ = writeln!(out, "persona={}", report.persona);
    let _ = writeln!(out, "target_label={}", report.target_label);
    let _ = writeln!(out, "advertised_name={}", report.advertised_name);
    let _ = writeln!(out, "expected_connection={}", report.expected_connection);
    let _ = writeln!(out, "usb_input_required={}", report.usb_input_required);
    let _ = writeln!(out, "usb_transport={}", report.usb_transport);
    let _ = writeln!(out, "bluetooth_output={}", report.bluetooth_output);
    let _ = writeln!(out, "status={}", report.status);
    let _ = writeln!(out, "warning={}", report.warning);
    let _ = writeln!(out, "pico_state_captured={}", report.pico_state_captured);
    let _ = writeln!(out, "usb_diag_captured={}", report.usb_diag_captured);
    let _ = writeln!(
        out,
        "bt_status_cdc_captured={}",
        report.bt_status_cdc_captured
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
    let _ = writeln!(out, "pico_diag_captured={}", report.pico_diag_captured);
    let _ = writeln!(out, "bt_receiver_contact={}", report.bt_receiver_contact);
    let _ = writeln!(out);

    out.push_str("bluetooth_state=\n");
    let _ = writeln!(out, "- bt_flags=0x{:02X}", report.bt_flags);
    let _ = writeln!(out, "- bt_started={}", report.bt_started);
    let _ = writeln!(out, "- bt_connected={}", report.bt_connected);
    let _ = writeln!(out, "- bt_send_requested={}", report.bt_send_requested);
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
    let _ = writeln!(out, "- pc_input_observed={}", report.pc_input_observed);
    let _ = writeln!(out, "- pc_input_evidence={}", report.pc_input_evidence);
    let _ = writeln!(out);

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
    out
}

pub(super) fn format_bluetooth_report_json(report: &BluetoothReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|e| {
        format!(
            "{{\"artifact_schema_version\":1,\"error\":\"bluetooth report serialization failed: {}\"}}\n",
            e
        )
    })
}

pub(super) fn bluetooth_usb_packets_stub(uid: &str, target: &cmd_run::PicoTarget) -> String {
    format!(
        "# Bluetooth mode USB packet capture\n\nuid={uid}\npersona={}\n\nBluetooth mode keeps the Pico plugged into this PC and streams controller input over USB CDC frames. The Pico then outputs Classic Bluetooth HID to the paired receiver.\n\nNo console USB adapter packet capture or persona survey was attempted for this Pico because Bluetooth mode does not use the Pico USB connector as a console-side controller output.\n",
        target.persona.label()
    )
}

pub(super) fn format_option_bool(value: Option<bool>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "not_captured".to_string())
}

pub(super) fn format_option_u32(value: Option<u32>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "not_captured".to_string())
}

pub(super) fn format_option_u16(value: Option<u16>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "not_captured".to_string())
}

pub(super) fn format_option_u8(value: Option<u8>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "not_captured".to_string())
}

pub(super) fn format_option_hex_u8(value: Option<u8>) -> String {
    value
        .map(|v| format!("0x{v:02X}"))
        .unwrap_or_else(|| "not_captured".to_string())
}

pub(super) fn format_option_hex_u16(value: Option<u16>) -> String {
    value
        .map(|v| format!("0x{v:04X}"))
        .unwrap_or_else(|| "not_captured".to_string())
}

pub(super) fn format_option_hid_report_type(value: Option<u8>) -> String {
    value
        .map(hid_report_type_name)
        .unwrap_or("not_captured")
        .to_string()
}

fn hid_report_type_name(report_type: u8) -> &'static str {
    match report_type {
        1 => "input",
        2 => "output",
        3 => "feature",
        _ => "unknown",
    }
}
pub(super) fn aggregate_bluetooth_report_text(captures: &[PicoBundleCapture]) -> String {
    let mut out = String::from("Aggregate Bluetooth output report\n\n");
    let mut count = 0usize;
    for capture in captures {
        if capture.bluetooth_report_text.is_empty() {
            continue;
        }
        count += 1;
        let _ = writeln!(
            out,
            "## uid={} path={}/bluetooth-report.txt",
            capture.manifest.uid, capture.manifest.path
        );
        out.push_str(&capture.bluetooth_report_text);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    if count == 0 {
        out.push_str("No live Bluetooth-mode Pico report was captured.\n");
    }
    out
}

#[derive(Serialize)]
pub(super) struct BluetoothBundleReport<'a> {
    pub(super) artifact_schema_version: u8,
    pub(super) report_count: usize,
    pub(super) per_pico: Vec<&'a BluetoothReport>,
    pub(super) notes: Vec<&'static str>,
}

pub(super) fn bluetooth_report_bundle_json(captures: &[PicoBundleCapture]) -> Result<String> {
    let per_pico = captures
        .iter()
        .filter_map(|capture| capture.bluetooth_report.as_ref())
        .collect::<Vec<_>>();
    let report = BluetoothBundleReport {
        artifact_schema_version: 1,
        report_count: per_pico.len(),
        per_pico,
        notes: vec![
            "Bluetooth reports are only present for live Pico boards in Bluetooth mode.",
            "Bluetooth mode uses USB CDC for live controller input from the bridge PC.",
            "Bluetooth mode skips the console USB adapter survey because the Pico USB connector stays plugged into the PC.",
        ],
    };
    Ok(serde_json::to_string_pretty(&report)?)
}
