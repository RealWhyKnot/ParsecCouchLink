use super::BluetoothRadioFlags;
use crate::{cdc, protocol};

pub(super) struct BluetoothContactState<'a> {
    pub(super) radio: &'a BluetoothRadioFlags,
    pub(super) last_status: u8,
    pub(super) open_count: u32,
    pub(super) close_count: u32,
    pub(super) hid_open_failed_count: u32,
    pub(super) security_contact_seen: bool,
    pub(super) reconnect_waiting: bool,
    pub(super) reconnect_attempt_count: u32,
    pub(super) reconnect_failed_count: u32,
    pub(super) reconnect_blocked_count: u32,
    pub(super) incoming_l2cap_connection_count: u32,
    pub(super) get_report_count: u32,
    pub(super) set_report_count: u32,
    pub(super) out_report_count: u32,
    pub(super) ready_count: u32,
}

pub(super) fn bluetooth_receiver_contact(state: &BluetoothContactState<'_>) -> &'static str {
    if !state.radio.started {
        "radio_not_started"
    } else if state.radio.connected
        || state.open_count > 0
        || state.close_count > 0
        || state.get_report_count > 0
        || state.set_report_count > 0
        || state.out_report_count > 0
    {
        "hid_receiver_contact_seen"
    } else if state.hid_open_failed_count > 0 || state.last_status != 0 {
        "hid_open_failed"
    } else if state.incoming_l2cap_connection_count > 0 {
        "hid_l2cap_incoming_no_hid_open"
    } else if state.reconnect_attempt_count > 0
        || state.reconnect_failed_count > 0
        || state.reconnect_blocked_count > 0
    {
        "hid_reconnect_attempted_no_hid_open"
    } else if state.reconnect_waiting {
        "hid_reconnect_pending"
    } else if state.security_contact_seen {
        "pairing_security_contact_no_hid_open"
    } else if state.ready_count > 0 {
        "discoverable_no_hid_contact"
    } else {
        "stack_started_no_ready_event"
    }
}

pub(super) fn bluetooth_diag_security_contact_seen(text: &str) -> bool {
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
    radio: &BluetoothRadioFlags,
    bt_report_send_count: u32,
    pc_input: &BluetoothPcInputEvidence,
) -> &'static str {
    if !state_captured {
        "pico_state_missing"
    } else if !radio.started {
        "bluetooth_stack_not_started"
    } else if !radio.connected {
        "waiting_for_receiver"
    } else if bt_report_send_count == 0 {
        "connected_waiting_for_input"
    } else if !pc_input.observed {
        match pc_input.bt_cdc_input_status {
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

pub(super) struct BluetoothPcInputEvidence {
    pub(super) observed: bool,
    pub(super) evidence: &'static str,
    pub(super) bt_cdc_input_status: &'static str,
}

pub(super) fn bluetooth_pc_input_evidence(
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
    if pico_state.is_some_and(|state| {
        state.last_bridge_packet_ms > 0
            || state.xinput_in_queued_count > 0
            || state.xinput_in_sent_count > 0
    }) {
        return BluetoothPcInputEvidence {
            observed: true,
            evidence: "pico_state_input",
            bt_cdc_input_status,
        };
    }
    if usb_diag.is_some_and(|diag| {
        diag.last_bridge_packet_ms > 0
            || diag.xinput_in_queued_count > 0
            || diag.xinput_in_sent_count > 0
    }) {
        return BluetoothPcInputEvidence {
            observed: true,
            evidence: "usb_diag_input",
            bt_cdc_input_status,
        };
    }
    if matches!(bt_cdc_input_status, "not_captured" | "not_captured_pre_v5")
        && let Some(status) = legacy_stream_status(pico_state, usb_diag)
    {
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
    BluetoothPcInputEvidence {
        observed: false,
        evidence: "none",
        bt_cdc_input_status,
    }
}

pub(super) fn legacy_stream_status(
    pico_state: Option<&protocol::PicoStateDiag>,
    usb_diag: Option<&protocol::UsbDiag>,
) -> Option<&'static str> {
    if pico_state.is_none() && usb_diag.is_none() {
        return None;
    }
    let bridge_peer = pico_state
        .is_some_and(|state| state.activity_flags & protocol::USB_DIAG_ACTIVITY_PEER != 0)
        || usb_diag.is_some_and(protocol::usb_diag::UsbDiag::bridge_peer_latched);
    if !bridge_peer {
        return Some("host_stream_not_active");
    }
    let source_connected = pico_state
        .is_some_and(|state| state.activity_flags & protocol::USB_DIAG_ACTIVITY_PARSEC != 0)
        || usb_diag.is_some_and(protocol::usb_diag::UsbDiag::parsec_connected);
    Some(if source_connected {
        "source_idle"
    } else {
        "source_never_connected"
    })
}

pub(super) fn bt_cdc_u32(
    bt_status: Option<&cdc::BtStatus>,
    read: impl FnOnce(&cdc::BtStatus) -> u32,
) -> Option<u32> {
    bt_status
        .filter(|status| status.bt_cdc_counters_captured())
        .map(read)
}

pub(super) fn bt_cdc_u8(
    bt_status: Option<&cdc::BtStatus>,
    read: impl FnOnce(&cdc::BtStatus) -> u8,
) -> Option<u8> {
    bt_status
        .filter(|status| status.bt_cdc_counters_captured())
        .map(read)
}

pub(in crate::cmd_bundle) fn bluetooth_report_next_steps(
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
        "waiting_for_receiver" if bt_receiver_contact == "hid_reconnect_attempted_no_hid_open" => {
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

pub(in crate::cmd_bundle) fn bluetooth_relevant_diag_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            line.contains("bt_hid:")
                || line.contains("Bluetooth")
                || line.contains("CDC")
                || line.contains("cdc:")
                || line.contains("usb_init:")
                || line.contains("run:")
        })
        .map(std::string::ToString::to_string)
        .collect()
}
