use super::bluetooth::{
    bluetooth_expected_name, format_bluetooth_peer_state, is_usb_output_persona,
    should_print_bluetooth_pairing_hint,
};
use super::route_runtime::RouteRuntime;
use super::{PEER_STALE_AFTER, StreamRoute};
use crate::pico_cache;
use crate::protocol::Persona;
use std::net::SocketAddr;
use std::time::Duration;

pub(super) fn print_stream_intro(routes: &[StreamRoute], local_addr: SocketAddr) {
    outln!();
    outln!("CouchLink streaming");
    let udp_routes = routes
        .iter()
        .filter(|route| !route.pico.persona.is_bluetooth())
        .count();
    let bluetooth_routes = routes.len().saturating_sub(udp_routes);
    if udp_routes > 0 {
        outln!("Local UDP socket: {local_addr}");
    }
    if bluetooth_routes > 0 {
        outln!("Bluetooth path: source controller -> PC USB -> Pico -> Bluetooth receiver.");
        outln!(
            "  Pair the receiver with the Pico's CouchLink Bluetooth gamepad; PIN is 0000 if requested."
        );
        outln!("  Status below reports both the USB input link and the Bluetooth receiver state.");
    }
    if routes
        .iter()
        .any(|route| is_usb_output_persona(route.pico.persona))
    {
        outln!(
            "USB-output note: Wi-Fi counters prove PC-to-Pico input, not console-adapter acceptance."
        );
        outln!("  If the console sees no input, run `couchlink test usb` or `couchlink bundle`.");
    }
    if routes
        .iter()
        .any(|route| route.pico.persona == Persona::Keyboard)
    {
        outln!(
            "Keyboard mode: only remote Parsec-injected keystrokes are relayed; empty keys means no captured typing yet."
        );
    }
    if routes
        .iter()
        .any(|route| route.pico.persona == Persona::Maple)
    {
        outln!("Maple mode: the Pico presents Xbox-compatible USB reports for Dreamcast adapters.");
    }
    outln!("Press Ctrl+C to stop.");
    outln!();
    outln!("Routes:");
    for route in routes {
        outln!("  {}", route.label());
    }
    outln!();
    outln!(
        "Live status updates show outbound packets, Pico replies, and the last controller state."
    );
}

pub(super) fn print_status(routes: &mut [RouteRuntime]) {
    outln!();
    outln!("Status");
    for route in routes {
        print_route_status(route);
    }
}

pub(super) fn print_route_status(route: &mut RouteRuntime) {
    let sent_delta = route.sent_total.saturating_sub(route.sent_at_last_status);
    route.sent_at_last_status = route.sent_total;
    let bluetooth_route = route.route.pico.persona.is_bluetooth();
    let inbound_age = route.last_inbound.elapsed();
    let peer_state = route_peer_state(route, bluetooth_route, inbound_age);
    let source_state = if route.source_connected {
        "source live"
    } else {
        "waiting for source"
    };
    let detail = route_input_detail(route);
    let last_inbound_ms_ago = if bluetooth_route || route.inbound_total == 0 {
        None
    } else {
        Some(pico_cache::duration_ms(inbound_age))
    };
    pico_cache::record(
        &pico_cache::PicoStateSnapshot::from_target("stream-status", &route.route.pico).with_route(
            pico_cache::RouteSnapshot {
                source_slot: Some(route.route.source_slot),
                source_label: Some(route.route.source_label()),
                peer_health: Some(peer_state.clone()),
                sent_total: Some(route.sent_total),
                inbound_total: Some(route.inbound_total),
                sent_delta: Some(sent_delta),
                last_inbound_ms_ago,
                source_connected: Some(route.source_connected),
                source_auto_switch_count: Some(route.source_auto_switch_count),
                last_source_auto_switch: route.last_source_auto_switch.clone(),
                last_send_type: Some(route.last_send_type.to_string()),
            },
        ),
    );
    let source_switch_note = match (
        route.source_auto_switch_count,
        route.last_source_auto_switch.as_deref(),
    ) {
        (0, _) => String::new(),
        (count, Some(last)) => format!(" | source auto-switches {count} last {last}"),
        (count, None) => format!(" | source auto-switches {count}"),
    };
    if bluetooth_route {
        outln!(
            "  {} -> {} ({}) | {}{} | PC USB input +{} total {} | Bluetooth output {} | {} {}",
            route.route.source_label(),
            route.route.pico.uid_hex(),
            route.route.pico.persona.label(),
            source_state,
            source_switch_note,
            sent_delta,
            route.sent_total,
            peer_state,
            route.last_send_type,
            detail,
        );
    } else {
        outln!(
            "  {} -> {} | {}{} | out +{} total {} | in {} ({}) | {} {}",
            route.route.source_label(),
            route.route.pico.uid_hex(),
            source_state,
            source_switch_note,
            sent_delta,
            route.sent_total,
            route.inbound_total,
            peer_state,
            route.last_send_type,
            detail,
        );
    }
    print_route_hints(route, bluetooth_route);
}

pub(super) fn route_peer_state(
    route: &RouteRuntime,
    bluetooth_route: bool,
    inbound_age: Duration,
) -> String {
    if bluetooth_route {
        format_bluetooth_peer_state(
            route.bluetooth_status.as_ref(),
            route.bluetooth_report_delta,
            route.bluetooth_status_unsupported,
            route.bluetooth_status_error.as_deref(),
        )
    } else if route.inbound_total == 0 {
        "no reply yet".to_string()
    } else if inbound_age > PEER_STALE_AFTER {
        format!("no reply for {:.1}s", inbound_age.as_secs_f32())
    } else {
        format!("reply {:.1}s ago", inbound_age.as_secs_f32())
    }
}

pub(super) fn route_input_detail(route: &RouteRuntime) -> String {
    match route.route.pico.persona {
        Persona::Xinput
        | Persona::Maple
        | Persona::Ps3
        | Persona::Ps4
        | Persona::XboxOne
        | Persona::GenericHid
        | Persona::BluetoothHid
        | Persona::BluetoothXbox
        | Persona::BluetoothPlaystation
        | Persona::Debug => format!(
            "buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
            route.last_state.buttons,
            route.last_state.left_trigger,
            route.last_state.right_trigger,
            route.last_state.left_x,
            route.last_state.left_y,
            route.last_state.right_x,
            route.last_state.right_y,
        ),
        Persona::Keyboard => {
            let keys: Vec<String> = route
                .last_key
                .keys
                .iter()
                .filter(|&&k| k != 0)
                .map(|k| format!("0x{k:02X}"))
                .collect();
            format!(
                "mods=0x{:02X} keys=[{}]",
                route.last_key.modifiers,
                keys.join(" ")
            )
        }
    }
}

pub(super) fn print_route_hints(route: &mut RouteRuntime, bluetooth_route: bool) {
    if !bluetooth_route
        && route.inbound_total == 0
        && route.sent_total > 180
        && !route.hints.recovery
    {
        outln!(
            "    hint: no Pico reply yet. Confirm this Pico is powered and on the same Wi-Fi; run `couchlink bundle` if it stays unreachable."
        );
        route.hints.recovery = true;
    } else if bluetooth_route
        && route.sent_total > 180
        && !route.hints.bluetooth_pairing
        && !route.source_connected
    {
        outln!(
            "    hint: the PC source controller is not live. Connect the Parsec or local controller feeding {}, or stop and choose a live source.",
            route.route.source_label()
        );
        route.hints.bluetooth_pairing = true;
    } else if bluetooth_route
        && route.sent_total > 180
        && !route.hints.bluetooth_pairing
        && route.source_connected
        && should_print_bluetooth_pairing_hint(route.bluetooth_status.as_ref())
    {
        let expected_name = bluetooth_expected_name(route.route.pico.persona);
        outln!(
            "    hint: USB input to the Pico is active. If the game sees nothing, put the receiver in pairing/search mode and pair with {expected_name}."
        );
        route.hints.bluetooth_pairing = true;
    } else if !bluetooth_route
        && route.inbound_total > 0
        && route.last_inbound.elapsed() > PEER_STALE_AFTER
        && !route.hints.recovery
    {
        outln!(
            "    hint: this Pico stopped replying. CouchLink will try to rediscover it; check power and Wi-Fi if it stays stale."
        );
        route.hints.recovery = true;
    }
}
