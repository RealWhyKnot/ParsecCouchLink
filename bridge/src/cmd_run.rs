//! `couchlink run` -- direct streaming mode. The no-argument
//! `couchlink` entrypoint wraps this with a guided menu, while this
//! module keeps the scriptable route syntax for startup shortcuts and
//! third-party launchers.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::protocol::{self, GamepadState, Packet, PacketKind, Persona, FLAG_PARSEC_CONNECTED};
use crate::{
    cdc, cmd_flash, config, debug_packets, discovery, journal, keyboard, net, pico_cache, support,
    xinput,
};

const DEFAULT_DISCOVER_SECONDS: u64 = 5;
const DEFAULT_STATUS_SECONDS: u64 = 2;
const STREAM_TICK: Duration = Duration::from_millis(16);
const PEER_STALE_AFTER: Duration = Duration::from_secs(5);
const PEER_RECOVER_EVERY: Duration = Duration::from_secs(10);
const PEER_RECOVERY_DISCOVER: Duration = Duration::from_secs(2);
const DEBUG_PACKET_HARVEST_EVERY: Duration = Duration::from_millis(500);
const DEBUG_PACKET_HARVEST_TIMEOUT: Duration = Duration::from_millis(1200);

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

pub fn identity_from_target(pico: &PicoTarget) -> config::PicoIdentity {
    config::PicoIdentity {
        unique_id_short: pico.info.unique_id_short,
        board_type: pico.info.board_type,
        fw_major: pico.info.fw_major,
        fw_minor: pico.info.fw_minor,
        fw_patch: pico.info.fw_patch,
        last_ip: Some(pico.peer.ip().to_string()),
        device_name: Some(pico.board_label().to_string()),
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

pub fn auto_routes(
    picos: Vec<PicoTarget>,
    preferred_slots: Option<Vec<u32>>,
) -> Result<Vec<StreamRoute>> {
    if picos.is_empty() {
        bail!("no Picos selected");
    }
    let connected: Vec<u32> = xinput::connected_slots()
        .into_iter()
        .map(|s| s.slot)
        .collect();
    let mut slots = preferred_slots.unwrap_or_else(|| {
        if connected.is_empty() {
            (0..picos.len().min(4)).map(|i| i as u32).collect()
        } else {
            connected
        }
    });
    slots.sort_unstable();
    slots.dedup();
    if slots.is_empty() {
        bail!("no XInput source slots are available");
    }
    if slots.len() < picos.len() {
        bail!(
            "{} Pico(s) selected but only {} source controller slot(s) available. Use --route to map explicit slots.",
            picos.len(),
            slots.len()
        );
    }
    Ok(picos
        .into_iter()
        .zip(slots)
        .map(|(pico, source_slot)| StreamRoute { source_slot, pico })
        .collect())
}

pub fn parse_route_specs(specs: &[String], picos: &[PicoTarget]) -> Result<Vec<StreamRoute>> {
    let mut routes = Vec::new();
    for spec in specs {
        let (source, target) = spec
            .split_once('=')
            .or_else(|| spec.split_once(':'))
            .ok_or_else(|| anyhow!("route must look like 1=07D37EB6 or 2=192.168.50.4"))?;
        let source_slot = parse_user_slot(source)?;
        let pico = match_pico_selector(target, picos)?;
        routes.push(StreamRoute { source_slot, pico });
    }
    if routes.is_empty() {
        bail!("no routes provided");
    }
    Ok(routes)
}

pub fn parse_user_slot(input: &str) -> Result<u32> {
    let s = input
        .trim()
        .trim_start_matches(['p', 'P'])
        .trim_start_matches(['x', 'X']);
    let user_slot: u32 = s
        .parse()
        .with_context(|| format!("invalid controller slot `{input}`"))?;
    if !(1..=4).contains(&user_slot) {
        bail!("controller slot must be 1, 2, 3, or 4");
    }
    Ok(user_slot - 1)
}

pub fn match_pico_selector(selector: &str, picos: &[PicoTarget]) -> Result<PicoTarget> {
    let selector = selector.trim();
    if selector.is_empty() {
        bail!("empty Pico selector");
    }

    if let Some(ip) = parse_ip_selector(selector) {
        let matches: Vec<_> = picos.iter().filter(|p| p.peer.ip() == ip).collect();
        return single_match(selector, matches);
    }

    let uid_text = selector
        .strip_prefix("0x")
        .or_else(|| selector.strip_prefix("0X"))
        .unwrap_or(selector);
    if uid_text.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(uid) = u32::from_str_radix(uid_text, 16) {
            let matches: Vec<_> = picos
                .iter()
                .filter(|p| p.info.unique_id_short == uid)
                .collect();
            return single_match(selector, matches);
        }
    }

    let wanted_board = match selector.to_ascii_lowercase().as_str() {
        "rp2350" | "pico2" | "pico2w" | "pico-2-w" => Some(protocol::BOARD_PICO_2_W),
        "rp2040" | "picow" | "pico-w" | "pico-wh" => Some(protocol::BOARD_PICO_W_RP2040),
        _ => None,
    };
    if let Some(board) = wanted_board {
        let matches: Vec<_> = picos
            .iter()
            .filter(|p| p.info.board_type == board)
            .collect();
        return single_match(selector, matches);
    }

    bail!(
        "Pico `{}` was not found. Use a UID like 07D37EB6, an IP address, or rp2350/rp2040.",
        selector
    );
}

pub fn parse_ip_selector(input: &str) -> Option<IpAddr> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Some(ip);
    }
    trimmed.parse::<SocketAddr>().ok().map(|addr| addr.ip())
}

fn manual_ips_from_options(options: &RunOptions) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for spec in &options.picos {
        push_ip_if_new(&mut ips, spec);
    }
    for spec in &options.routes {
        if let Some(target) = route_target(spec) {
            push_ip_if_new(&mut ips, target);
        }
    }
    ips
}

fn push_ip_if_new(ips: &mut Vec<IpAddr>, input: &str) {
    let Some(ip) = parse_ip_selector(input) else {
        return;
    };
    if !ips.contains(&ip) {
        ips.push(ip);
    }
}

fn route_target(spec: &str) -> Option<&str> {
    spec.split_once('=')
        .or_else(|| spec.split_once(':'))
        .map(|(_, target)| target.trim())
}

fn single_match(selector: &str, matches: Vec<&PicoTarget>) -> Result<PicoTarget> {
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("Pico `{selector}` was not found in the current discovery results"),
        _ => bail!("Pico selector `{selector}` matched more than one board; use the UID instead"),
    }
}

fn select_picos_by_specs(specs: &[String], picos: &[PicoTarget]) -> Result<Vec<PicoTarget>> {
    let mut selected = Vec::new();
    for spec in specs {
        let pico = match_pico_selector(spec, picos)?;
        if !selected
            .iter()
            .any(|p: &PicoTarget| p.info.unique_id_short == pico.info.unique_id_short)
        {
            selected.push(pico);
        }
    }
    Ok(selected)
}

fn routes_from_saved(
    saved: &[config::RouteConfig],
    picos: &[PicoTarget],
) -> Result<Vec<StreamRoute>> {
    if saved.is_empty() {
        bail!("no saved routing layout found; run `couchlink` to create one");
    }
    let mut routes = Vec::new();
    for saved_route in saved {
        let selector = format!("{:08X}", saved_route.pico_uid);
        let pico = match_pico_selector(&selector, picos)?;
        routes.push(StreamRoute {
            source_slot: saved_route.source_slot,
            pico,
        });
    }
    Ok(routes)
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

fn validate_routes(routes: &[StreamRoute]) -> Result<()> {
    let mut pico_uids = HashSet::new();
    for route in routes {
        if route.source_slot >= 4 {
            bail!("controller slot must be 1, 2, 3, or 4");
        }
        if !pico_uids.insert(route.pico.info.unique_id_short) {
            bail!(
                "the same Pico ({}) is routed more than once. Pick one source controller per Pico.",
                route.pico.uid_hex()
            );
        }
    }
    Ok(())
}

fn open_bluetooth_usb_links(
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
                    println!(
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

fn save_routes(routes: &[StreamRoute]) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    cfg.routes = routes
        .iter()
        .map(|route| config::RouteConfig {
            source_slot: route.source_slot,
            pico_uid: route.pico.info.unique_id_short,
            label: Some(route.pico.board_label().to_string()),
        })
        .collect();
    for route in routes {
        cfg.remember_pico(identity_from_target(&route.pico));
    }
    config::save(&cfg)
}

#[derive(Clone, Debug)]
struct DebugPacketHarvestResult {
    target: PicoTarget,
    duration_ms: u64,
    outcome: Result<DebugPacketHarvestOk, String>,
}

#[derive(Clone, Debug)]
struct DebugPacketHarvestOk {
    lines: Vec<String>,
    raw_packet_lines: usize,
    stats_lines: usize,
    event_lines: usize,
    snapshot: debug_packets::DiagLogSnapshot,
}

fn ensure_debug_packet_sinks(
    routes: &[RouteRuntime],
    sinks: &mut HashMap<u32, debug_packets::DebugPacketSink>,
    disabled: &mut HashSet<u32>,
    quiet: bool,
) {
    for route in routes
        .iter()
        .filter(|route| route.route.pico.persona == Persona::Debug)
    {
        ensure_debug_packet_sink_for_target(&route.route.pico, sinks, disabled, quiet);
    }
}

fn ensure_debug_packet_sink_for_target(
    target: &PicoTarget,
    sinks: &mut HashMap<u32, debug_packets::DebugPacketSink>,
    disabled: &mut HashSet<u32>,
    quiet: bool,
) {
    let uid = target.info.unique_id_short;
    if sinks.contains_key(&uid) || disabled.contains(&uid) {
        return;
    }
    match debug_packets::DebugPacketSink::create(&target.uid_hex(), target.peer) {
        Ok(sink) => {
            tracing::info!(
                "debug-packets: capturing {} from {} into {}",
                target.uid_hex(),
                target.peer,
                sink.path().display()
            );
            if !quiet {
                println!(
                    "Debug USB packet capture: {} -> {}",
                    target.short_label(),
                    sink.path().display()
                );
            }
            sinks.insert(uid, sink);
        }
        Err(e) => {
            disabled.insert(uid);
            tracing::warn!(
                "debug-packets: disabled for {}: {e:#}",
                target.short_label()
            );
            if !quiet {
                println!(
                    "Debug USB packet capture could not open a retained log for {}: {e:#}",
                    target.short_label()
                );
            }
        }
    }
}

fn has_debug_packet_routes(routes: &[RouteRuntime], disabled: &HashSet<u32>) -> bool {
    routes.iter().any(|route| {
        route.route.pico.persona == Persona::Debug
            && !disabled.contains(&route.route.pico.info.unique_id_short)
    })
}

fn debug_packet_harvest_targets(
    routes: &[RouteRuntime],
    disabled: &HashSet<u32>,
) -> Vec<PicoTarget> {
    routes
        .iter()
        .filter(|route| {
            route.route.pico.persona == Persona::Debug
                && !disabled.contains(&route.route.pico.info.unique_id_short)
        })
        .map(|route| route.route.pico.clone())
        .collect()
}

async fn collect_debug_packet_harvests(targets: Vec<PicoTarget>) -> Vec<DebugPacketHarvestResult> {
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let started = Instant::now();
        let outcome =
            match debug_packets::capture_run_diag_log(target.peer, DEBUG_PACKET_HARVEST_TIMEOUT)
                .await
            {
                Ok(snapshot) => {
                    let lines = debug_packets::extract_usb_packet_lines(&snapshot.text);
                    Ok(DebugPacketHarvestOk {
                        raw_packet_lines: lines
                            .iter()
                            .filter(|line| line.starts_with("usb-packet "))
                            .count(),
                        stats_lines: lines
                            .iter()
                            .filter(|line| line.starts_with("usb-packet-stats "))
                            .count(),
                        event_lines: lines
                            .iter()
                            .filter(|line| line.starts_with("usb-event "))
                            .count(),
                        lines,
                        snapshot,
                    })
                }
                Err(e) => Err(format!("{e:#}")),
            };
        out.push(DebugPacketHarvestResult {
            target,
            duration_ms: duration_ms_u64(started.elapsed()),
            outcome,
        });
    }
    out
}

fn apply_debug_packet_harvests(
    results: Vec<DebugPacketHarvestResult>,
    sinks: &mut HashMap<u32, debug_packets::DebugPacketSink>,
    disabled: &mut HashSet<u32>,
    quiet: bool,
) {
    for result in results {
        let uid = result.target.info.unique_id_short;
        ensure_debug_packet_sink_for_target(&result.target, sinks, disabled, quiet);
        let Some(sink) = sinks.get_mut(&uid) else {
            continue;
        };
        let duration_ms = result.duration_ms;
        match result.outcome {
            Ok(ok) => {
                let written = match sink.append_lines(&ok.lines) {
                    Ok(written) => written,
                    Err(e) => {
                        tracing::warn!(
                            "debug-packets: write failed for {}: {e:#}",
                            result.target.short_label()
                        );
                        disabled.insert(uid);
                        continue;
                    }
                };
                let harvest_record = debug_packets::HarvestOkRecord {
                    duration_ms,
                    snapshot: ok.snapshot,
                    packet_lines: ok.lines.len(),
                    raw_packet_lines: ok.raw_packet_lines,
                    stats_lines: ok.stats_lines,
                    event_lines: ok.event_lines,
                    new_lines: written,
                };
                let lost_bytes = harvest_record.snapshot.lost_bytes;
                let chunk_count = harvest_record.snapshot.chunk_count;
                let missing_chunk_count = harvest_record.snapshot.missing_chunks.len();
                let duplicate_chunk_count = harvest_record.snapshot.duplicate_chunk_count;
                if let Err(e) = sink.append_harvest_ok(harvest_record) {
                    tracing::warn!(
                        "debug-packets: harvest metadata write failed for {}: {e:#}",
                        result.target.short_label()
                    );
                    disabled.insert(uid);
                    continue;
                }
                tracing::debug!(
                    "debug-packets: harvest {} duration_ms={} chunks={} lost={} packets={} new={} total={}",
                    result.target.short_label(),
                    duration_ms,
                    chunk_count,
                    lost_bytes,
                    ok.lines.len(),
                    written,
                    sink.total_written()
                );
                if missing_chunk_count > 0 || duplicate_chunk_count > 0 {
                    tracing::debug!(
                        "debug-packets: harvest {} chunk health missing={} duplicate={}",
                        result.target.short_label(),
                        missing_chunk_count,
                        duplicate_chunk_count
                    );
                }
                if written > 0 || lost_bytes > 0 || missing_chunk_count > 0 {
                    pico_cache::record(
                        pico_cache::PicoStateSnapshot::from_target(
                            "debug-packet-harvest",
                            &result.target,
                        )
                        .with_outcome(format!(
                            "new_packets={written}; total_packets={}; lost_bytes={}; chunks={}; missing_chunks={}",
                            sink.total_written(),
                            lost_bytes,
                            chunk_count,
                            missing_chunk_count
                        )),
                    );
                }
            }
            Err(e) => {
                if let Err(write_error) = sink.append_harvest_error(duration_ms, &e) {
                    tracing::warn!(
                        "debug-packets: harvest failure metadata write failed for {}: {write_error:#}",
                        result.target.short_label()
                    );
                    disabled.insert(uid);
                    continue;
                }
                tracing::debug!(
                    "debug-packets: harvest failed for {} duration_ms={duration_ms}: {e}",
                    result.target.short_label(),
                );
            }
        }
    }
    debug_packets::prune_packet_files();
}

fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
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

fn bluetooth_cdc_frame_from_packet(packet: &Packet) -> Result<(u8, [u8; 13])> {
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

fn is_usb_output_persona(persona: Persona) -> bool {
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

fn refresh_bluetooth_statuses(routes: &mut [RouteRuntime]) {
    for route in routes {
        route.refresh_bluetooth_status();
    }
}

pub fn print_bluetooth_pairing_help(persona: Persona) {
    if !persona.is_bluetooth() {
        return;
    }
    let expected_name = bluetooth_expected_name(persona);
    println!();
    println!("Bluetooth mode setup");
    println!("  Keep this Pico plugged into the bridge PC over USB.");
    println!("  The Pico will advertise as {expected_name}.");
    println!("  Put the receiver or console adapter into Bluetooth pairing/search mode.");
    println!("  Pair the receiver with {expected_name}. Use PIN 0000 if it asks for one.");
    println!("  Persona switching still uses Wi-Fi; live controller input then uses PC USB.");
}

fn bluetooth_expected_name(persona: Persona) -> &'static str {
    match persona {
        Persona::BluetoothXbox => "Xbox Wireless Controller",
        Persona::BluetoothPlaystation => "Wireless Controller",
        Persona::BluetoothHid => "CouchLink BT HID",
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

fn format_bluetooth_peer_state(
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
    if !status.connected() {
        let name = bluetooth_display_name(status);
        let mut msg = format!("discoverable as \"{name}\"; pair receiver/search mode, PIN 0000");
        if status.last_status != 0 {
            msg.push_str(&format!("; last status 0x{:02X}", status.last_status));
        }
        if status.close_count > 0 {
            msg.push_str(&format!("; disconnects {}", status.close_count));
        }
        return msg;
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
    if status.get_report_count > 0 {
        msg.push_str(&format!(
            "; GET_REPORT ok {}/{}",
            status.get_report_success_count, status.get_report_count
        ));
        if status.get_report_unsupported_count > 0 {
            msg.push_str(&format!(
                " rejected {}",
                status.get_report_unsupported_count
            ));
        }
        if status.last_get_report_len > 0 {
            msg.push_str(&format!(
                "; last GET {} 0x{:02X} len {}",
                hid_report_type_name(status.last_get_report_type),
                status.last_get_report_id,
                status.last_get_report_len
            ));
        }
    }
    if status.set_report_count > 0 {
        msg.push_str(&format!(
            "; SET_REPORT accepted {}/{}",
            status.set_report_accepted_count, status.set_report_count
        ));
        if status.set_report_unsupported_count > 0 {
            msg.push_str(&format!(" ignored {}", status.set_report_unsupported_count));
        }
        if status.last_set_report_len > 0 {
            msg.push_str(&format!(
                "; last SET {} 0x{:02X} len {}",
                hid_report_type_name(status.last_set_report_type),
                status.last_set_report_id,
                status.last_set_report_len
            ));
        }
    }
    if status.out_report_count > 0 {
        msg.push_str(&format!(
            "; interrupt OUT accepted {}/{}",
            status.out_report_accepted_count, status.out_report_count
        ));
        if status.out_report_unsupported_count > 0 {
            msg.push_str(&format!(" ignored {}", status.out_report_unsupported_count));
        }
        if status.last_out_report_len > 0 {
            msg.push_str(&format!(
                "; last OUT {} 0x{:02X} len {}",
                hid_report_type_name(status.last_out_report_type),
                status.last_out_report_id,
                status.last_out_report_len
            ));
        }
    }
    if status.close_count > 0 {
        msg.push_str(&format!("; disconnects {}", status.close_count));
    }
    msg
}

fn bluetooth_display_name(status: &cdc::BtStatus) -> &str {
    if status.local_name.is_empty() {
        "CouchLink BT HID"
    } else {
        &status.local_name
    }
}

fn should_print_bluetooth_pairing_hint(status: Option<&cdc::BtStatus>) -> bool {
    status.map(|status| !status.connected()).unwrap_or(true)
}

fn short_error(error: &anyhow::Error) -> String {
    let text = error.to_string();
    const MAX_LEN: usize = 120;
    if text.len() <= MAX_LEN {
        text
    } else {
        let prefix: String = text.chars().take(MAX_LEN).collect();
        format!("{prefix}...")
    }
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
                "  {} -> {} | {} | USB input +{} total {} | Bluetooth {} | {} {}",
                route.route.source_label(),
                route.route.pico.uid_hex(),
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
