mod bluetooth;
mod bluetooth_source;
mod debug_harvest;
mod recovery;
mod route_runtime;
mod routing;
mod status;
mod stream;

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::protocol::{self, Persona};
use crate::{cmd_flash, config, discovery, journal, net, pico_cache, support, xinput};

pub(crate) use bluetooth::print_bluetooth_pairing_help;
use bluetooth_source::{
    BluetoothSourceSlotDecision, bluetooth_source_slot_decision, format_xinput_snapshot,
    prepare_bluetooth_routes, should_auto_switch_bluetooth_source,
};
use recovery::{
    merge_unique_picos, recover_setup_usb_to_wifi, recovered_target_count,
    wait_for_wifi_after_recovery,
};
pub(crate) use recovery::{probe_pico_ip, run_recover_command};
use route_runtime::RouteRuntime;
#[cfg(test)]
use routing::parse_user_slot;
pub(crate) use routing::{
    auto_routes, identity_from_target, match_pico_selector, parse_ip_selector, parse_route_specs,
};
use routing::{manual_ips_from_options, routes_from_saved, select_picos_by_specs};
pub(crate) use stream::stream_routes;

const DEFAULT_DISCOVER_SECONDS: u64 = 5;
const DEFAULT_STATUS_SECONDS: u64 = 2;
const STREAM_TICK: Duration = Duration::from_millis(16);
const PEER_STALE_AFTER: Duration = Duration::from_secs(5);
const PEER_RECOVER_EVERY: Duration = Duration::from_secs(10);
const PEER_RECOVERY_DISCOVER: Duration = Duration::from_secs(2);
const BLUETOOTH_SOURCE_WAIT: Duration = Duration::from_secs(10);
const BLUETOOTH_SOURCE_POLL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub(crate) struct RunOptions {
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
pub(crate) struct PicoTarget {
    pub peer: SocketAddr,
    pub info: protocol::AckInfo,
    pub persona: Persona,
    pub ack_flags: u8,
}

impl PicoTarget {
    pub(crate) fn uid_hex(&self) -> String {
        format!("{:08X}", self.info.unique_id_short)
    }

    pub(crate) fn board_label(&self) -> &'static str {
        match self.info.board_type {
            protocol::BOARD_PICO_2_W => "Pico 2 W",
            protocol::BOARD_PICO_W_RP2040 => "Pico W",
            _ => "Pico",
        }
    }

    pub(crate) fn short_label(&self) -> String {
        format!(
            "{} {} at {}",
            self.board_label(),
            self.uid_hex(),
            self.peer.ip()
        )
    }

    pub(crate) fn detail_label(&self) -> String {
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
pub(crate) struct StreamRoute {
    pub source_slot: u32,
    pub pico: PicoTarget,
}

impl StreamRoute {
    pub(crate) fn label(&self) -> String {
        format!("{} -> {}", self.source_label(), self.pico.short_label())
    }

    pub(crate) fn source_label(&self) -> String {
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
pub(crate) struct StreamOptions {
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

pub(crate) async fn run(options: RunOptions) -> Result<()> {
    tracing::info!("run: starting, bridge v{}", crate::VERSION);
    journal!("run", "started bridge v{}", crate::VERSION);

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

pub(crate) async fn discover_picos(timeout: Duration) -> Result<Vec<PicoTarget>> {
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

pub(crate) async fn discover_picos_with_auto_recovery(
    timeout: Duration,
    quiet: bool,
) -> Result<Vec<PicoTarget>> {
    let mut picos = discover_picos(timeout).await?;

    if picos.is_empty() && !quiet {
        outln!("No Pico replied on Wi-Fi. Checking recoverable USB states...");
    }

    let baseline_ids: HashSet<u32> = picos.iter().map(|p| p.info.unique_id_short).collect();
    let recovered = recover_setup_usb_to_wifi(quiet).await?;
    if recovered > 0 {
        if !quiet {
            outln!("Waiting for the recovered Pico to answer on Wi-Fi...");
        }
        let recovered_picos =
            wait_for_wifi_after_recovery(Duration::from_secs(60), quiet, &baseline_ids, recovered)
                .await?;
        let recovered_count = recovered_target_count(&recovered_picos, &baseline_ids);
        if recovered_count < recovered && !quiet {
            outln!(
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
        outln!("Found Pico board(s) in BOOTSEL firmware mode:");
        for (mount, board) in bootsel {
            outln!("  {}  {}", mount.display(), board.label());
        }
        outln!("Next step: choose `Update Pico firmware` or run `couchlink flash --all`.");
    }

    Ok(Vec::new())
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

async fn run_legacy_single() -> Result<()> {
    outln!("Looking for a Pico on the LAN...");
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

    if !protocol::proto_supported(pico.info.proto_version) {
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
