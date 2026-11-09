use super::bluetooth::{open_bluetooth_usb_links, refresh_bluetooth_statuses};
use super::debug_harvest::{
    DEBUG_PACKET_HARVEST_EVERY, DebugPacketHarvestResult, apply_debug_packet_harvests,
    collect_debug_packet_harvests, debug_packet_harvest_targets, ensure_debug_packet_sinks,
    has_debug_packet_routes,
};
use super::route_runtime::RouteRuntime;
use super::routing::{save_routes, validate_routes};
use super::status::{print_status, print_stream_intro};
use super::{
    PEER_RECOVER_EVERY, PEER_RECOVERY_DISCOVER, PEER_STALE_AFTER, PicoTarget, STREAM_TICK,
    StreamOptions, StreamRoute, discover_picos, prepare_bluetooth_routes,
    should_auto_switch_bluetooth_source,
};
use crate::protocol::{Packet, PacketKind, Persona};
use crate::{keyboard, net, pico_cache, udp_auth};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};

pub(crate) async fn stream_routes(routes: Vec<StreamRoute>, options: StreamOptions) -> Result<()> {
    let (socket, mut runtime) = start_stream(routes, &options).await?;
    let mut tick = interval(STREAM_TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut status = interval(Duration::from_secs(options.status_seconds.max(1)));
    status.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut debug_harvest = interval(DEBUG_PACKET_HARVEST_EVERY);
    debug_harvest.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut buf = [0u8; 64];
    let (recovery_tx, mut recovery_rx) = mpsc::channel::<Result<Vec<PicoTarget>>>(1);
    let mut recovery_in_flight = false;
    let (debug_harvest_tx, mut debug_harvest_rx) =
        mpsc::channel::<Vec<DebugPacketHarvestResult>>(1);
    let mut debug_harvest_in_flight = false;
    let mut debug_packet_sinks = HashMap::new();
    let mut debug_packet_disabled = HashSet::new();
    ensure_debug_packet_sinks(
        &runtime,
        &mut debug_packet_sinks,
        &mut debug_packet_disabled,
        options.quiet,
    );

    loop {
        tokio::select! {
            _ = tick.tick() => {
                send_stream_ticks(&mut runtime, &socket).await?;
            }
            recv = socket.recv_from(&mut buf) => {
                match recv {
                    Ok((n, from)) => handle_stream_reply(&mut runtime, from, &buf[..n]),
                    Err(e) if net::is_transient(&e) => {
                        tracing::debug!("stream: transient recv error (continuing): {e}");
                    }
                    Err(e) => return Err(e).context("receiving stream reply"),
                }
            }
            _ = status.tick() => {
                if !options.quiet {
                    refresh_bluetooth_statuses(&mut runtime);
                    print_status(&mut runtime);
                }
                if !recovery_in_flight && schedule_recovery_if_needed(&mut runtime) {
                    recovery_in_flight = true;
                    spawn_recovery_discovery(recovery_tx.clone());
                }
            }
            _ = debug_harvest.tick(), if !debug_harvest_in_flight && has_debug_packet_routes(&runtime, &debug_packet_disabled) => {
                debug_harvest_in_flight = true;
                let targets = debug_packet_harvest_targets(&runtime, &debug_packet_disabled);
                spawn_debug_packet_harvest(targets, debug_harvest_tx.clone());
            }
            harvest = debug_harvest_rx.recv() => {
                debug_harvest_in_flight = false;
                if let Some(results) = harvest {
                    apply_debug_packet_harvests(
                        results,
                        &mut debug_packet_sinks,
                        &mut debug_packet_disabled,
                        options.quiet,
                    );
                }
            }
            recovery = recovery_rx.recv() => {
                recovery_in_flight = false;
                finish_recovery(&mut runtime, recovery, options.quiet);
            }
            _ = tokio::signal::ctrl_c() => {
                if !options.quiet {
                    outln!();
                    outln!("Stopped.");
                }
                return Ok(());
            }
        }
    }
}

pub(super) async fn start_stream(
    routes: Vec<StreamRoute>,
    options: &StreamOptions,
) -> Result<(UdpSocket, Vec<RouteRuntime>)> {
    if routes.is_empty() {
        bail!("no routes selected");
    }
    validate_routes(&routes)?;
    let routes = prepare_bluetooth_routes(routes, options.quiet).await?;
    validate_routes(&routes)?;
    for route in routes.iter().filter(|r| !r.pico.persona.is_bluetooth()) {
        udp_auth::prepare(route.pico.peer).await?;
    }
    let mut bluetooth_usb_links = open_bluetooth_usb_links(&routes, options.quiet)?;
    if options.save_routes {
        save_routes(&routes)?;
    }
    tracing::info!(
        "stream: starting {} route(s), status={}s quiet={} save_routes={}",
        routes.len(),
        options.status_seconds,
        options.quiet,
        options.save_routes,
    );
    for route in &routes {
        tracing::debug!(
            "stream: route source_slot={} source={} pico={} peer={} persona={} flags=0x{:02X}",
            route.source_slot,
            route.source_label(),
            route.pico.uid_hex(),
            route.pico.peer,
            route.pico.persona.label(),
            route.pico.ack_flags,
        );
        pico_cache::record(
            &pico_cache::PicoStateSnapshot::from_target("stream-start", &route.pico).with_route(
                pico_cache::RouteSnapshot {
                    source_slot: Some(route.source_slot),
                    source_label: Some(route.source_label()),
                    ..pico_cache::RouteSnapshot::default()
                },
            ),
        );
    }

    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP stream socket")?;
    socket
        .set_broadcast(true)
        .context("enabling broadcast on UDP stream socket")?;

    if !options.quiet {
        print_stream_intro(&routes, socket.local_addr()?);
    }

    let allow_bluetooth_source_auto_switch = should_auto_switch_bluetooth_source(&routes);
    let runtime: Vec<RouteRuntime> = routes
        .into_iter()
        .map(|route| {
            let bluetooth_usb = bluetooth_usb_links.remove(&route.pico.info.unique_id_short);
            RouteRuntime::new(route, bluetooth_usb, allow_bluetooth_source_auto_switch)
        })
        .collect();
    if runtime
        .iter()
        .any(|r| r.route.pico.persona == Persona::Keyboard)
    {
        keyboard::start_capture();
    }
    Ok((socket, runtime))
}

pub(super) async fn send_stream_ticks(
    runtime: &mut [RouteRuntime],
    socket: &UdpSocket,
) -> Result<()> {
    for route in runtime {
        if route.route.pico.persona.is_bluetooth() {
            if let Err(e) = route.send_bluetooth_usb_tick() {
                return Err(e).with_context(|| {
                    format!("streaming over USB to {}", route.route.pico.short_label())
                });
            }
            continue;
        }
        if let Err(e) = route.send_udp_tick(socket).await {
            if net::is_transient(&e) {
                tracing::debug!(
                    "stream: transient send error to {} (continuing): {e}",
                    route.route.pico.short_label()
                );
            } else {
                return Err(e)
                    .with_context(|| format!("streaming to {}", route.route.pico.short_label()));
            }
        }
    }
    Ok(())
}

pub(super) fn spawn_recovery_discovery(tx: mpsc::Sender<Result<Vec<PicoTarget>>>) {
    tokio::spawn(async move {
        let result = discover_picos(PEER_RECOVERY_DISCOVER).await;
        let _ = tx.send(result).await;
    });
}

pub(super) fn spawn_debug_packet_harvest(
    targets: Vec<PicoTarget>,
    tx: mpsc::Sender<Vec<DebugPacketHarvestResult>>,
) {
    tokio::spawn(async move {
        let results = collect_debug_packet_harvests(targets).await;
        let _ = tx.send(results).await;
    });
}

pub(super) fn finish_recovery(
    runtime: &mut [RouteRuntime],
    recovery: Option<Result<Vec<PicoTarget>>>,
    quiet: bool,
) {
    match recovery {
        Some(Ok(picos)) => apply_recovery_results(runtime, &picos, quiet),
        Some(Err(e)) if !quiet => {
            outln!("Recovery discovery failed: {e:#}");
            outln!(
                "  Check Wi-Fi, firewall, and router client isolation, then run `couchlink bundle` if it persists."
            );
        }
        Some(Err(_)) | None => {}
    }
}

pub(super) fn handle_stream_reply(routes: &mut [RouteRuntime], from: SocketAddr, buf: &[u8]) {
    let Ok(packet) = Packet::decode(buf) else {
        tracing::debug!("stream: dropping malformed packet from {from}");
        return;
    };
    for route in routes {
        if route.route.pico.peer.ip() == from.ip() {
            route.inbound_total += 1;
            route.last_inbound = Instant::now();
            if let PacketKind::Ack(info) = packet.kind {
                tracing::trace!(
                    "stream: ack from {} uid=0x{:08X} uptime={}s",
                    from,
                    info.unique_id_short,
                    info.uptime_seconds
                );
            }
            return;
        }
    }
    tracing::debug!("stream: packet from non-routed peer {from}");
}

pub(super) fn schedule_recovery_if_needed(routes: &mut [RouteRuntime]) -> bool {
    let now = Instant::now();
    let mut needed = false;
    for route in routes {
        if route.route.pico.persona.is_bluetooth() {
            continue;
        }
        if route.last_inbound.elapsed() <= PEER_STALE_AFTER {
            continue;
        }
        if route
            .last_recovery_attempt
            .is_some_and(|last| now.duration_since(last) < PEER_RECOVER_EVERY)
        {
            continue;
        }
        route.last_recovery_attempt = Some(now);
        needed = true;
    }
    needed
}

pub(super) fn apply_recovery_results(
    routes: &mut [RouteRuntime],
    picos: &[PicoTarget],
    quiet: bool,
) {
    for route in routes {
        let Some(found) = picos
            .iter()
            .find(|p| p.info.unique_id_short == route.route.pico.info.unique_id_short)
        else {
            continue;
        };
        if found.peer != route.route.pico.peer {
            if !quiet {
                outln!(
                    "Recovered {}: {} -> {}",
                    route.route.pico.uid_hex(),
                    route.route.pico.peer,
                    found.peer
                );
            }
            route.route.pico = found.clone();
            route.last_inbound = Instant::now();
            route.hints.recovery = false;
        }
    }
}
