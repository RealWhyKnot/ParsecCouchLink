mod evidence;
mod model;
mod text;
mod text_link;

use std::fmt::Write as _;

use anyhow::Result;
use serde::Serialize;

use crate::{cdc, cmd_run, protocol};

use super::PicoBundleCapture;
use evidence::{
    BluetoothContactState, BluetoothPcInputEvidence, bluetooth_diag_security_contact_seen,
    bluetooth_pc_input_evidence, bluetooth_receiver_contact, bluetooth_report_status, bt_cdc_u8,
    bt_cdc_u32,
};
pub(super) use evidence::{bluetooth_relevant_diag_lines, bluetooth_report_next_steps};
pub(super) use model::{
    BluetoothLinkPlan, BluetoothPcInput, BluetoothRadioFlags, BluetoothReport, BluetoothReportInput,
};
pub(super) use text::format_bluetooth_report_text;
use text_link::{
    write_bt_acl_l2cap, write_bt_reconnect, write_bt_security, write_diag_lines_steps_and_notes,
    write_pc_usb_input,
};

struct BluetoothVerdict {
    radio: BluetoothRadioFlags,
    bt_target: u8,
    bt_last_status: u8,
    bt_open_count: u32,
    bt_close_count: u32,
    bt_report_send_count: u32,
    pc_input: BluetoothPcInputEvidence,
    status: &'static str,
    bt_security_event_source: &'static str,
    bt_receiver_contact: &'static str,
}

fn bluetooth_verdict(
    target: &cmd_run::PicoTarget,
    input: &BluetoothReportInput<'_>,
) -> BluetoothVerdict {
    let pico_state = input.pico_state;
    let bt_status = input.bt_status;
    let usb_diag = input.usb_diag;
    let bt_flags = bt_status
        .map(|status| status.flags)
        .or_else(|| pico_state.map(|state| state.bt_flags))
        .unwrap_or(0);
    let radio = BluetoothRadioFlags {
        flags: bt_flags,
        started: bt_flags & protocol::BT_HID_STATUS_STARTED != 0,
        connected: bt_flags & protocol::BT_HID_STATUS_CONNECTED != 0,
        send_requested: bt_flags & protocol::BT_HID_STATUS_SEND_REQUESTED != 0,
    };
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
        &radio,
        bt_report_send_count,
        &pc_input,
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
    let bt_out_report_count = bt_status.map(|status| status.out_report_count);
    let bt_security_contact_from_cdc =
        bt_status.is_some_and(crate::cdc::BtStatus::pairing_security_contact_seen);
    let bt_security_contact_from_diag = bluetooth_diag_security_contact_seen(input.pico_diag_text);
    let bt_security_event_source = if bt_security_contact_from_cdc {
        "cdc_status"
    } else if bt_security_contact_from_diag {
        "diag_log"
    } else {
        "none"
    };
    let bt_reconnect_attempt_count = bt_status.map(|status| status.reconnect_attempt_count);
    let bt_reconnect_failed_count = bt_status.map(|status| status.reconnect_failed_count);
    let bt_reconnect_blocked_count = bt_status.map(|status| status.reconnect_blocked_count);
    let bt_incoming_l2cap_connection_count =
        bt_status.map(|status| status.incoming_l2cap_connection_count);
    let bt_receiver_contact = bluetooth_receiver_contact(&BluetoothContactState {
        radio: &radio,
        last_status: bt_last_status,
        open_count: bt_open_count,
        close_count: bt_close_count,
        hid_open_failed_count: bt_status.map_or(0, |status| status.hid_open_failed_count),
        security_contact_seen: bt_security_contact_from_cdc || bt_security_contact_from_diag,
        reconnect_waiting: bt_status.is_some_and(|status| {
            status.reconnect_pending()
                || status.reconnect_in_progress()
                || status.reconnect_activity_seen()
        }),
        reconnect_attempt_count: bt_reconnect_attempt_count.unwrap_or(0),
        reconnect_failed_count: bt_reconnect_failed_count.unwrap_or(0),
        reconnect_blocked_count: bt_reconnect_blocked_count.unwrap_or(0),
        incoming_l2cap_connection_count: bt_incoming_l2cap_connection_count.unwrap_or(0),
        get_report_count: bt_status.map_or(0, |status| status.get_report_count),
        set_report_count: bt_status.map_or(0, |status| status.set_report_count),
        out_report_count: bt_out_report_count.unwrap_or(0),
        ready_count: bt_status
            .map(|status| status.ready_count)
            .or_else(|| pico_state.map(|state| state.bt_ready_count))
            .unwrap_or(0),
    });
    BluetoothVerdict {
        radio,
        bt_target,
        bt_last_status,
        bt_open_count,
        bt_close_count,
        bt_report_send_count,
        pc_input,
        status,
        bt_security_event_source,
        bt_receiver_contact,
    }
}

fn fill_link_counters(
    report: &mut BluetoothReport,
    bt_status: Option<&cdc::BtStatus>,
    pico_state: Option<&protocol::PicoStateDiag>,
) {
    report.bt_report_len = bt_status
        .map(|status| status.report_len)
        .or_else(|| pico_state.map(|state| state.bt_report_len))
        .unwrap_or(0);
    report.bt_cid = bt_status
        .map(|status| status.cid)
        .or_else(|| pico_state.map(|state| state.bt_cid))
        .unwrap_or(0);
    report.bt_init_count = bt_status
        .map(|status| status.init_count)
        .or_else(|| pico_state.map(|state| state.bt_init_count))
        .unwrap_or(0);
    report.bt_ready_count = bt_status
        .map(|status| status.ready_count)
        .or_else(|| pico_state.map(|state| state.bt_ready_count))
        .unwrap_or(0);
    report.bt_can_send_count = bt_status
        .map(|status| status.can_send_count)
        .or_else(|| pico_state.map(|state| state.bt_can_send_count))
        .unwrap_or(0);
    report.bt_report_build_count = bt_status
        .map(|status| status.report_build_count)
        .or_else(|| pico_state.map(|state| state.bt_report_build_count))
        .unwrap_or(0);
    report.bt_send_request_count = bt_status
        .map(|status| status.send_request_count)
        .or_else(|| pico_state.map(|state| state.bt_send_request_count))
        .unwrap_or(0);
    report.bt_last_event_ms = bt_status
        .map(|status| status.last_event_ms)
        .or_else(|| pico_state.map(|state| state.bt_last_event_ms))
        .unwrap_or(0);
    report.bt_last_send_ms = bt_status
        .map(|status| status.last_send_ms)
        .or_else(|| pico_state.map(|state| state.bt_last_send_ms))
        .unwrap_or(0);
}

fn fill_control_plane(report: &mut BluetoothReport, bt_status: Option<&cdc::BtStatus>) {
    report.bt_reported_local_name = bt_status
        .map(|status| status.local_name.trim())
        .filter(|name| !name.is_empty())
        .map(std::string::ToString::to_string);
    report.bt_get_report_count = bt_status.map(|status| status.get_report_count);
    report.bt_get_report_success_count = bt_status.map(|status| status.get_report_success_count);
    report.bt_get_report_unsupported_count =
        bt_status.map(|status| status.get_report_unsupported_count);
    report.bt_set_report_count = bt_status.map(|status| status.set_report_count);
    report.bt_set_report_accepted_count = bt_status.map(|status| status.set_report_accepted_count);
    report.bt_set_report_unsupported_count =
        bt_status.map(|status| status.set_report_unsupported_count);
    report.bt_out_report_count = bt_status.map(|status| status.out_report_count);
    report.bt_out_report_accepted_count = bt_status.map(|status| status.out_report_accepted_count);
    report.bt_out_report_unsupported_count =
        bt_status.map(|status| status.out_report_unsupported_count);
    report.bt_last_get_report_id = bt_status.map(|status| status.last_get_report_id);
    report.bt_last_get_report_type = bt_status.map(|status| status.last_get_report_type);
    report.bt_last_set_report_id = bt_status.map(|status| status.last_set_report_id);
    report.bt_last_set_report_type = bt_status.map(|status| status.last_set_report_type);
    report.bt_last_out_report_id = bt_status.map(|status| status.last_out_report_id);
    report.bt_last_out_report_type = bt_status.map(|status| status.last_out_report_type);
    report.bt_last_get_report_len = bt_status.map(|status| status.last_get_report_len);
    report.bt_last_set_report_len = bt_status.map(|status| status.last_set_report_len);
    report.bt_last_out_report_len = bt_status.map(|status| status.last_out_report_len);
}

fn fill_security(report: &mut BluetoothReport, bt_status: Option<&cdc::BtStatus>) {
    report.bt_pin_code_request_count = bt_status.map(|status| status.pin_code_request_count);
    report.bt_pin_code_response_count = bt_status.map(|status| status.pin_code_response_count);
    report.bt_user_confirmation_request_count =
        bt_status.map(|status| status.user_confirmation_request_count);
    report.bt_user_confirmation_response_count =
        bt_status.map(|status| status.user_confirmation_response_count);
    report.bt_simple_pairing_complete_count =
        bt_status.map(|status| status.simple_pairing_complete_count);
    report.bt_authentication_complete_count =
        bt_status.map(|status| status.authentication_complete_count);
    report.bt_link_key_notification_count =
        bt_status.map(|status| status.link_key_notification_count);
    report.bt_encryption_change_count = bt_status.map(|status| status.encryption_change_count);
    report.bt_disconnection_complete_count =
        bt_status.map(|status| status.disconnection_complete_count);
    report.bt_hid_open_failed_count = bt_status.map(|status| status.hid_open_failed_count);
    report.bt_last_security_event_ms = bt_status.map(|status| status.last_security_event_ms);
    report.bt_last_simple_pairing_status =
        bt_status.map(|status| status.last_simple_pairing_status);
    report.bt_last_authentication_status =
        bt_status.map(|status| status.last_authentication_status);
    report.bt_last_encryption_status = bt_status.map(|status| status.last_encryption_status);
    report.bt_last_encryption_enabled = bt_status.map(|status| status.last_encryption_enabled);
    report.bt_last_disconnection_reason = bt_status.map(|status| status.last_disconnection_reason);
    report.bt_last_hid_open_status = bt_status.map(|status| status.last_hid_open_status);
}

fn fill_reconnect_and_l2cap(report: &mut BluetoothReport, bt_status: Option<&cdc::BtStatus>) {
    report.bt_reconnect_state = bt_status.map(|status| status.reconnect_state);
    report.bt_reconnect_cycle_attempts = bt_status.map(|status| status.reconnect_cycle_attempts);
    report.bt_last_reconnect_status = bt_status.map(|status| status.last_reconnect_status);
    report.bt_last_reconnect_reason = bt_status.map(|status| status.last_reconnect_reason);
    report.bt_reconnect_schedule_count = bt_status.map(|status| status.reconnect_schedule_count);
    report.bt_reconnect_attempt_count = bt_status.map(|status| status.reconnect_attempt_count);
    report.bt_reconnect_success_count = bt_status.map(|status| status.reconnect_success_count);
    report.bt_reconnect_failed_count = bt_status.map(|status| status.reconnect_failed_count);
    report.bt_reconnect_blocked_count = bt_status.map(|status| status.reconnect_blocked_count);
    report.bt_last_reconnect_ms = bt_status.map(|status| status.last_reconnect_ms);
    report.bt_connection_complete_count = bt_status.map(|status| status.connection_complete_count);
    report.bt_last_connection_complete_status =
        bt_status.map(|status| status.last_connection_complete_status);
    report.bt_last_connection_complete_link_type =
        bt_status.map(|status| status.last_connection_complete_link_type);
    report.bt_last_connection_complete_ms =
        bt_status.map(|status| status.last_connection_complete_ms);
    report.bt_incoming_l2cap_connection_count =
        bt_status.map(|status| status.incoming_l2cap_connection_count);
    report.bt_incoming_l2cap_hid_control_count =
        bt_status.map(|status| status.incoming_l2cap_hid_control_count);
    report.bt_incoming_l2cap_hid_interrupt_count =
        bt_status.map(|status| status.incoming_l2cap_hid_interrupt_count);
    report.bt_last_incoming_l2cap_psm = bt_status.map(|status| status.last_incoming_l2cap_psm);
    report.bt_last_incoming_l2cap_local_cid =
        bt_status.map(|status| status.last_incoming_l2cap_local_cid);
    report.bt_last_incoming_l2cap_ms = bt_status.map(|status| status.last_incoming_l2cap_ms);
}

fn fill_cdc_and_usb(
    report: &mut BluetoothReport,
    bt_status: Option<&cdc::BtStatus>,
    usb_diag: Option<&protocol::UsbDiag>,
) {
    report.bt_cdc_state_count = bt_cdc_u32(bt_status, |status| status.bt_cdc_state_count);
    report.bt_cdc_heartbeat_count = bt_cdc_u32(bt_status, |status| status.bt_cdc_heartbeat_count);
    report.bt_cdc_bad_length_count = bt_cdc_u32(bt_status, |status| status.bt_cdc_bad_length_count);
    report.bt_cdc_rejected_count = bt_cdc_u32(bt_status, |status| status.bt_cdc_rejected_count);
    report.bt_cdc_last_frame_ms = bt_cdc_u32(bt_status, |status| status.bt_cdc_last_frame_ms);
    report.bt_cdc_last_state_ms = bt_cdc_u32(bt_status, |status| status.bt_cdc_last_state_ms);
    report.bt_cdc_last_heartbeat_ms =
        bt_cdc_u32(bt_status, |status| status.bt_cdc_last_heartbeat_ms);
    report.bt_cdc_last_seq = bt_cdc_u8(bt_status, |status| status.bt_cdc_last_seq);
    report.bt_cdc_last_command = bt_cdc_u8(bt_status, |status| status.bt_cdc_last_command);
    report.bt_cdc_last_flags = bt_cdc_u8(bt_status, |status| status.bt_cdc_last_flags);
    report.usb_mounted = usb_diag.map(super::super::protocol::usb_diag::UsbDiag::mounted);
    report.usb_suspended = usb_diag.map(super::super::protocol::usb_diag::UsbDiag::suspended);
    report.usb_mount_count = usb_diag.map(|diag| diag.mount_count);
    report.usb_umount_count = usb_diag.map(|diag| diag.umount_count);
    report.usb_device_desc_count = usb_diag.map(|diag| diag.device_desc_count);
    report.usb_config_desc_count = usb_diag.map(|diag| diag.config_desc_count);
    report.usb_input_queued_count = usb_diag.map(|diag| diag.xinput_in_queued_count);
    report.usb_input_sent_count = usb_diag.map(|diag| diag.xinput_in_sent_count);
    report.usb_host_out_count = usb_diag.map(|diag| diag.xinput_out_count);
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
    let BluetoothVerdict {
        radio,
        bt_target,
        bt_last_status,
        bt_open_count,
        bt_close_count,
        bt_report_send_count,
        pc_input,
        status,
        bt_security_event_source,
        bt_receiver_contact,
    } = bluetooth_verdict(target, &input);
    let mut report = BluetoothReport {
        artifact_schema_version: 6,
        uid: uid.to_string(),
        path: path.to_string(),
        peer: Some(target.peer.to_string()),
        live: true,
        persona: target.persona.label().to_string(),
        target_label: protocol::bt_hid_target_label(bt_target).to_string(),
        advertised_name: bluetooth_advertised_name(bt_target),
        plan: BluetoothLinkPlan {
            expected_connection: "pc_usb_input_bluetooth_output",
            usb_input_required: true,
            usb_transport: "cdc_framed_controller_state",
            bluetooth_output: "classic_bluetooth_hid_gamepad",
        },
        pico_state_captured: pico_state.is_some().into(),
        usb_diag_captured: usb_diag.is_some().into(),
        bt_status_cdc_captured: bt_status.is_some().into(),
        bt_status_cdc_error: input.bt_status_error,
        bt_status_version: bt_status.map(|status| status.status_version),
        bt_decoded_status_version: bt_status.map(|status| status.decoded_status_version),
        bt_status_newer_than_host: bt_status
            .is_some_and(crate::cdc::BtStatus::newer_status_version),
        pico_diag_captured: (!input.pico_diag_text.trim().is_empty()).into(),
        status,
        warning: status != "reports_sent",
        bt_receiver_contact,
        radio,
        bt_target,
        bt_last_status,
        bt_open_count,
        bt_close_count,
        bt_report_send_count,
        pc_input: BluetoothPcInput {
            pc_input_observed: pc_input.observed,
            pc_input_evidence: pc_input.evidence,
        },
        bt_security_event_source,
        bt_cdc_input_status: pc_input.bt_cdc_input_status,
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
        ..BluetoothReport::default()
    };
    fill_link_counters(&mut report, bt_status, pico_state);
    fill_control_plane(&mut report, bt_status);
    fill_security(&mut report, bt_status);
    fill_reconnect_and_l2cap(&mut report, bt_status);
    fill_cdc_and_usb(&mut report, bt_status, usb_diag);
    report
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

pub(super) fn format_bluetooth_report_json(report: &BluetoothReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|e| {
        format!(
            "{{\"artifact_schema_version\":1,\"error\":\"bluetooth report serialization failed: {e}\"}}\n"
        )
    })
}

pub(super) fn bluetooth_usb_packets_stub(uid: &str, target: &cmd_run::PicoTarget) -> String {
    format!(
        "# Bluetooth mode USB packet capture\n\nuid={uid}\npersona={}\n\nBluetooth mode keeps the Pico plugged into this PC and streams controller input over USB CDC frames. The Pico then outputs Classic Bluetooth HID to the paired receiver.\n\nNo console USB adapter packet capture or persona survey was attempted for this Pico because Bluetooth mode does not use the Pico USB connector as a console-side controller output.\n",
        target.persona.label()
    )
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
