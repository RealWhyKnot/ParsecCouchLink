use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};

use crate::cdc;
use crate::protocol::{Packet, PacketKind, Persona};

use super::{RouteRuntime, StreamRoute};

pub(in crate::cmd_run) fn open_bluetooth_usb_links(
    routes: &[StreamRoute],
    quiet: bool,
) -> Result<HashMap<u32, cdc::PicoSetup>> {
    let needed: HashSet<u32> = routes
        .iter()
        .filter(|route| route.pico.persona.is_bluetooth())
        .map(|route| route.pico.info.unique_id_short)
        .collect();
    if needed.is_empty() {
        return Ok(HashMap::new());
    }

    let ports = cdc::find_setup_ports().context(
        "Bluetooth mode requires the Pico to be plugged into this PC over USB; could not enumerate local CouchLink USB diagnostic ports",
    )?;
    let mut found = HashMap::new();
    let mut probe_errors = Vec::new();
    for port in ports {
        match cdc::PicoSetup::open_named(&port).and_then(|mut pico| {
            let uid = pico.unique_id_short()?;
            Ok((uid, pico))
        }) {
            Ok((uid, pico)) if needed.contains(&uid) => {
                if !quiet {
                    outln!(
                        "Bluetooth USB link ready: Pico {uid:08X} on {}.",
                        pico.port_name()
                    );
                }
                found.insert(uid, pico);
            }
            Ok((_uid, _pico)) => {}
            Err(e) => probe_errors.push(format!("{port}: {e:#}")),
        }
    }

    let missing = needed
        .iter()
        .filter(|uid| !found.contains_key(uid))
        .map(|uid| format!("{uid:08X}"))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        let mut msg = format!(
            "Bluetooth mode will not stream to a Wi-Fi-only Pico. Plug Pico {} into this PC over USB, wait for the CouchLink USB diagnostic device, then run the command again.",
            missing.join(", ")
        );
        msg.push_str(" Expected USB identity: VID 0x2E8A PID 0xCAF0.");
        if !probe_errors.is_empty() {
            msg.push_str(" Local USB probe errors: ");
            msg.push_str(&probe_errors.join(" | "));
        }
        bail!("{msg}");
    }

    Ok(found)
}

pub(in crate::cmd_run) fn bluetooth_cdc_frame_from_packet(
    packet: &Packet,
) -> Result<(u8, [u8; 13])> {
    let command = match packet.kind {
        PacketKind::State(_) => cdc::CMD_BT_STATE,
        PacketKind::Heartbeat(_) => cdc::CMD_BT_HEARTBEAT,
        _ => bail!("Bluetooth USB streaming only accepts controller state packets"),
    };
    let encoded = packet.encode();
    let mut payload = [0u8; 13];
    payload[0] = encoded[3];
    payload[1..].copy_from_slice(&encoded[4..16]);
    Ok((command, payload))
}

pub(in crate::cmd_run) fn is_usb_output_persona(persona: Persona) -> bool {
    matches!(
        persona,
        Persona::Xinput
            | Persona::Maple
            | Persona::Ps3
            | Persona::Ps4
            | Persona::XboxOne
            | Persona::GenericHid
            | Persona::Debug
    )
}

pub(in crate::cmd_run) fn refresh_bluetooth_statuses(routes: &mut [RouteRuntime]) {
    for route in routes {
        route.refresh_bluetooth_status();
    }
}

pub(crate) fn print_bluetooth_pairing_help(persona: Persona) {
    if !persona.is_bluetooth() {
        return;
    }
    let expected_name = bluetooth_expected_name(persona);
    outln!();
    outln!("Bluetooth mode setup");
    outln!("  Keep this Pico plugged into the bridge PC over USB.");
    outln!("  The Pico will advertise as {expected_name}.");
    outln!("  Put the receiver or console adapter into Bluetooth pairing/search mode.");
    outln!("  Pair the receiver with {expected_name}. Use PIN 0000 if it asks for one.");
    outln!("  This command keeps streaming status and controller input until you stop it.");
    match persona {
        Persona::BluetoothPlaystation => {
            outln!(
                "  For BITFUNX/BlueRetro N64, try this DualShock 4 mode first; use couchlink blueretro as the fallback."
            );
        }
        Persona::BluetoothHid => {
            outln!(
                "  For BITFUNX/BlueRetro N64, couchlink blueretro-playstation is the first choice; this generic HID mode is the fallback."
            );
        }
        Persona::BluetoothXbox => {
            outln!(
                "  For BITFUNX/BlueRetro N64, try couchlink blueretro-playstation first, then couchlink blueretro; this Xbox-named Classic HID mimic is only a diagnostic."
            );
        }
        _ => {}
    }
    outln!("  Persona switching still uses Wi-Fi; live controller input then uses PC USB.");
}

pub(in crate::cmd_run) fn bluetooth_expected_name(persona: Persona) -> &'static str {
    match persona {
        Persona::BluetoothXbox => "Xbox Wireless Controller",
        Persona::BluetoothPlaystation => "Wireless Controller",
        _ => "CouchLink BT HID",
    }
}

fn hid_report_type_name(report_type: u8) -> &'static str {
    match report_type {
        1 => "input",
        2 => "output",
        3 => "feature",
        _ => "unknown",
    }
}

pub(in crate::cmd_run) fn format_bluetooth_peer_state(
    status: Option<&cdc::BtStatus>,
    report_delta: Option<u32>,
    unsupported: bool,
    error: Option<&str>,
) -> String {
    if unsupported {
        return "status unavailable: update Pico firmware to show receiver pairing state"
            .to_string();
    }
    if let Some(error) = error {
        return format!("status unavailable: {error}");
    }
    let Some(status) = status else {
        return "status pending".to_string();
    };
    if !status.started() {
        return "radio starting".to_string();
    }
    let version_note = if status.newer_status_version() {
        format!(
            "newer BT_STATUS v{} decoded as v{}; update CouchLink; ",
            status.status_version, status.decoded_status_version
        )
    } else {
        String::new()
    };
    if !status.connected() {
        return disconnected_peer_state(status, &version_note);
    }

    let mut msg = match report_delta {
        Some(delta) => format!(
            "receiver connected; HID report len {}; reports +{} total {}",
            status.report_len, delta, status.report_send_count
        ),
        None => format!(
            "receiver connected; HID report len {}; reports total {}",
            status.report_len, status.report_send_count
        ),
    };
    if status.send_requested() {
        msg.push_str("; send queued");
    }
    write_report_exchange_counts(&mut msg, status);
    write_cdc_input_counts(&mut msg, status);
    if status.close_count > 0 {
        let _ = write!(msg, "; disconnects {}", status.close_count);
    }
    if status.newer_status_version() {
        let _ = write!(
            msg,
            "; newer BT_STATUS v{} decoded as v{}; update CouchLink",
            status.status_version, status.decoded_status_version
        );
    }
    msg
}

fn disconnected_peer_state(status: &cdc::BtStatus, version_note: &str) -> String {
    let name = bluetooth_display_name(status);
    if status.incoming_hid_l2cap_seen() {
        return l2cap_contact_state(status, version_note, name);
    }
    if status.reconnect_pending() || status.reconnect_in_progress() {
        return reconnect_pending_state(status, version_note, name);
    }
    if status.reconnect_activity_seen() {
        return reconnect_attempted_state(status, version_note, name);
    }
    if status.pairing_security_contact_seen() || status.hid_open_failed_count > 0 {
        return pairing_contact_state(status, version_note, name);
    }
    discoverable_state(status, version_note, name)
}

fn l2cap_contact_state(status: &cdc::BtStatus, version_note: &str, name: &str) -> String {
    let mut msg = format!(
        "{version_note}receiver reached HID L2CAP for \"{name}\" but no Classic HID channel opened"
    );
    let _ = write!(
        msg,
        "; incoming control {} interrupt {} last PSM 0x{:04X}",
        status.incoming_l2cap_hid_control_count,
        status.incoming_l2cap_hid_interrupt_count,
        status.last_incoming_l2cap_psm
    );
    if status.last_connection_complete_status != 0 {
        let _ = write!(
            msg,
            "; ACL status 0x{:02X}",
            status.last_connection_complete_status
        );
    }
    msg.push_str("; keep bundle with receiver-side logs if it repeats");
    msg
}

fn reconnect_pending_state(status: &cdc::BtStatus, version_note: &str, name: &str) -> String {
    let phase = if status.reconnect_in_progress() {
        "in progress"
    } else {
        "scheduled"
    };
    let mut msg =
        format!("{version_note}pairing/security seen for \"{name}\"; HID reconnect {phase}");
    let _ = write!(
        msg,
        "; attempts {}/6 scheduled {}",
        status.reconnect_cycle_attempts, status.reconnect_schedule_count
    );
    if status.connection_complete_count > 0 {
        let _ = write!(
            msg,
            "; ACL completes {} last 0x{:02X}",
            status.connection_complete_count, status.last_connection_complete_status
        );
    }
    msg
}

fn reconnect_attempted_state(status: &cdc::BtStatus, version_note: &str, name: &str) -> String {
    let mut msg = format!(
        "{version_note}pairing/security seen for \"{name}\"; HID reconnect attempts {} failed {} blocked {}; no Classic HID channel opened",
        status.reconnect_attempt_count,
        status.reconnect_failed_count,
        status.reconnect_blocked_count
    );
    if status.connection_complete_count > 0 {
        let _ = write!(
            msg,
            "; ACL completes {} last status 0x{:02X}",
            status.connection_complete_count, status.last_connection_complete_status
        );
    }
    if status.last_reconnect_status != 0 {
        let _ = write!(
            msg,
            "; last reconnect status 0x{:02X}",
            status.last_reconnect_status
        );
    }
    msg.push_str("; BITFUNX/BlueRetro N64: try couchlink blueretro-playstation first, then couchlink blueretro");
    msg
}

fn pairing_contact_state(status: &cdc::BtStatus, version_note: &str, name: &str) -> String {
    let mut msg = format!(
        "{version_note}pairing/security seen for \"{name}\" but no Classic HID channel opened; clear receiver pairing and pair again"
    );
    msg.push_str("; BITFUNX/BlueRetro N64: try couchlink blueretro-playstation first, then couchlink blueretro");
    if status.user_confirmation_request_count > 0 {
        let _ = write!(
            msg,
            "; confirmations {}/{}",
            status.user_confirmation_response_count, status.user_confirmation_request_count
        );
    }
    if status.pin_code_request_count > 0 {
        let _ = write!(
            msg,
            "; PIN replies {}/{}",
            status.pin_code_response_count, status.pin_code_request_count
        );
    }
    if status.hid_open_failed_count > 0 {
        let _ = write!(
            msg,
            "; HID open failures {} last 0x{:02X}",
            status.hid_open_failed_count, status.last_hid_open_status
        );
    }
    if status.last_authentication_status != 0 {
        let _ = write!(
            msg,
            "; auth status 0x{:02X}",
            status.last_authentication_status
        );
    }
    if status.last_disconnection_reason != 0 {
        let _ = write!(
            msg,
            "; disconnect reason 0x{:02X}",
            status.last_disconnection_reason
        );
    }
    msg
}

fn discoverable_state(status: &cdc::BtStatus, version_note: &str, name: &str) -> String {
    let mut msg =
        format!("{version_note}discoverable as \"{name}\"; pair receiver/search mode, PIN 0000");
    if status.last_status != 0 {
        let _ = write!(msg, "; last status 0x{:02X}", status.last_status);
    }
    if status.close_count > 0 {
        let _ = write!(msg, "; disconnects {}", status.close_count);
    }
    msg
}

fn write_report_exchange_counts(msg: &mut String, status: &cdc::BtStatus) {
    if status.get_report_count > 0 {
        let _ = write!(
            msg,
            "; GET_REPORT ok {}/{}",
            status.get_report_success_count, status.get_report_count
        );
        if status.get_report_unsupported_count > 0 {
            let _ = write!(msg, " rejected {}", status.get_report_unsupported_count);
        }
        if status.last_get_report_len > 0 {
            let _ = write!(
                msg,
                "; last GET {} 0x{:02X} len {}",
                hid_report_type_name(status.last_get_report_type),
                status.last_get_report_id,
                status.last_get_report_len
            );
        }
    }
    if status.set_report_count > 0 {
        let _ = write!(
            msg,
            "; SET_REPORT accepted {}/{}",
            status.set_report_accepted_count, status.set_report_count
        );
        if status.set_report_unsupported_count > 0 {
            let _ = write!(msg, " ignored {}", status.set_report_unsupported_count);
        }
        if status.last_set_report_len > 0 {
            let _ = write!(
                msg,
                "; last SET {} 0x{:02X} len {}",
                hid_report_type_name(status.last_set_report_type),
                status.last_set_report_id,
                status.last_set_report_len
            );
        }
    }
    if status.out_report_count > 0 {
        let _ = write!(
            msg,
            "; interrupt OUT accepted {}/{}",
            status.out_report_accepted_count, status.out_report_count
        );
        if status.out_report_unsupported_count > 0 {
            let _ = write!(msg, " ignored {}", status.out_report_unsupported_count);
        }
        if status.last_out_report_len > 0 {
            let _ = write!(
                msg,
                "; last OUT {} 0x{:02X} len {}",
                hid_report_type_name(status.last_out_report_type),
                status.last_out_report_id,
                status.last_out_report_len
            );
        }
    }
}

fn write_cdc_input_counts(msg: &mut String, status: &cdc::BtStatus) {
    if status.bt_cdc_input_seen() {
        let _ = write!(
            msg,
            "; PC CDC input state {} heartbeat {}",
            status.bt_cdc_state_count, status.bt_cdc_heartbeat_count
        );
        if status.bt_cdc_last_frame_ms > 0 {
            let _ = write!(
                msg,
                " last cmd 0x{:02X} seq {} flags 0x{:02X}",
                status.bt_cdc_last_command, status.bt_cdc_last_seq, status.bt_cdc_last_flags
            );
        }
    } else {
        msg.push_str("; no PC CDC input frames yet");
    }
    if status.bt_cdc_bad_length_count > 0 || status.bt_cdc_rejected_count > 0 {
        let _ = write!(
            msg,
            "; CDC input errors bad_len {} rejected {}",
            status.bt_cdc_bad_length_count, status.bt_cdc_rejected_count
        );
    }
}

fn bluetooth_display_name(status: &cdc::BtStatus) -> &str {
    if status.local_name.is_empty() {
        "CouchLink BT HID"
    } else {
        &status.local_name
    }
}

pub(in crate::cmd_run) fn should_print_bluetooth_pairing_hint(
    status: Option<&cdc::BtStatus>,
) -> bool {
    status.is_none_or(|status| !status.connected())
}

pub(in crate::cmd_run) fn short_error(error: &anyhow::Error) -> String {
    const MAX_LEN: usize = 120;
    let text = error.to_string();
    if text.len() <= MAX_LEN {
        text
    } else {
        let prefix: String = text.chars().take(MAX_LEN).collect();
        format!("{prefix}...")
    }
}
