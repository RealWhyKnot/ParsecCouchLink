use super::bluetooth::{bluetooth_cdc_frame_from_packet, short_error};
use super::{
    BluetoothSourceSlotDecision, StreamRoute, bluetooth_source_slot_decision,
    format_xinput_snapshot,
};
use crate::protocol::{self, FLAG_PARSEC_CONNECTED, GamepadState, Packet, Persona};
use crate::{cdc, keyboard, udp_auth, xinput};
use anyhow::{Result, anyhow};
use std::time::Instant;
use tokio::net::UdpSocket;

pub(super) struct RouteRuntime {
    pub(super) route: StreamRoute,
    bluetooth_usb: Option<cdc::PicoSetup>,
    pub(super) bluetooth_status: Option<cdc::BtStatus>,
    pub(super) bluetooth_report_delta: Option<u32>,
    pub(super) bluetooth_status_unsupported: bool,
    pub(super) bluetooth_status_error: Option<String>,
    seq: u8,
    pub(super) sent_total: u64,
    pub(super) sent_at_last_status: u64,
    pub(super) inbound_total: u64,
    pub(super) last_inbound: Instant,
    pub(super) last_state: GamepadState,
    pub(super) last_key: protocol::KeyboardReport,
    last_packet_number: Option<u32>,
    pub(super) source_connected: bool,
    allow_source_auto_switch: bool,
    pub(super) source_auto_switch_count: u32,
    pub(super) last_source_auto_switch: Option<String>,
    pub(super) last_send_type: &'static str,
    pub(super) last_recovery_attempt: Option<Instant>,
    pub(super) hints: HintsPrinted,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HintsPrinted {
    pub(super) recovery: bool,
    pub(super) bluetooth_pairing: bool,
}

impl RouteRuntime {
    pub(super) fn new(
        route: StreamRoute,
        bluetooth_usb: Option<cdc::PicoSetup>,
        allow_source_auto_switch: bool,
    ) -> Self {
        Self {
            route,
            bluetooth_usb,
            bluetooth_status: None,
            bluetooth_report_delta: None,
            bluetooth_status_unsupported: false,
            bluetooth_status_error: None,
            seq: 0,
            sent_total: 0,
            sent_at_last_status: 0,
            inbound_total: 0,
            last_inbound: Instant::now(),
            last_state: GamepadState::default(),
            last_key: protocol::KeyboardReport::default(),
            last_packet_number: None,
            source_connected: false,
            allow_source_auto_switch,
            source_auto_switch_count: 0,
            last_source_auto_switch: None,
            last_send_type: "heartbeat",
            last_recovery_attempt: None,
            hints: HintsPrinted::default(),
        }
    }

    pub(super) async fn send_udp_tick(&mut self, socket: &UdpSocket) -> std::io::Result<()> {
        let packet = match self.route.pico.persona {
            Persona::Xinput
            | Persona::Maple
            | Persona::Ps3
            | Persona::Ps4
            | Persona::XboxOne
            | Persona::GenericHid
            | Persona::Debug => self.next_controller_packet(),
            Persona::Keyboard => self.next_keyboard_packet(),
            Persona::BluetoothHid | Persona::BluetoothXbox | Persona::BluetoothPlaystation => {
                self.next_controller_packet()
            }
        };
        self.seq = self.seq.wrapping_add(1);
        let bytes = udp_auth::seal(self.route.pico.peer, &packet.encode());
        socket.send_to(&bytes, self.route.pico.peer).await?;
        self.sent_total += 1;
        Ok(())
    }

    pub(super) fn send_bluetooth_usb_tick(&mut self) -> Result<()> {
        let packet = self.next_controller_packet();
        let (command, payload) = bluetooth_cdc_frame_from_packet(&packet)?;
        let link = self.bluetooth_usb.as_mut().ok_or_else(|| {
            anyhow!(
                "Bluetooth mode requires Pico {} to be plugged into this PC over USB; no matching CouchLink USB diagnostic port is open",
                self.route.pico.uid_hex()
            )
        })?;
        link.write_frame_no_response(command, self.seq, &payload)?;
        self.seq = self.seq.wrapping_add(1);
        self.sent_total += 1;
        Ok(())
    }

    pub(super) fn refresh_bluetooth_status(&mut self) {
        if !self.route.pico.persona.is_bluetooth() || self.bluetooth_status_unsupported {
            return;
        }
        let Some(link) = self.bluetooth_usb.as_mut() else {
            self.bluetooth_status_error = Some("Pico USB diagnostic port is not open".to_string());
            return;
        };
        match link.bt_status() {
            Ok(status) => {
                self.bluetooth_report_delta = self.bluetooth_status.as_ref().map(|previous| {
                    status
                        .report_send_count
                        .saturating_sub(previous.report_send_count)
                });
                self.bluetooth_status = Some(status);
                self.bluetooth_status_error = None;
            }
            Err(e) if cdc::error_has_nack_code(&e, cdc::ERR_UNKNOWN_COMMAND) => {
                self.bluetooth_status_unsupported = true;
                self.bluetooth_status_error = None;
            }
            Err(e) => {
                self.bluetooth_status_error = Some(short_error(&e));
            }
        }
    }

    fn next_controller_packet(&mut self) -> Packet {
        let source = xinput::read_slot(self.route.source_slot)
            .or_else(|| self.try_auto_switch_bluetooth_source());
        let (state, packet_number, connected) = match source {
            Some(snapshot) => (snapshot.state, Some(snapshot.packet_number), true),
            None => (GamepadState::default(), None, false),
        };
        let changed = connected != self.source_connected
            || state != self.last_state
            || packet_number != self.last_packet_number;
        let flags = if connected { FLAG_PARSEC_CONNECTED } else { 0 };
        let packet = if changed {
            self.last_send_type = "state";
            Packet::state(self.seq, flags, state)
        } else {
            self.last_send_type = "heartbeat";
            Packet::heartbeat(self.seq, flags, state)
        };
        self.last_state = state;
        self.last_packet_number = packet_number;
        self.source_connected = connected;
        packet
    }

    fn try_auto_switch_bluetooth_source(&mut self) -> Option<xinput::SlotSnapshot> {
        if !self.allow_source_auto_switch || !self.route.pico.persona.is_bluetooth() {
            return None;
        }
        let connected = xinput::connected_slots();
        let BluetoothSourceSlotDecision::AutoSwitch { from, to } =
            bluetooth_source_slot_decision(self.route.source_slot, &connected, true)
        else {
            return None;
        };
        let snapshot = connected
            .iter()
            .find(|snapshot| snapshot.slot == to)
            .copied()?;
        self.route.source_slot = to;
        self.source_auto_switch_count = self.source_auto_switch_count.saturating_add(1);
        self.last_source_auto_switch = Some(format!(
            "{} -> {}",
            xinput::user_slot_label(from),
            xinput::user_slot_label(to)
        ));
        self.last_packet_number = None;
        self.source_connected = false;
        tracing::warn!(
            "stream: Bluetooth source auto-switched during stream pico={} from {} to {} packet={} {}",
            self.route.pico.uid_hex(),
            xinput::user_slot_label(from),
            xinput::user_slot_label(to),
            snapshot.packet_number,
            format_xinput_snapshot(&snapshot)
        );
        Some(snapshot)
    }

    fn next_keyboard_packet(&mut self) -> Packet {
        let report = keyboard::read_keyboard();
        let changed = !self.source_connected || report != self.last_key;
        let packet = if changed {
            self.last_send_type = "keys";
            Packet::key_state(self.seq, FLAG_PARSEC_CONNECTED, report)
        } else {
            self.last_send_type = "key-heartbeat";
            Packet::key_heartbeat(self.seq, FLAG_PARSEC_CONNECTED, report)
        };
        self.last_key = report;
        self.source_connected = true;
        packet
    }
}
