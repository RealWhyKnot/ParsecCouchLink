//! `couchlink run` -- direct streaming mode. The no-argument
//! `couchlink` entrypoint wraps this with a guided menu, while this
//! module keeps the scriptable route syntax for startup shortcuts and
//! third-party launchers.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::protocol::{self, GamepadState, Packet, PacketKind, FLAG_PARSEC_CONNECTED};
use crate::{cdc, cmd_flash, config, discovery, journal, support, xinput};

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
            "{} {}  {}  fw v{}  uptime {}s",
            self.board_label(),
            self.uid_hex(),
            self.peer,
            self.info.firmware_version(),
            self.info.uptime_seconds,
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
        format!(
            "{} -> {}",
            xinput::user_slot_label(self.source_slot),
            self.pico.short_label()
        )
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
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("binding UDP discovery socket")?;
    let found = discovery::collect(&socket, timeout)
        .await
        .context("collecting Pico discovery replies")?;
    Ok(found
        .into_iter()
        .map(|(peer, info)| PicoTarget { peer, info })
        .collect())
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
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("binding UDP manual-IP probe socket")?;
    let Some((peer, info)) = discovery::probe_ip(&socket, ip, timeout)
        .await
        .with_context(|| format!("probing Pico at {ip}:{}", protocol::PORT))?
    else {
        bail!(
            "no Pico replied at {ip}:{} within {} s",
            protocol::PORT,
            timeout.as_secs()
        );
    };
    if info.proto_version != protocol::PROTO_VERSION {
        bail!(
            "Pico at {peer} speaks protocol v{}, bridge speaks v{}. Update whichever side is older.",
            info.proto_version,
            protocol::PROTO_VERSION,
        );
    }
    Ok(PicoTarget { peer, info })
}

async fn recover_setup_usb_to_wifi(quiet: bool) -> Result<usize> {
    let ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        return Ok(0);
    }

    let mut rebooted = 0usize;
    let mut blocked = 0usize;
    if !quiet {
        println!("Found setup-mode USB Pico port(s): {}", ports.join(", "));
    }

    for port in ports {
        match setup_port_reboot_to_run(port.clone()).await {
            Ok(SetupRecovery::Rebooted { firmware, board }) => {
                rebooted += 1;
                if !quiet {
                    println!("  {port}: fw v{firmware} {board} -> Wi-Fi/controller mode");
                }
            }
            Ok(SetupRecovery::NoCredentials { firmware, board }) => {
                blocked += 1;
                if !quiet {
                    println!(
                        "  {port}: fw v{firmware} {board} has no saved Wi-Fi; choose `Set up or change Wi-Fi`."
                    );
                }
            }
            Err(e) => {
                blocked += 1;
                if !quiet {
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

enum SetupRecovery {
    Rebooted {
        firmware: String,
        board: &'static str,
    },
    NoCredentials {
        firmware: String,
        board: &'static str,
    },
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
        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }

        let firmware = hello.firmware_version().to_string();
        let board = setup_board_label(hello.board_type);
        if !hello.creds_present() {
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
    if options.save_routes {
        save_routes(&routes)?;
    }

    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("binding UDP stream socket")?;
    socket
        .set_broadcast(true)
        .context("enabling broadcast on UDP stream socket")?;

    if !options.quiet {
        print_stream_intro(&routes, socket.local_addr()?);
    }

    let mut runtime: Vec<RouteRuntime> = routes.into_iter().map(RouteRuntime::new).collect();
    let mut tick = interval(STREAM_TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut status = interval(Duration::from_secs(options.status_seconds.max(1)));
    status.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut buf = [0u8; 64];
    let (recovery_tx, mut recovery_rx) = mpsc::channel::<Result<Vec<PicoTarget>>>(1);
    let mut recovery_in_flight = false;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                for route in &mut runtime {
                    if let Err(e) = route.send_tick(&socket).await {
                        return Err(e).with_context(|| format!("streaming to {}", route.route.pico.short_label()));
                    }
                }
            }
            recv = socket.recv_from(&mut buf) => {
                match recv {
                    Ok((n, from)) => handle_stream_reply(&mut runtime, from, &buf[..n]),
                    Err(e) => return Err(e).context("receiving stream reply"),
                }
            }
            _ = status.tick() => {
                if !options.quiet {
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
            recovery = recovery_rx.recv() => {
                recovery_in_flight = false;
                match recovery {
                    Some(Ok(picos)) => apply_recovery_results(&mut runtime, &picos, options.quiet),
                    Some(Err(e)) if !options.quiet => {
                        println!("Recovery discovery failed: {e:#}");
                        println!("  Check Wi-Fi, firewall, and router client isolation, then run `couchlink test discover --all`.");
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
    if let Some(first) = routes.first() {
        cfg.last_pico = Some(config::PicoIdentity {
            unique_id_short: first.pico.info.unique_id_short,
            board_type: first.pico.info.board_type,
            fw_major: first.pico.info.fw_major,
            fw_minor: first.pico.info.fw_minor,
            fw_patch: first.pico.info.fw_patch,
            last_ip: Some(first.pico.peer.ip().to_string()),
            device_name: Some(first.pico.board_label().to_string()),
        });
    }
    config::save(&cfg)
}

struct RouteRuntime {
    route: StreamRoute,
    seq: u8,
    sent_total: u64,
    sent_at_last_status: u64,
    inbound_total: u64,
    last_inbound: Instant,
    last_state: GamepadState,
    last_packet_number: Option<u32>,
    source_connected: bool,
    last_send_type: &'static str,
    last_recovery_attempt: Option<Instant>,
    recovery_hint_printed: bool,
}

impl RouteRuntime {
    fn new(route: StreamRoute) -> Self {
        Self {
            route,
            seq: 0,
            sent_total: 0,
            sent_at_last_status: 0,
            inbound_total: 0,
            last_inbound: Instant::now(),
            last_state: GamepadState::default(),
            last_packet_number: None,
            source_connected: false,
            last_send_type: "heartbeat",
            last_recovery_attempt: None,
            recovery_hint_printed: false,
        }
    }

    async fn send_tick(&mut self, socket: &UdpSocket) -> Result<()> {
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
        self.seq = self.seq.wrapping_add(1);
        socket
            .send_to(&packet.encode(), self.route.pico.peer)
            .await?;
        self.sent_total += 1;
        self.last_state = state;
        self.last_packet_number = packet_number;
        self.source_connected = connected;
        Ok(())
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
    println!("Local UDP socket: {local_addr}");
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
        let inbound_age = route.last_inbound.elapsed();
        let peer_state = if route.inbound_total == 0 {
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
        println!(
            "  {} -> {} | {} | out +{} total {} | in {} ({}) | {} buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
            xinput::user_slot_label(route.route.source_slot),
            route.route.pico.uid_hex(),
            source_state,
            sent_delta,
            route.sent_total,
            route.inbound_total,
            peer_state,
            route.last_send_type,
            route.last_state.buttons,
            route.last_state.left_trigger,
            route.last_state.right_trigger,
            route.last_state.left_x,
            route.last_state.left_y,
            route.last_state.right_x,
            route.last_state.right_y,
        );
        if route.inbound_total == 0 && route.sent_total > 180 && !route.recovery_hint_printed {
            println!(
                "    hint: no Pico reply yet. Confirm this Pico is powered, on the same Wi-Fi, and visible in `couchlink test discover --all`."
            );
            route.recovery_hint_printed = true;
        } else if route.inbound_total > 0
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
mod tests {
    use super::*;

    fn pico(uid: u32, ip: &str, board: u8) -> PicoTarget {
        PicoTarget {
            peer: format!("{ip}:4242").parse().unwrap(),
            info: protocol::AckInfo {
                proto_version: protocol::PROTO_VERSION,
                fw_major: 26,
                fw_minor: 5,
                fw_patch: 30,
                board_type: board,
                uptime_seconds: 12,
                unique_id_short: uid,
            },
        }
    }

    #[test]
    fn parse_user_slot_is_one_based() {
        assert_eq!(parse_user_slot("1").unwrap(), 0);
        assert_eq!(parse_user_slot("P4").unwrap(), 3);
        assert!(parse_user_slot("0").is_err());
        assert!(parse_user_slot("5").is_err());
    }

    #[test]
    fn match_pico_by_uid_ip_and_board() {
        let picos = vec![
            pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        ];
        assert_eq!(
            match_pico_selector("07D37EB6", &picos)
                .unwrap()
                .info
                .unique_id_short,
            0x07D37EB6
        );
        assert_eq!(
            match_pico_selector("192.168.50.4", &picos)
                .unwrap()
                .info
                .unique_id_short,
            0x523861E6
        );
        assert_eq!(
            match_pico_selector("192.168.50.4:4242", &picos)
                .unwrap()
                .info
                .unique_id_short,
            0x523861E6
        );
        assert_eq!(
            match_pico_selector("rp2040", &picos)
                .unwrap()
                .info
                .unique_id_short,
            0x523861E6
        );
    }

    #[test]
    fn parse_ip_selector_accepts_ip_and_socket_addr() {
        assert_eq!(
            parse_ip_selector("192.168.50.4"),
            Some("192.168.50.4".parse().unwrap())
        );
        assert_eq!(
            parse_ip_selector("192.168.50.4:4242"),
            Some("192.168.50.4".parse().unwrap())
        );
        assert_eq!(parse_ip_selector("07D37EB6"), None);
    }

    #[test]
    fn manual_ips_include_pico_and_route_targets() {
        let options = RunOptions {
            picos: vec!["192.168.50.4".to_string(), "07D37EB6".to_string()],
            routes: vec![
                "1=192.168.50.226".to_string(),
                "2:192.168.50.4:4242".to_string(),
            ],
            ..RunOptions::default()
        };
        assert_eq!(
            manual_ips_from_options(&options),
            vec![
                "192.168.50.4".parse::<IpAddr>().unwrap(),
                "192.168.50.226".parse::<IpAddr>().unwrap()
            ]
        );
    }

    #[test]
    fn merge_unique_picos_updates_existing_and_adds_new() {
        let mut picos = vec![pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W)];
        let incoming = vec![
            pico(0x07D37EB6, "192.168.50.227", protocol::BOARD_PICO_2_W),
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        ];

        merge_unique_picos(&mut picos, incoming);

        assert_eq!(picos.len(), 2);
        assert_eq!(picos[0].peer.ip().to_string(), "192.168.50.227");
        assert_eq!(picos[1].info.unique_id_short, 0x523861E6);
    }

    #[test]
    fn recovered_target_count_ignores_already_online_picos() {
        let baseline_ids = HashSet::from([0x523861E6]);
        let picos = vec![
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
            pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
        ];

        assert_eq!(recovered_target_count(&picos, &baseline_ids), 1);
    }

    #[test]
    fn parse_route_specs_maps_sources_to_targets() {
        let picos = vec![
            pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        ];
        let specs = vec!["1=07D37EB6".to_string(), "2=192.168.50.4".to_string()];
        let routes = parse_route_specs(&specs, &picos).unwrap();
        assert_eq!(routes[0].source_slot, 0);
        assert_eq!(routes[0].pico.info.unique_id_short, 0x07D37EB6);
        assert_eq!(routes[1].source_slot, 1);
        assert_eq!(routes[1].pico.info.unique_id_short, 0x523861E6);
    }

    #[test]
    fn validate_routes_rejects_same_pico_twice() {
        let target = pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W);
        let routes = vec![
            StreamRoute {
                source_slot: 0,
                pico: target.clone(),
            },
            StreamRoute {
                source_slot: 1,
                pico: target,
            },
        ];
        assert!(validate_routes(&routes).is_err());
    }
}
