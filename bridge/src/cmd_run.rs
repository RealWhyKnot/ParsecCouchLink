//! `couchlink run` -- direct streaming mode. The no-argument
//! `couchlink` entrypoint wraps this with a guided menu, while this
//! module keeps the scriptable route syntax for startup shortcuts and
//! third-party launchers.

mod bluetooth;
mod debug_harvest;
mod routing;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use bluetooth::{
    bluetooth_cdc_frame_from_packet, bluetooth_expected_name, format_bluetooth_peer_state,
    is_usb_output_persona, open_bluetooth_usb_links, refresh_bluetooth_statuses, short_error,
    should_print_bluetooth_pairing_hint,
};
use debug_harvest::{
    apply_debug_packet_harvests, collect_debug_packet_harvests, debug_packet_harvest_targets,
    ensure_debug_packet_sinks, has_debug_packet_routes, DebugPacketHarvestResult,
    DEBUG_PACKET_HARVEST_EVERY,
};

use crate::protocol::{self, GamepadState, Packet, PacketKind, Persona, FLAG_PARSEC_CONNECTED};
use crate::{
    cdc, cmd_flash, config, discovery, journal, keyboard, net, pico_cache, support, xinput,
};

pub use bluetooth::print_bluetooth_pairing_help;
#[cfg(test)]
use routing::parse_user_slot;
pub use routing::{
    auto_routes, identity_from_target, match_pico_selector, parse_ip_selector, parse_route_specs,
};
use routing::{
    manual_ips_from_options, routes_from_saved, save_routes, select_picos_by_specs, validate_routes,
};

const DEFAULT_DISCOVER_SECONDS: u64 = 5;
const DEFAULT_STATUS_SECONDS: u64 = 2;
const STREAM_TICK: Duration = Duration::from_millis(16);
const PEER_STALE_AFTER: Duration = Duration::from_secs(5);
const PEER_RECOVER_EVERY: Duration = Duration::from_secs(10);
const PEER_RECOVERY_DISCOVER: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub all: bool,
    pub picos: Vec<String>,
    pub routes: Vec<String>,
    pub use_saved: bool,
    pub status_seconds: u64,
    pub discover_seconds: u64,
    pub quiet: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            all: false,
            picos: Vec::new(),
            routes: Vec::new(),
            use_saved: false,
            status_seconds: DEFAULT_STATUS_SECONDS,
            discover_seconds: DEFAULT_DISCOVER_SECONDS,
            quiet: false,
        }
    }
}

impl RunOptions {
    fn has_explicit_layout(&self) -> bool {
        self.all || !self.picos.is_empty() || !self.routes.is_empty() || self.use_saved
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PicoTarget {
    pub peer: SocketAddr,
    pub info: protocol::AckInfo,
    /// Output persona this Pico is currently presenting, read from the
    /// ACK flags at discovery. Determines whether the bridge streams pad
    /// state or keyboard reports to it.
    pub persona: Persona,
    pub ack_flags: u8,
}

impl PicoTarget {
    pub fn uid_hex(&self) -> String {
        format!("{:08X}", self.info.unique_id_short)
    }

    pub fn board_label(&self) -> &'static str {
        match self.info.board_type {
            protocol::BOARD_PICO_2_W => "Pico 2 W",
            protocol::BOARD_PICO_W_RP2040 => "Pico W",
            _ => "Pico",
        }
    }

    pub fn short_label(&self) -> String {
        format!(
            "{} {} at {}",
            self.board_label(),
            self.uid_hex(),
            self.peer.ip()
        )
    }

    pub fn detail_label(&self) -> String {
        format!(
            "{} {}  {}  fw v{}  uptime {}s flags=0x{:02X}",
            self.board_label(),
            self.uid_hex(),
            self.peer,
            self.info.firmware_version(),
            self.info.uptime_seconds,
            self.ack_flags,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamRoute {
    pub source_slot: u32,
    pub pico: PicoTarget,
}

impl StreamRoute {
    pub fn label(&self) -> String {
        format!("{} -> {}", self.source_label(), self.pico.short_label())
    }

    /// What drives this route: a specific controller slot, or the host
    /// keyboard for a keyboard-persona Pico.
    pub fn source_label(&self) -> String {
        match self.pico.persona {
            Persona::Keyboard => "Keyboard".to_string(),
            Persona::Xinput
            | Persona::Maple
            | Persona::Ps3
            | Persona::Ps4
            | Persona::XboxOne
            | Persona::GenericHid
            | Persona::BluetoothHid
            | Persona::BluetoothXbox
            | Persona::BluetoothPlaystation
            | Persona::Debug => xinput::user_slot_label(self.source_slot),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StreamOptions {
    pub status_seconds: u64,
    pub quiet: bool,
    pub save_routes: bool,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            status_seconds: DEFAULT_STATUS_SECONDS,
            quiet: false,
            save_routes: false,
        }
    }
}

pub async fn run(options: RunOptions) -> Result<()> {
    tracing::info!("run: starting, bridge v{}", env!("CARGO_PKG_VERSION"));
    journal!("run", "started bridge v{}", env!("CARGO_PKG_VERSION"));

    let cfg = config::load().unwrap_or_default();
    if !cfg.setup_complete {
        tracing::warn!(
            "setup_complete = false in config. If this is a fresh install, run `couchlink setup` first."
        );
    }

    let saved_routes = cfg.routes.clone();
    if !options.has_explicit_layout() && saved_routes.is_empty() {
        return run_legacy_single().await;
    }

    let discover_timeout = Duration::from_secs(options.discover_seconds);
    let mut picos = discover_picos_with_auto_recovery(discover_timeout, options.quiet).await?;
    let manual_ips = manual_ips_from_options(&options);
    if !manual_ips.is_empty() {
        picos = add_manual_ip_targets(picos, &manual_ips, discover_timeout).await?;
    }
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(options.discover_seconds));
    }

    let routes = if !options.routes.is_empty() {
        parse_route_specs(&options.routes, &picos)?
    } else if options.all {
        auto_routes(picos.clone(), Some((0..4).collect()))?
    } else if !options.picos.is_empty() {
        let selected = select_picos_by_specs(&options.picos, &picos)?;
        auto_routes(selected, Some((0..4).collect()))?
    } else {
        routes_from_saved(&saved_routes, &picos)?
    };

    stream_routes(
        routes,
        StreamOptions {
            status_seconds: options.status_seconds,
            quiet: options.quiet,
            save_routes: false,
        },
    )
    .await
}

pub async fn discover_picos(timeout: Duration) -> Result<Vec<PicoTarget>> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP discovery socket")?;
    let found = discovery::collect(&socket, timeout)
        .await
        .context("collecting Pico discovery replies")?;
    let targets: Vec<PicoTarget> = found
        .into_iter()
        .map(|reply| PicoTarget {
            peer: reply.peer,
            info: reply.info,
            persona: reply.persona,
            ack_flags: reply.flags,
        })
        .collect();
    for pico in &targets {
        pico_cache::record_target("discover", pico);
    }
    Ok(targets)
}

pub async fn discover_picos_with_auto_recovery(
    timeout: Duration,
    quiet: bool,
) -> Result<Vec<PicoTarget>> {
    let mut picos = discover_picos(timeout).await?;

    if picos.is_empty() && !quiet {
        println!("No Pico replied on Wi-Fi. Checking recoverable USB states...");
    }

    let baseline_ids: HashSet<u32> = picos.iter().map(|p| p.info.unique_id_short).collect();
    let recovered = recover_setup_usb_to_wifi(quiet).await?;
    if recovered > 0 {
        if !quiet {
            println!("Waiting for the recovered Pico to answer on Wi-Fi...");
        }
        let recovered_picos =
            wait_for_wifi_after_recovery(Duration::from_secs(60), quiet, &baseline_ids, recovered)
                .await?;
        let recovered_count = recovered_target_count(&recovered_picos, &baseline_ids);
        if recovered_count < recovered && !quiet {
            println!(
                "Only {recovered_count}/{recovered} recovered setup-mode Pico board(s) answered on Wi-Fi before timeout."
            );
        }
        merge_unique_picos(&mut picos, recovered_picos);
    }

    if !picos.is_empty() {
        return Ok(picos);
    }

    let bootsel = cmd_flash::visible_bootsel_mounts();
    if !bootsel.is_empty() && !quiet {
        println!("Found Pico board(s) in BOOTSEL firmware mode:");
        for (mount, board) in bootsel {
            println!("  {}  {}", mount.display(), board.label());
        }
        println!("Next step: choose `Update Pico firmware` or run `couchlink flash --all`.");
    }

    Ok(Vec::new())
}

pub async fn run_recover_command() -> Result<()> {
    println!("Recovering Pico boards for streaming...");
    let picos = discover_picos_with_auto_recovery(Duration::from_secs(5), false).await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(5));
    }
    println!("Ready for streaming:");
    for pico in picos {
        println!("  {}", pico.detail_label());
        println!("    manual IP: {}", pico.peer.ip());
    }
    Ok(())
}

pub async fn probe_pico_ip(ip: IpAddr, timeout: Duration) -> Result<PicoTarget> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP manual-IP probe socket")?;
    let Some(reply) = discovery::probe_ip(&socket, ip, timeout)
        .await
        .with_context(|| format!("probing Pico at {ip}:{}", protocol::PORT))?
    else {
        bail!(
            "no Pico replied at {ip}:{} within {} s",
            protocol::PORT,
            timeout.as_secs()
        );
    };
    if reply.info.proto_version != protocol::PROTO_VERSION {
        bail!(
            "Pico at {} speaks protocol v{}, bridge speaks v{}. Update whichever side is older.",
            reply.peer,
            reply.info.proto_version,
            protocol::PROTO_VERSION,
        );
    }
    let target = PicoTarget {
        peer: reply.peer,
        info: reply.info,
        persona: reply.persona,
        ack_flags: reply.flags,
    };
    pico_cache::record_target("probe-ip", &target);
    Ok(target)
}

async fn recover_setup_usb_to_wifi(quiet: bool) -> Result<usize> {
    let ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        return Ok(0);
    }

    let mut rebooted = 0usize;
    let mut blocked = 0usize;
    let mut printed_header = false;

    for port in ports {
        match setup_port_reboot_to_run(port.clone()).await {
            Ok(SetupRecovery::Rebooted { firmware, board }) => {
                rebooted += 1;
                if !quiet {
                    print_setup_recovery_header(&mut printed_header);
                    println!("  {port}: fw v{firmware} {board} -> Wi-Fi/input mode");
                }
            }
            Ok(SetupRecovery::NoCredentials { firmware, board }) => {
                blocked += 1;
                if !quiet {
                    print_setup_recovery_header(&mut printed_header);
                    println!(
                        "  {port}: fw v{firmware} {board} has no saved Wi-Fi; choose `Set up or change Wi-Fi`."
                    );
                }
            }
            Ok(SetupRecovery::AlreadyRunMode { firmware, board }) => {
                tracing::debug!(
                    "run: {port} fw v{firmware} {board} is already in run mode; skipping USB recovery"
                );
            }
            Err(e) => {
                blocked += 1;
                if !quiet {
                    print_setup_recovery_header(&mut printed_header);
                    println!("  {port}: could not auto-recover: {e:#}");
                }
            }
        }
    }

    if rebooted == 0 && blocked > 0 && !quiet {
        println!("Auto-recovery could not switch any setup-mode Pico back to Wi-Fi.");
    }

    Ok(rebooted)
}

fn print_setup_recovery_header(printed: &mut bool) {
    if !*printed {
        println!("Found recoverable setup-mode USB Pico port(s):");
        *printed = true;
    }
}

enum SetupRecovery {
    Rebooted {
        firmware: String,
        board: &'static str,
    },
    AlreadyRunMode {
        firmware: String,
        board: &'static str,
    },
    NoCredentials {
        firmware: String,
        board: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetupUsbMode {
    RunModeActive,
    SetupModeWithCredentials,
    SetupModeWithoutCredentials,
}

fn classify_setup_usb_hello(hello: &cdc::HelloAck) -> SetupUsbMode {
    if hello.run_mode_active() {
        SetupUsbMode::RunModeActive
    } else if hello.creds_present() {
        SetupUsbMode::SetupModeWithCredentials
    } else {
        SetupUsbMode::SetupModeWithoutCredentials
    }
}

async fn setup_port_reboot_to_run(port: String) -> Result<SetupRecovery> {
    tokio::task::spawn_blocking(move || -> Result<SetupRecovery> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        if hello.proto_version != cdc::PROTO_VERSION {
            bail!(
                "Pico speaks CDC protocol v{}, bridge speaks v{}. Update whichever side is older.",
                hello.proto_version,
                cdc::PROTO_VERSION,
            );
        }

        let firmware = hello.firmware_version().to_string();
        let board = setup_board_label(hello.board_type);
        if classify_setup_usb_hello(&hello) == SetupUsbMode::RunModeActive {
            return Ok(SetupRecovery::AlreadyRunMode { firmware, board });
        }

        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }

        if classify_setup_usb_hello(&hello) == SetupUsbMode::SetupModeWithoutCredentials {
            return Ok(SetupRecovery::NoCredentials { firmware, board });
        }

        pico.reboot_to_run()?;
        Ok(SetupRecovery::Rebooted { firmware, board })
    })
    .await?
}

async fn wait_for_wifi_after_recovery(
    timeout: Duration,
    quiet: bool,
    baseline_ids: &HashSet<u32>,
    expected_new: usize,
) -> Result<Vec<PicoTarget>> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        let picos = discover_picos(Duration::from_secs(2)).await?;
        merge_unique_picos(&mut seen, picos);
        if recovered_target_count(&seen, baseline_ids) >= expected_new {
            return Ok(seen);
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(seen);
        }
        if now >= next_beat {
            if !quiet {
                let elapsed = now.duration_since(started).as_secs();
                println!(
                    "  ... still waiting for Wi-Fi reply ({elapsed}/{})",
                    timeout.as_secs()
                );
            }
            next_beat = now + Duration::from_secs(10);
        }
    }
}

fn merge_unique_picos(picos: &mut Vec<PicoTarget>, incoming: Vec<PicoTarget>) {
    for pico in incoming {
        if let Some(existing) = picos
            .iter_mut()
            .find(|p| p.info.unique_id_short == pico.info.unique_id_short)
        {
            *existing = pico;
        } else {
            picos.push(pico);
        }
    }
}

fn recovered_target_count(picos: &[PicoTarget], baseline_ids: &HashSet<u32>) -> usize {
    picos
        .iter()
        .filter(|p| !baseline_ids.contains(&p.info.unique_id_short))
        .count()
}

fn setup_board_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W / WH",
        _ => "Pico",
    }
}

async fn add_manual_ip_targets(
    mut picos: Vec<PicoTarget>,
    ips: &[IpAddr],
    timeout: Duration,
) -> Result<Vec<PicoTarget>> {
    for ip in ips {
        if picos.iter().any(|p| p.peer.ip() == *ip) {
            continue;
        }
        let pico = probe_pico_ip(*ip, timeout).await?;
        if !picos
            .iter()
            .any(|p| p.info.unique_id_short == pico.info.unique_id_short)
        {
            picos.push(pico);
        }
    }
    Ok(picos)
}

pub async fn stream_routes(routes: Vec<StreamRoute>, options: StreamOptions) -> Result<()> {
    if routes.is_empty() {
        bail!("no routes selected");
    }
    validate_routes(&routes)?;
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
            pico_cache::PicoStateSnapshot::from_target("stream-start", &route.pico).with_route(
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

    let mut runtime: Vec<RouteRuntime> = routes
        .into_iter()
        .map(|route| {
            let bluetooth_usb = bluetooth_usb_links.remove(&route.pico.info.unique_id_short);
            RouteRuntime::new(route, bluetooth_usb)
        })
        .collect();
    // Bring the injected-input keyboard hook up before the first tick so a
    // keyboard route doesn't start with empty reports.
    if runtime
        .iter()
        .any(|r| r.route.pico.persona == Persona::Keyboard)
    {
        keyboard::start_capture();
    }
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
                for route in &mut runtime {
                    if route.route.pico.persona.is_bluetooth() {
                        if let Err(e) = route.send_bluetooth_usb_tick() {
                            return Err(e).with_context(|| {
                                format!("streaming over USB to {}", route.route.pico.short_label())
                            });
                        }
                        continue;
                    }
                    if let Err(e) = route.send_udp_tick(&socket).await {
                        // A Pico that rebooted or dropped off Wi-Fi makes the OS
                        // surface a transient error (ICMP unreachable -> reset on
                        // Windows). Keep streaming the other routes and let the
                        // recovery path rediscover this one instead of exiting.
                        if net::is_transient(&e) {
                            tracing::debug!(
                                "stream: transient send error to {} (continuing): {e}",
                                route.route.pico.short_label()
                            );
                        } else {
                            return Err(e).with_context(|| {
                                format!("streaming to {}", route.route.pico.short_label())
                            });
                        }
                    }
                }
            }
            recv = socket.recv_from(&mut buf) => {
                match recv {
                    Ok((n, from)) => handle_stream_reply(&mut runtime, from, &buf[..n]),
                    // Same transient family on the shared recv socket -- not
                    // attributable to one peer, so just log and keep listening.
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
                    let tx = recovery_tx.clone();
                    tokio::spawn(async move {
                        let result = discover_picos(PEER_RECOVERY_DISCOVER).await;
                        let _ = tx.send(result).await;
                    });
                }
            }
            _ = debug_harvest.tick(), if !debug_harvest_in_flight && has_debug_packet_routes(&runtime, &debug_packet_disabled) => {
                debug_harvest_in_flight = true;
                let targets = debug_packet_harvest_targets(&runtime, &debug_packet_disabled);
                let tx = debug_harvest_tx.clone();
                tokio::spawn(async move {
                    let results = collect_debug_packet_harvests(targets).await;
                    let _ = tx.send(results).await;
                });
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
                match recovery {
                    Some(Ok(picos)) => apply_recovery_results(&mut runtime, &picos, options.quiet),
                    Some(Err(e)) if !options.quiet => {
                        println!("Recovery discovery failed: {e:#}");
                        println!("  Check Wi-Fi, firewall, and router client isolation, then run `couchlink bundle` if it persists.");
                    }
                    Some(Err(_)) => {}
                    None => {}
                }
            }
            _ = tokio::signal::ctrl_c() => {
                if !options.quiet {
                    println!();
                    println!("Stopped.");
                }
                return Ok(());
            }
        }
    }
}

struct RouteRuntime {
    route: StreamRoute,
    bluetooth_usb: Option<cdc::PicoSetup>,
    bluetooth_status: Option<cdc::BtStatus>,
    bluetooth_report_delta: Option<u32>,
    bluetooth_status_unsupported: bool,
    bluetooth_status_error: Option<String>,
    seq: u8,
    sent_total: u64,
    sent_at_last_status: u64,
    inbound_total: u64,
    last_inbound: Instant,
    last_state: GamepadState,
    last_key: protocol::KeyboardReport,
    last_packet_number: Option<u32>,
    source_connected: bool,
    last_send_type: &'static str,
    last_recovery_attempt: Option<Instant>,
    recovery_hint_printed: bool,
    bluetooth_pairing_hint_printed: bool,
}

impl RouteRuntime {
    fn new(route: StreamRoute, bluetooth_usb: Option<cdc::PicoSetup>) -> Self {
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
            last_send_type: "heartbeat",
            last_recovery_attempt: None,
            recovery_hint_printed: false,
            bluetooth_pairing_hint_printed: false,
        }
    }

    async fn send_udp_tick(&mut self, socket: &UdpSocket) -> std::io::Result<()> {
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
        socket
            .send_to(&packet.encode(), self.route.pico.peer)
            .await?;
        self.sent_total += 1;
        Ok(())
    }

    fn send_bluetooth_usb_tick(&mut self) -> Result<()> {
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

    fn refresh_bluetooth_status(&mut self) {
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
        let source = xinput::read_slot(self.route.source_slot);
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

    fn next_keyboard_packet(&mut self) -> Packet {
        let report = keyboard::read_keyboard();
        // The host keyboard is always present in the Parsec model, so the
        // first tick is "changed" and the source is always advertised as
        // connected. Unchanged ticks still send a heartbeat so the
        // firmware watchdog stays fed.
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

fn handle_stream_reply(routes: &mut [RouteRuntime], from: SocketAddr, buf: &[u8]) {
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

fn print_stream_intro(routes: &[StreamRoute], local_addr: SocketAddr) {
    println!();
    println!("CouchLink streaming");
    let udp_routes = routes
        .iter()
        .filter(|route| !route.pico.persona.is_bluetooth())
        .count();
    let bluetooth_routes = routes.len().saturating_sub(udp_routes);
    if udp_routes > 0 {
        println!("Local UDP socket: {local_addr}");
    }
    if bluetooth_routes > 0 {
        println!("Bluetooth path: source controller -> PC USB -> Pico -> Bluetooth receiver.");
        println!(
            "  Pair the receiver with the Pico's CouchLink Bluetooth gamepad; PIN is 0000 if requested."
        );
        println!(
            "  Status below reports both the USB input link and the Bluetooth receiver state."
        );
    }
    if routes
        .iter()
        .any(|route| is_usb_output_persona(route.pico.persona))
    {
        println!(
            "USB-output note: Wi-Fi counters prove PC-to-Pico input, not console-adapter acceptance."
        );
        println!("  If the console sees no input, run `couchlink test usb` or `couchlink bundle`.");
    }
    if routes
        .iter()
        .any(|route| route.pico.persona == Persona::Keyboard)
    {
        println!(
            "Keyboard mode: only remote Parsec-injected keystrokes are relayed; empty keys means no captured typing yet."
        );
    }
    if routes
        .iter()
        .any(|route| route.pico.persona == Persona::Maple)
    {
        println!(
            "Maple mode: the Pico presents Xbox-compatible USB reports for Dreamcast adapters."
        );
    }
    println!("Press Ctrl+C to stop.");
    println!();
    println!("Routes:");
    for route in routes {
        println!("  {}", route.label());
    }
    println!();
    println!(
        "Live status updates show outbound packets, Pico replies, and the last controller state."
    );
}

fn print_status(routes: &mut [RouteRuntime]) {
    println!();
    println!("Status");
    for route in routes {
        let sent_delta = route.sent_total.saturating_sub(route.sent_at_last_status);
        route.sent_at_last_status = route.sent_total;
        let bluetooth_route = route.route.pico.persona.is_bluetooth();
        let inbound_age = route.last_inbound.elapsed();
        let peer_state = if bluetooth_route {
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
        };
        let source_state = if route.source_connected {
            "source live"
        } else {
            "waiting for source"
        };
        let detail = match route.route.pico.persona {
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
        };
        let last_inbound_ms_ago = if bluetooth_route || route.inbound_total == 0 {
            None
        } else {
            Some(pico_cache::duration_ms(inbound_age))
        };
        pico_cache::record(
            pico_cache::PicoStateSnapshot::from_target("stream-status", &route.route.pico)
                .with_route(pico_cache::RouteSnapshot {
                    source_slot: Some(route.route.source_slot),
                    source_label: Some(route.route.source_label()),
                    peer_health: Some(peer_state.clone()),
                    sent_total: Some(route.sent_total),
                    inbound_total: Some(route.inbound_total),
                    sent_delta: Some(sent_delta),
                    last_inbound_ms_ago,
                    source_connected: Some(route.source_connected),
                    last_send_type: Some(route.last_send_type.to_string()),
                }),
        );
        if bluetooth_route {
            println!(
                "  {} -> {} ({}) | {} | PC USB input +{} total {} | Bluetooth output {} | {} {}",
                route.route.source_label(),
                route.route.pico.uid_hex(),
                route.route.pico.persona.label(),
                source_state,
                sent_delta,
                route.sent_total,
                peer_state,
                route.last_send_type,
                detail,
            );
        } else {
            println!(
                "  {} -> {} | {} | out +{} total {} | in {} ({}) | {} {}",
                route.route.source_label(),
                route.route.pico.uid_hex(),
                source_state,
                sent_delta,
                route.sent_total,
                route.inbound_total,
                peer_state,
                route.last_send_type,
                detail,
            );
        }
        if !bluetooth_route
            && route.inbound_total == 0
            && route.sent_total > 180
            && !route.recovery_hint_printed
        {
            println!(
                "    hint: no Pico reply yet. Confirm this Pico is powered and on the same Wi-Fi; run `couchlink bundle` if it stays unreachable."
            );
            route.recovery_hint_printed = true;
        } else if bluetooth_route
            && route.sent_total > 180
            && !route.bluetooth_pairing_hint_printed
            && should_print_bluetooth_pairing_hint(route.bluetooth_status.as_ref())
        {
            let expected_name = bluetooth_expected_name(route.route.pico.persona);
            println!(
                "    hint: USB input to the Pico is active. If the game sees nothing, put the receiver in pairing/search mode and pair with {expected_name}."
            );
            route.bluetooth_pairing_hint_printed = true;
        } else if !bluetooth_route
            && route.inbound_total > 0
            && route.last_inbound.elapsed() > PEER_STALE_AFTER
            && !route.recovery_hint_printed
        {
            println!(
                "    hint: this Pico stopped replying. CouchLink will try to rediscover it; check power and Wi-Fi if it stays stale."
            );
            route.recovery_hint_printed = true;
        }
    }
}

fn schedule_recovery_if_needed(routes: &mut [RouteRuntime]) -> bool {
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
            .map(|last| now.duration_since(last) < PEER_RECOVER_EVERY)
            .unwrap_or(false)
        {
            continue;
        }
        route.last_recovery_attempt = Some(now);
        needed = true;
    }
    needed
}

fn apply_recovery_results(routes: &mut [RouteRuntime], picos: &[PicoTarget], quiet: bool) {
    for route in routes {
        let Some(found) = picos
            .iter()
            .find(|p| p.info.unique_id_short == route.route.pico.info.unique_id_short)
        else {
            continue;
        };
        if found.peer != route.route.pico.peer {
            if !quiet {
                println!(
                    "Recovered {}: {} -> {}",
                    route.route.pico.uid_hex(),
                    route.route.pico.peer,
                    found.peer
                );
            }
            route.route.pico = found.clone();
            route.last_inbound = Instant::now();
            route.recovery_hint_printed = false;
        }
    }
}

async fn run_legacy_single() -> Result<()> {
    println!("Looking for a Pico on the LAN...");
    let mut picos =
        discover_picos_with_auto_recovery(Duration::from_secs(DEFAULT_DISCOVER_SECONDS), false)
            .await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(DEFAULT_DISCOVER_SECONDS));
    }
    let pico = picos.remove(0);
    tracing::info!(
        "run: discovered Pico {} fw v{} uid 0x{:08X}",
        pico.peer,
        pico.info.firmware_version(),
        pico.info.unique_id_short,
    );
    journal!(
        "run",
        "discovered Pico {} fw v{} uid 0x{:08X}",
        pico.peer,
        pico.info.firmware_version(),
        pico.info.unique_id_short,
    );

    if pico.info.proto_version != protocol::PROTO_VERSION {
        bail!(
            "wire protocol mismatch: Pico speaks v{}, bridge speaks v{}. Update whichever is older.",
            pico.info.proto_version,
            protocol::PROTO_VERSION,
        );
    }

    let route = auto_routes(vec![pico], None)?
        .into_iter()
        .next()
        .expect("auto_routes returned one route");
    stream_routes(
        vec![route],
        StreamOptions {
            status_seconds: DEFAULT_STATUS_SECONDS,
            quiet: false,
            save_routes: false,
        },
    )
    .await
}

#[cfg(test)]
mod tests;
