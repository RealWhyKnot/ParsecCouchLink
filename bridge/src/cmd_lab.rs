//! `couchlink lab` -- unattended hardware bench checks for plugged-in Picos.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;
use zeroize::Zeroize;

use crate::{cdc, cmd_flash, cmd_run, cmd_usb_diag, config, net, pico_mode, protocol, xinput};

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(5);
const SETUP_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const WIFI_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const BOOTSEL_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const POWER_OFF_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const SIGNAL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const SIGNAL_SEND_INTERVAL: Duration = Duration::from_millis(16);
const SIGNAL_NEUTRALIZE_DURATION: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum LabScenario {
    Full,
    ModeCycle,
    FlashCycle,
    PowerCycle,
    Status,
}

impl LabScenario {
    fn includes_mode_cycle(self) -> bool {
        matches!(self, LabScenario::Full | LabScenario::ModeCycle)
    }

    fn includes_flash_cycle(self) -> bool {
        matches!(self, LabScenario::Full | LabScenario::FlashCycle)
    }

    fn includes_power_cycle(self) -> bool {
        matches!(self, LabScenario::Full | LabScenario::PowerCycle)
    }

    fn includes_final_run_check(self) -> bool {
        matches!(self, LabScenario::Full)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum LabPower {
    Auto,
    Reset,
    External,
    PnpRestart,
}

#[derive(Clone, Debug)]
pub struct LabOptions {
    pub all: bool,
    pub picos: Vec<String>,
    pub scenario: LabScenario,
    pub cycles: u32,
    pub power: LabPower,
    pub uf2: Option<PathBuf>,
    pub json: Option<PathBuf>,
    pub no_flash: bool,
}

pub async fn run(options: LabOptions) -> Result<()> {
    if options.cycles == 0 {
        bail!("--cycles must be at least 1");
    }

    let mut report = LabReport::new(&options);
    println!("couchlink lab");
    println!(
        "scenario={:?} cycles={} power={:?}",
        options.scenario, options.cycles, options.power
    );
    println!();

    let cfg = config::load().unwrap_or_default();
    let selectors = if options.all {
        Vec::new()
    } else {
        parse_selectors(&options.picos)?
    };
    let selected_power = select_power_backend(options.power, cfg.lab_power.as_ref(), &mut report);
    report.power_selected = selected_power.name().to_string();

    let mut lab = LabHarness {
        options,
        cfg,
        selectors,
        selected_power,
        report,
    };
    lab.run_scenario().await?;
    lab.report.print_summary();

    if let Some(path) = lab.options.json.as_deref() {
        let text = serde_json::to_string_pretty(&lab.report)?;
        tokio::fs::write(path, text)
            .await
            .with_context(|| format!("writing lab report {}", path.display()))?;
        println!("JSON report: {}", path.display());
    }

    if lab.report.fail_count() > 0 {
        bail!("lab failed with {} failed step(s)", lab.report.fail_count());
    }
    Ok(())
}

struct LabHarness {
    options: LabOptions,
    cfg: config::Config,
    selectors: Vec<PicoSelector>,
    selected_power: SelectedPower,
    report: LabReport,
}

impl LabHarness {
    async fn run_scenario(&mut self) -> Result<()> {
        self.record_status_snapshot().await;
        if self.options.scenario == LabScenario::Status {
            return Ok(());
        }

        if self.options.scenario.includes_flash_cycle() {
            self.recover_initial_bootsel().await;
        }

        let mut probes = self.normalize_selected_boards_to_setup().await?;
        if probes.is_empty() {
            self.report.fail(
                "select boards",
                None,
                "no selected Pico was found in Wi-Fi, setup USB, or recoverable BOOTSEL mode",
                0,
            );
            return Ok(());
        }

        for cycle in 1..=self.options.cycles {
            println!();
            println!("Lab cycle {cycle}/{}", self.options.cycles);

            if self.options.scenario.includes_mode_cycle() {
                probes = self.run_mode_cycles(probes, cycle).await;
            }

            if self.options.scenario.includes_power_cycle() {
                probes = self.run_power_cycle(probes, cycle).await;
            }

            if self.options.scenario.includes_flash_cycle() {
                probes = self.run_flash_cycles(probes, cycle).await;
            }
        }

        if self.options.scenario.includes_final_run_check() {
            self.run_final_run_checks(probes).await;
        }

        Ok(())
    }

    async fn record_status_snapshot(&mut self) {
        let started = Instant::now();
        let mut details = Vec::new();

        match probe_setup_ports().await {
            Ok(setup) => {
                self.report
                    .devices
                    .extend(setup.iter().map(LabDevice::from_setup_probe));
                details.push(format!("setup={}", setup.len()));
            }
            Err(e) => details.push(format!("setup=error({e:#})")),
        }

        match cmd_run::discover_picos(DISCOVER_TIMEOUT).await {
            Ok(wifi) => {
                self.report
                    .devices
                    .extend(wifi.iter().map(LabDevice::from_wifi_target));
                details.push(format!("wifi={}", wifi.len()));
            }
            Err(e) => details.push(format!("wifi=error({e:#})")),
        }

        let bootsel = cmd_flash::visible_bootsel_mounts();
        self.report
            .devices
            .extend(bootsel.iter().map(LabDevice::from_bootsel));
        details.push(format!("bootsel={}", bootsel.len()));

        let slots = xinput::connected_slots();
        details.push(format!("xinput={}", slots.len()));

        let problem_usb = problem_usb_devices();
        for problem in &problem_usb {
            self.report.devices.push(LabDevice {
                mode: "problem-usb".to_string(),
                uid: None,
                board: None,
                address: None,
                detail: Some(problem.clone()),
            });
        }
        details.push(format!("problem-usb={}", problem_usb.len()));

        self.report.pass(
            "status snapshot",
            None,
            details.join(", "),
            started.elapsed().as_millis(),
        );
    }

    async fn recover_initial_bootsel(&mut self) {
        let mounts = selected_bootsel_mounts(&self.selectors, cmd_flash::visible_bootsel_mounts());
        if mounts.is_empty() {
            return;
        }

        if self.options.no_flash {
            self.report.skip(
                "initial BOOTSEL recovery",
                None,
                "BOOTSEL drive is visible, but --no-flash was set",
                0,
            );
            return;
        }

        for (mount, board) in mounts {
            let started = Instant::now();
            let result = async {
                let uf2 = cmd_flash::resolve_uf2_path(self.options.uf2.as_deref(), board)?;
                let outcome = cmd_flash::flash_uf2_to_mount(&uf2, mount, board, 0).await?;
                Ok::<_, anyhow::Error>(outcome)
            }
            .await;
            match result {
                Ok(outcome) => self.report.pass(
                    "initial BOOTSEL recovery",
                    None,
                    format!(
                        "{} flashed from {} ({} bytes)",
                        outcome.board.label(),
                        outcome.uf2_path.display(),
                        outcome.bytes_written
                    ),
                    started.elapsed().as_millis(),
                ),
                Err(e) => self.report.fail(
                    "initial BOOTSEL recovery",
                    None,
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                ),
            }
        }
    }

    async fn normalize_selected_boards_to_setup(&mut self) -> Result<Vec<SetupLabProbe>> {
        let mut selected = Vec::new();

        let setup = probe_setup_ports().await?;
        for probe in setup {
            if setup_probe_selected(&self.selectors, &probe) {
                selected.push(probe);
            }
        }

        let wifi = cmd_run::discover_picos(DISCOVER_TIMEOUT).await?;
        for target in wifi {
            if !wifi_target_selected(&self.selectors, &target) {
                continue;
            }
            if selected
                .iter()
                .any(|p| p.uid == target.info.unique_id_short)
            {
                continue;
            }

            let uid = target.info.unique_id_short;
            let started = Instant::now();
            println!("Moving {} to setup USB...", target.short_label());
            let result = async {
                pico_mode::request_reboot_to_setup(&target).await?;
                wait_for_setup_uid(uid, SETUP_WAIT_TIMEOUT).await
            }
            .await;
            match result {
                Ok(mut probe) => {
                    probe.last_ip = Some(target.peer.ip());
                    self.report.pass(
                        "normalize Wi-Fi to setup",
                        Some(uid),
                        format!("{} -> {}", target.peer.ip(), probe.port),
                        started.elapsed().as_millis(),
                    );
                    selected.push(probe);
                }
                Err(e) => self.report.fail(
                    "normalize Wi-Fi to setup",
                    Some(uid),
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                ),
            }
        }

        Ok(dedup_setup_probes(selected))
    }

    async fn run_mode_cycles(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        let mut next = Vec::new();
        for probe in probes {
            let uid = probe.uid;
            let started = Instant::now();
            println!("Mode cycle for {} on {}...", probe.uid_hex(), probe.port);
            match self.mode_cycle_one(probe).await {
                Ok(updated) => {
                    self.report.pass(
                        format!("mode cycle {cycle}"),
                        Some(uid),
                        format!("setup -> Wi-Fi -> setup on {}", updated.port),
                        started.elapsed().as_millis(),
                    );
                    next.push(updated);
                }
                Err((failed_probe, e)) => {
                    self.report.fail(
                        format!("mode cycle {cycle}"),
                        Some(uid),
                        format!("{e:#}"),
                        started.elapsed().as_millis(),
                    );
                    next.push(failed_probe);
                }
            }
        }
        dedup_setup_probes(next)
    }

    async fn mode_cycle_one(
        &self,
        probe: SetupLabProbe,
    ) -> std::result::Result<SetupLabProbe, (SetupLabProbe, anyhow::Error)> {
        if let Err(e) = ensure_wifi_ready(&probe).await {
            return Err((probe, e));
        }

        if let Err(e) = reboot_setup_to_run(probe.port.clone()).await {
            return Err((probe, e));
        }

        let target = match wait_for_wifi_uid(probe.uid, WIFI_WAIT_TIMEOUT).await {
            Ok(target) => target,
            Err(e) => return Err((probe, e)),
        };
        let last_ip = Some(target.peer.ip());

        if let Err(e) = pico_mode::request_reboot_to_setup(&target).await {
            return Err((probe, e));
        }

        match wait_for_setup_or_wifi_uid(probe.uid, last_ip, SETUP_WAIT_TIMEOUT).await {
            Ok(SetupOrWifiMode::Setup(mut updated)) => {
                updated.last_ip = last_ip;
                Ok(updated)
            }
            Ok(SetupOrWifiMode::Wifi(target)) => {
                if let Err(e) = pico_mode::request_reboot_to_setup(&target).await {
                    return Err((probe, e));
                }
                match wait_for_setup_uid(probe.uid, SETUP_WAIT_TIMEOUT).await {
                    Ok(mut updated) => {
                        updated.last_ip = Some(target.peer.ip());
                        Ok(updated)
                    }
                    Err(e) => Err((probe, e)),
                }
            }
            Err(e) => Err((probe, e)),
        }
    }

    async fn run_power_cycle(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        match self.selected_power.kind {
            SelectedPowerKind::External => self.run_external_power_cycle(probes, cycle).await,
            SelectedPowerKind::PnpRestart => self.run_pnp_restart_cycle(probes, cycle).await,
            SelectedPowerKind::Reset => self.run_reset_power_cycle(probes, cycle).await,
        }
    }

    async fn run_reset_power_cycle(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        let mut next = Vec::new();
        for probe in probes {
            let uid = probe.uid;
            let started = Instant::now();
            match self.mode_cycle_one(probe).await {
                Ok(updated) => {
                    self.report.pass(
                        format!("power cycle {cycle}"),
                        Some(uid),
                        "firmware reset re-enumeration; USB power was not cut",
                        started.elapsed().as_millis(),
                    );
                    next.push(updated);
                }
                Err((failed_probe, e)) => {
                    self.report.fail(
                        format!("power cycle {cycle}"),
                        Some(uid),
                        format!("{e:#}"),
                        started.elapsed().as_millis(),
                    );
                    next.push(failed_probe);
                }
            }
        }
        dedup_setup_probes(next)
    }

    async fn run_external_power_cycle(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        let Some(power) = self.cfg.lab_power.as_ref() else {
            self.report.fail(
                format!("power cycle {cycle}"),
                None,
                "external power selected but [lab_power] is not configured",
                0,
            );
            return probes;
        };
        let uids: BTreeSet<u32> = probes.iter().map(|p| p.uid).collect();
        let started = Instant::now();

        let result = async {
            run_configured_command(&power.off).context("external power off failed")?;
            wait_for_setup_uids_absent(&uids, POWER_OFF_WAIT_TIMEOUT).await?;
            run_configured_command(&power.on).context("external power on failed")?;
            wait_for_setup_uids(&uids, SETUP_WAIT_TIMEOUT).await
        }
        .await;

        match result {
            Ok(updated) => {
                self.report.pass(
                    format!("power cycle {cycle}"),
                    None,
                    "external power backend cut and restored visible setup USB devices",
                    started.elapsed().as_millis(),
                );
                updated
            }
            Err(e) => {
                self.report.fail(
                    format!("power cycle {cycle}"),
                    None,
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                );
                probes
            }
        }
    }

    async fn run_pnp_restart_cycle(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        let uids: BTreeSet<u32> = probes.iter().map(|p| p.uid).collect();
        let started = Instant::now();
        let result = async {
            restart_setup_pnp_devices()?;
            wait_for_setup_uids(&uids, SETUP_WAIT_TIMEOUT).await
        }
        .await;
        match result {
            Ok(updated) => {
                self.report.pass(
                    format!("power cycle {cycle}"),
                    None,
                    "PnP device restart completed; USB power was not cut",
                    started.elapsed().as_millis(),
                );
                updated
            }
            Err(e) => {
                self.report.fail(
                    format!("power cycle {cycle}"),
                    None,
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                );
                probes
            }
        }
    }

    async fn run_flash_cycles(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        if self.options.no_flash {
            for probe in &probes {
                self.report.skip(
                    format!("flash cycle {cycle}"),
                    Some(probe.uid),
                    "--no-flash set",
                    0,
                );
            }
            return probes;
        }

        let mut next = Vec::new();
        for probe in probes {
            let uid = probe.uid;
            let started = Instant::now();
            println!("Flash cycle for {} on {}...", probe.uid_hex(), probe.port);
            match self.flash_cycle_one(probe).await {
                Ok(updated) => {
                    self.report.pass(
                        format!("flash cycle {cycle}"),
                        Some(uid),
                        format!(
                            "BOOTSEL -> flash -> setup, USB fw v{}",
                            updated.hello.firmware_version()
                        ),
                        started.elapsed().as_millis(),
                    );
                    next.push(updated);
                }
                Err((failed_probe, e)) => {
                    self.report.fail(
                        format!("flash cycle {cycle}"),
                        Some(uid),
                        format!("{e:#}"),
                        started.elapsed().as_millis(),
                    );
                    next.push(failed_probe);
                }
            }
        }
        dedup_setup_probes(next)
    }

    async fn flash_cycle_one(
        &self,
        probe: SetupLabProbe,
    ) -> std::result::Result<SetupLabProbe, (SetupLabProbe, anyhow::Error)> {
        let before = bootsel_keys();
        if let Err(e) = reboot_setup_to_bootsel(probe.port.clone()).await {
            return Err((probe, e));
        }

        let (mount, board) = match wait_for_new_bootsel_mount(&before, BOOTSEL_WAIT_TIMEOUT).await {
            Ok(mount) => mount,
            Err(e) => return Err((probe, e)),
        };

        let uf2 = match cmd_flash::resolve_uf2_path(self.options.uf2.as_deref(), board) {
            Ok(uf2) => uf2,
            Err(e) => return Err((probe, e)),
        };

        if let Err(e) = cmd_flash::flash_uf2_to_mount(&uf2, mount, board, 0).await {
            return Err((probe, e));
        }

        match wait_for_setup_or_wifi_uid(probe.uid, probe.last_ip, SETUP_WAIT_TIMEOUT).await {
            Ok(SetupOrWifiMode::Setup(mut updated)) => {
                updated.last_ip = probe.last_ip;
                Ok(updated)
            }
            Ok(SetupOrWifiMode::Wifi(target)) => {
                if let Err(e) = pico_mode::request_reboot_to_setup(&target).await {
                    return Err((probe, e));
                }
                match wait_for_setup_uid(probe.uid, SETUP_WAIT_TIMEOUT).await {
                    Ok(mut updated) => {
                        updated.last_ip = Some(target.peer.ip());
                        Ok(updated)
                    }
                    Err(e) => Err((probe, e)),
                }
            }
            Err(e) => Err((probe, e)),
        }
    }

    async fn run_final_run_checks(&mut self, probes: Vec<SetupLabProbe>) {
        let mut run_targets = Vec::new();
        for probe in probes {
            let uid = probe.uid;
            let started = Instant::now();
            println!("Returning {} to run mode...", probe.uid_hex());
            let result = async {
                ensure_wifi_ready(&probe).await?;
                reboot_setup_to_run(probe.port.clone()).await?;
                let target = wait_for_wifi_uid(uid, WIFI_WAIT_TIMEOUT).await?;
                let diag = cmd_usb_diag::query_usb_diag(&target, Duration::from_secs(3)).await?;
                Ok::<_, anyhow::Error>((target, diag))
            }
            .await;

            match result {
                Ok((target, diag)) => {
                    self.report.pass(
                        "final run check",
                        Some(uid),
                        format!(
                            "{} fw v{} usb_mounted={} reports_sent={}",
                            target.peer,
                            target.info.firmware_version(),
                            diag.mounted(),
                            diag.xinput_report_sent()
                        ),
                        started.elapsed().as_millis(),
                    );
                    run_targets.push(target);
                }
                Err(e) => self.report.fail(
                    "final run check",
                    Some(uid),
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                ),
            }
        }
        self.run_signal_checks(&run_targets).await;
    }

    async fn run_signal_checks(&mut self, targets: &[cmd_run::PicoTarget]) {
        neutralize_targets(targets).await;
        for (idx, target) in targets.iter().enumerate() {
            let uid = target.info.unique_id_short;
            let state = lab_signal_state(idx);
            let started = Instant::now();
            let result = verify_signal_roundtrip(target, state).await;
            match result {
                Ok(slot) => self.report.pass(
                    "signal check",
                    Some(uid),
                    format!(
                        "{} -> {} exact buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
                        target.peer,
                        xinput::user_slot_label(slot),
                        state.buttons,
                        state.left_trigger,
                        state.right_trigger,
                        state.left_x,
                        state.left_y,
                        state.right_x,
                        state.right_y
                    ),
                    started.elapsed().as_millis(),
                ),
                Err(e) => self.report.fail(
                    "signal check",
                    Some(uid),
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                ),
            }
        }
    }
}

#[derive(Clone, Debug)]
struct SetupLabProbe {
    port: String,
    hello: cdc::HelloAck,
    self_test: cdc::SelfTestAck,
    uid: u32,
    last_ip: Option<IpAddr>,
    log_bytes: usize,
    log_lost: u32,
}

impl SetupLabProbe {
    fn uid_hex(&self) -> String {
        format!("{:08X}", self.uid)
    }

    fn board_label(&self) -> &'static str {
        board_type_label(self.hello.board_type)
    }
}

async fn probe_setup_ports() -> Result<Vec<SetupLabProbe>> {
    let ports = cdc::find_setup_ports()?;
    let mut probes = Vec::new();
    for port in ports {
        match probe_setup_port(port.clone()).await {
            Ok(probe) => probes.push(probe),
            Err(e) => tracing::warn!("lab: setup probe {port} failed: {e:#}"),
        }
    }
    Ok(probes)
}

async fn probe_setup_port(port: String) -> Result<SetupLabProbe> {
    tokio::task::spawn_blocking(move || -> Result<SetupLabProbe> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        if hello.proto_version != cdc::PROTO_VERSION {
            bail!(
                "Pico speaks CDC protocol v{}, bridge speaks v{}",
                hello.proto_version,
                cdc::PROTO_VERSION
            );
        }
        let self_test = pico.self_test()?;
        let uid = pico.unique_id_short()?;
        let (log_text, log_lost) = pico.get_log_buffer().unwrap_or_default();
        Ok(SetupLabProbe {
            port,
            hello,
            self_test,
            uid,
            last_ip: None,
            log_bytes: log_text.len(),
            log_lost,
        })
    })
    .await?
}

async fn reboot_setup_to_run(port: String) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        if !hello.creds_present() {
            bail!("Pico has no saved Wi-Fi credentials");
        }
        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }
        pico.reboot_to_run()?;
        Ok(())
    })
    .await?
}

async fn reboot_setup_to_bootsel(port: String) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }
        pico.reboot_to_bootsel()?;
        Ok(())
    })
    .await?
}

async fn ensure_wifi_ready(probe: &SetupLabProbe) -> Result<()> {
    if probe.hello.creds_present() {
        return Ok(());
    }

    let ssid = std::env::var("COUCHLINK_WIFI_SSID").unwrap_or_default();
    if ssid.is_empty() {
        bail!(
            "{} has no saved Wi-Fi credentials. Set COUCHLINK_WIFI_SSID and COUCHLINK_WIFI_PASSWORD for full unattended lab runs.",
            probe.uid_hex()
        );
    }
    if ssid.len() > 32 {
        bail!("COUCHLINK_WIFI_SSID can't be longer than 32 bytes");
    }
    let mut password = std::env::var("COUCHLINK_WIFI_PASSWORD").unwrap_or_default();
    if password.len() > 63 {
        password.zeroize();
        bail!("COUCHLINK_WIFI_PASSWORD can't be longer than 63 bytes");
    }
    let port = probe.port.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let result = (|| -> Result<()> {
            let mut pico = cdc::PicoSetup::open_named(&port)?;
            pico.set_wifi(&ssid, &mut password)?;
            Ok(())
        })();
        password.zeroize();
        result
    })
    .await?
}

async fn wait_for_setup_uid(uid: u32, timeout: Duration) -> Result<SetupLabProbe> {
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        let probes = probe_setup_ports().await?;
        if let Some(probe) = probes.into_iter().find(|p| p.uid == uid) {
            return Ok(probe);
        }
        if Instant::now() >= deadline {
            bail!(
                "Pico {uid:08X} did not appear in setup USB within {} s",
                timeout.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

enum SetupOrWifiMode {
    Setup(SetupLabProbe),
    Wifi(cmd_run::PicoTarget),
}

async fn wait_for_setup_or_wifi_uid(
    uid: u32,
    last_ip: Option<IpAddr>,
    timeout: Duration,
) -> Result<SetupOrWifiMode> {
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        let probes = probe_setup_ports().await?;
        if let Some(probe) = probes.into_iter().find(|p| p.uid == uid) {
            return Ok(SetupOrWifiMode::Setup(probe));
        }

        let now = Instant::now();
        if now >= deadline {
            bail!(
                "Pico {uid:08X} did not appear in setup USB or Wi-Fi within {} s",
                timeout.as_secs()
            );
        }

        let remaining = deadline.saturating_duration_since(now);
        let discover_for = remaining.min(Duration::from_millis(750));
        let picos = cmd_run::discover_picos(discover_for).await?;
        if let Some(target) = picos.into_iter().find(|p| p.info.unique_id_short == uid) {
            return Ok(SetupOrWifiMode::Wifi(target));
        }

        if let Some(ip) = last_ip {
            if let Some(target) = probe_known_ip_for_uid(ip, uid, discover_for).await? {
                return Ok(SetupOrWifiMode::Wifi(target));
            }
        }
    }
}

async fn probe_known_ip_for_uid(
    ip: IpAddr,
    uid: u32,
    timeout: Duration,
) -> Result<Option<cmd_run::PicoTarget>> {
    match cmd_run::probe_pico_ip(ip, timeout).await {
        Ok(target) if target.info.unique_id_short == uid => Ok(Some(target)),
        Ok(target) => {
            tracing::debug!(
                "lab: known IP {ip} replied as {:08X}, expected {uid:08X}",
                target.info.unique_id_short
            );
            Ok(None)
        }
        Err(e) => {
            tracing::debug!("lab: known IP {ip} did not answer for {uid:08X}: {e:#}");
            Ok(None)
        }
    }
}

async fn neutralize_targets(targets: &[cmd_run::PicoTarget]) {
    let Ok(socket) = net::bind_udp("0.0.0.0:0").await else {
        return;
    };
    for target in targets {
        let _ = send_state_burst(
            &socket,
            target.peer,
            protocol::GamepadState::default(),
            0,
            SIGNAL_NEUTRALIZE_DURATION,
        )
        .await;
    }
}

async fn verify_signal_roundtrip(
    target: &cmd_run::PicoTarget,
    state: protocol::GamepadState,
) -> Result<u32> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP signal-test socket")?;
    let matched = send_until_xinput_match(&socket, target.peer, state).await;
    let neutralized = send_state_burst(
        &socket,
        target.peer,
        protocol::GamepadState::default(),
        0,
        SIGNAL_NEUTRALIZE_DURATION,
    )
    .await;

    let slot = matched?;
    neutralized?;
    wait_for_slot_state(
        slot,
        protocol::GamepadState::default(),
        Duration::from_secs(2),
    )
    .await?;
    Ok(slot)
}

async fn send_until_xinput_match(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    state: protocol::GamepadState,
) -> Result<u32> {
    let started = Instant::now();
    let mut seq = 0u8;
    loop {
        send_state_once(socket, target, seq, state, protocol::FLAG_PARSEC_CONNECTED).await?;
        seq = seq.wrapping_add(1);

        let matches = slots_matching_state(state);
        if matches.len() == 1 {
            return Ok(matches[0]);
        }
        if matches.len() > 1 {
            bail!(
                "multiple XInput slots matched synthetic state: {:?}",
                matches
            );
        }

        if started.elapsed() >= SIGNAL_WAIT_TIMEOUT {
            bail!(
                "no XInput slot reported synthetic state within {} s; live slots: {}",
                SIGNAL_WAIT_TIMEOUT.as_secs(),
                slot_state_summary()
            );
        }
        tokio::time::sleep(SIGNAL_SEND_INTERVAL).await;
    }
}

async fn send_state_burst(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    state: protocol::GamepadState,
    flags: u8,
    duration: Duration,
) -> Result<()> {
    let started = Instant::now();
    let mut seq = 0u8;
    while started.elapsed() < duration {
        send_state_once(socket, target, seq, state, flags).await?;
        seq = seq.wrapping_add(1);
        tokio::time::sleep(SIGNAL_SEND_INTERVAL).await;
    }
    Ok(())
}

async fn send_state_once(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    seq: u8,
    state: protocol::GamepadState,
    flags: u8,
) -> Result<()> {
    let packet = protocol::Packet::state(seq, flags, state);
    socket
        .send_to(&packet.encode(), target)
        .await
        .with_context(|| format!("sending synthetic state to {target}"))?;
    Ok(())
}

async fn wait_for_slot_state(
    slot: u32,
    state: protocol::GamepadState,
    timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    loop {
        if let Some(snapshot) = xinput::read_slot(slot) {
            if snapshot.state == state {
                return Ok(());
            }
        }
        if started.elapsed() >= timeout {
            bail!(
                "{} did not return to neutral within {} s; live slots: {}",
                xinput::user_slot_label(slot),
                timeout.as_secs(),
                slot_state_summary()
            );
        }
        tokio::time::sleep(SIGNAL_SEND_INTERVAL).await;
    }
}

fn slots_matching_state(state: protocol::GamepadState) -> Vec<u32> {
    xinput::connected_slots()
        .into_iter()
        .filter(|slot| slot.state == state)
        .map(|slot| slot.slot)
        .collect()
}

fn slot_state_summary() -> String {
    let slots = xinput::connected_slots();
    if slots.is_empty() {
        return "none".to_string();
    }
    slots
        .iter()
        .map(|slot| {
            format!(
                "{} buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
                xinput::user_slot_label(slot.slot),
                slot.state.buttons,
                slot.state.left_trigger,
                slot.state.right_trigger,
                slot.state.left_x,
                slot.state.left_y,
                slot.state.right_x,
                slot.state.right_y
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn lab_signal_state(index: usize) -> protocol::GamepadState {
    let states = [
        protocol::GamepadState {
            buttons: 0x1100,
            left_trigger: 77,
            right_trigger: 133,
            left_x: 12000,
            left_y: -8000,
            right_x: 6000,
            right_y: -4000,
        },
        protocol::GamepadState {
            buttons: 0x2200,
            left_trigger: 33,
            right_trigger: 201,
            left_x: -16000,
            left_y: 7000,
            right_x: -9000,
            right_y: 15000,
        },
        protocol::GamepadState {
            buttons: 0x0043,
            left_trigger: 149,
            right_trigger: 19,
            left_x: 21000,
            left_y: 9000,
            right_x: -12000,
            right_y: -16000,
        },
        protocol::GamepadState {
            buttons: 0x208C,
            left_trigger: 8,
            right_trigger: 240,
            left_x: -23000,
            left_y: -3000,
            right_x: 13000,
            right_y: 4000,
        },
    ];
    states[index % states.len()]
}

async fn wait_for_setup_uids(
    uids: &BTreeSet<u32>,
    timeout: Duration,
) -> Result<Vec<SetupLabProbe>> {
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        let probes = probe_setup_ports().await?;
        let seen: BTreeSet<u32> = probes.iter().map(|p| p.uid).collect();
        if uids.is_subset(&seen) {
            return Ok(probes
                .into_iter()
                .filter(|p| uids.contains(&p.uid))
                .collect());
        }
        if Instant::now() >= deadline {
            let missing: Vec<String> = uids
                .difference(&seen)
                .map(|uid| format!("{uid:08X}"))
                .collect();
            bail!(
                "Pico(s) did not return to setup USB within {} s: {}",
                timeout.as_secs(),
                missing.join(", ")
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_setup_uids_absent(uids: &BTreeSet<u32>, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        let probes = probe_setup_ports().await?;
        let seen: BTreeSet<u32> = probes.iter().map(|p| p.uid).collect();
        if uids.is_disjoint(&seen) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let still_seen: Vec<String> = uids
                .intersection(&seen)
                .map(|uid| format!("{uid:08X}"))
                .collect();
            bail!(
                "external power off did not remove setup USB device(s): {}",
                still_seen.join(", ")
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_wifi_uid(uid: u32, timeout: Duration) -> Result<cmd_run::PicoTarget> {
    let started = Instant::now();
    let deadline = started + timeout;
    tokio::time::sleep(Duration::from_secs(2)).await;
    loop {
        let picos = cmd_run::discover_picos(Duration::from_secs(2)).await?;
        if let Some(pico) = picos.into_iter().find(|p| p.info.unique_id_short == uid) {
            return Ok(pico);
        }
        if Instant::now() >= deadline {
            bail!(
                "Pico {uid:08X} did not answer on Wi-Fi within {} s",
                timeout.as_secs()
            );
        }
    }
}

async fn wait_for_new_bootsel_mount(
    before: &BTreeSet<String>,
    timeout: Duration,
) -> Result<(PathBuf, cmd_flash::BootselBoard)> {
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        let mounts = cmd_flash::visible_bootsel_mounts();
        if let Some((mount, board)) = mounts
            .into_iter()
            .find(|(path, _)| !before.contains(&path.display().to_string()))
        {
            return Ok((mount, board));
        }
        if Instant::now() >= deadline {
            bail!(
                "no new BOOTSEL drive appeared within {} s",
                timeout.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn bootsel_keys() -> BTreeSet<String> {
    cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(path, _)| path.display().to_string())
        .collect()
}

fn dedup_setup_probes(probes: Vec<SetupLabProbe>) -> Vec<SetupLabProbe> {
    let mut by_uid = BTreeMap::new();
    for probe in probes {
        by_uid.insert(probe.uid, probe);
    }
    by_uid.into_values().collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PicoSelector {
    Uid(u32),
    Ip(IpAddr),
    Board(u8),
}

fn parse_selectors(values: &[String]) -> Result<Vec<PicoSelector>> {
    values.iter().map(|s| parse_selector(s)).collect()
}

fn parse_selector(value: &str) -> Result<PicoSelector> {
    let trimmed = value.trim();
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Ok(PicoSelector::Ip(ip));
    }

    let normalized = trimmed
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .replace(['-', '_', ' '], "")
        .to_ascii_lowercase();
    match normalized.as_str() {
        "pico2w" | "pico2" | "rp2350" => return Ok(PicoSelector::Board(protocol::BOARD_PICO_2_W)),
        "picow" | "picowh" | "pico" | "rp2040" => {
            return Ok(PicoSelector::Board(protocol::BOARD_PICO_W_RP2040));
        }
        _ => {}
    }

    if normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        let uid = u32::from_str_radix(&normalized, 16)
            .with_context(|| format!("invalid Pico UID selector `{value}`"))?;
        return Ok(PicoSelector::Uid(uid));
    }

    bail!("invalid Pico selector `{value}`; use UID, IP, pico2w, picow, rp2350, or rp2040")
}

fn setup_probe_selected(selectors: &[PicoSelector], probe: &SetupLabProbe) -> bool {
    selectors.is_empty()
        || selectors.iter().any(|selector| match selector {
            PicoSelector::Uid(uid) => *uid == probe.uid,
            PicoSelector::Board(board) => *board == probe.hello.board_type,
            PicoSelector::Ip(_) => false,
        })
}

fn wifi_target_selected(selectors: &[PicoSelector], target: &cmd_run::PicoTarget) -> bool {
    selectors.is_empty()
        || selectors.iter().any(|selector| match selector {
            PicoSelector::Uid(uid) => *uid == target.info.unique_id_short,
            PicoSelector::Board(board) => *board == target.info.board_type,
            PicoSelector::Ip(ip) => *ip == target.peer.ip(),
        })
}

fn selected_bootsel_mounts(
    selectors: &[PicoSelector],
    mounts: Vec<(PathBuf, cmd_flash::BootselBoard)>,
) -> Vec<(PathBuf, cmd_flash::BootselBoard)> {
    mounts
        .into_iter()
        .filter(|(_, board)| {
            selectors.is_empty()
                || selectors.iter().any(|selector| match selector {
                    PicoSelector::Board(board_type) => {
                        board_type_matches_bootsel(*board_type, *board)
                    }
                    PicoSelector::Uid(_) | PicoSelector::Ip(_) => false,
                })
        })
        .collect()
}

fn board_type_matches_bootsel(board_type: u8, bootsel: cmd_flash::BootselBoard) -> bool {
    matches!(
        (board_type, bootsel),
        (protocol::BOARD_PICO_2_W, cmd_flash::BootselBoard::Rp2350)
            | (
                protocol::BOARD_PICO_W_RP2040,
                cmd_flash::BootselBoard::Rp2040
            )
    )
}

fn board_type_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W",
        _ => "Pico",
    }
}

#[derive(Clone, Debug)]
struct SelectedPower {
    kind: SelectedPowerKind,
    label: &'static str,
}

impl SelectedPower {
    fn name(&self) -> &'static str {
        self.label
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedPowerKind {
    Reset,
    External,
    PnpRestart,
}

fn select_power_backend(
    requested: LabPower,
    cfg: Option<&config::LabPowerConfig>,
    report: &mut LabReport,
) -> SelectedPower {
    match requested {
        LabPower::Reset => SelectedPower {
            kind: SelectedPowerKind::Reset,
            label: "reset",
        },
        LabPower::PnpRestart => SelectedPower {
            kind: SelectedPowerKind::PnpRestart,
            label: "pnp-restart",
        },
        LabPower::External => {
            if external_power_configured(cfg) {
                SelectedPower {
                    kind: SelectedPowerKind::External,
                    label: "external",
                }
            } else {
                report.fail(
                    "select power backend",
                    None,
                    "external power requested, but [lab_power] off/on commands are not configured",
                    0,
                );
                SelectedPower {
                    kind: SelectedPowerKind::Reset,
                    label: "reset",
                }
            }
        }
        LabPower::Auto => {
            if external_power_probe_ok(cfg) {
                report.pass(
                    "select power backend",
                    None,
                    "external power probe passed",
                    0,
                );
                SelectedPower {
                    kind: SelectedPowerKind::External,
                    label: "external",
                }
            } else {
                report.pass(
                    "select power backend",
                    None,
                    "using firmware reset re-enumeration; no proven external power backend",
                    0,
                );
                SelectedPower {
                    kind: SelectedPowerKind::Reset,
                    label: "reset",
                }
            }
        }
    }
}

fn external_power_probe_ok(cfg: Option<&config::LabPowerConfig>) -> bool {
    let Some(cfg) = cfg else {
        return false;
    };
    if !external_power_configured(Some(cfg)) {
        return false;
    }
    match cfg.probe.as_ref() {
        Some(probe) => run_configured_command(probe).is_ok(),
        None => false,
    }
}

fn external_power_configured(cfg: Option<&config::LabPowerConfig>) -> bool {
    cfg.map(|cfg| !cfg.off.is_empty() && !cfg.on.is_empty())
        .unwrap_or(false)
}

fn run_configured_command(command: &[String]) -> Result<()> {
    if command.is_empty() {
        bail!("lab power command is empty");
    }
    let status = Command::new(&command[0])
        .args(&command[1..])
        .status()
        .with_context(|| format!("starting {}", command_label(command)))?;
    if !status.success() {
        bail!("{} exited with {}", command_label(command), status);
    }
    Ok(())
}

fn command_label(command: &[String]) -> String {
    command.join(" ")
}

fn restart_setup_pnp_devices() -> Result<()> {
    let status = Command::new("pnputil")
        .args(["/restart-device", "/deviceid", r"USB\VID_2E8A&PID_CAF0"])
        .status()
        .context("starting pnputil")?;
    if !status.success() {
        bail!(
            "pnputil /restart-device failed with {status}. Run as administrator or use --power reset."
        );
    }
    Ok(())
}

fn problem_usb_devices() -> Vec<String> {
    #[cfg(windows)]
    {
        let output = Command::new("pnputil")
            .args(["/enum-devices", "/connected", "/problem", "/ids"])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .filter(|line| {
                line.contains("VID_0000")
                    || line.contains("VID_2E8A")
                    || line.contains("VID_045E&PID_028E")
            })
            .map(|line| line.trim().to_string())
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[derive(Clone, Debug, Serialize)]
struct LabReport {
    started_utc: String,
    scenario: LabScenario,
    all: bool,
    cycles: u32,
    power_requested: LabPower,
    power_selected: String,
    no_flash: bool,
    steps: Vec<LabStep>,
    devices: Vec<LabDevice>,
}

impl LabReport {
    fn new(options: &LabOptions) -> Self {
        Self {
            started_utc: chrono::Utc::now().to_rfc3339(),
            scenario: options.scenario,
            all: options.all,
            cycles: options.cycles,
            power_requested: options.power,
            power_selected: "unknown".to_string(),
            no_flash: options.no_flash,
            steps: Vec::new(),
            devices: Vec::new(),
        }
    }

    fn pass(
        &mut self,
        name: impl Into<String>,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.push_step(name, StepStatus::Pass, uid, detail, elapsed_ms);
    }

    fn fail(
        &mut self,
        name: impl Into<String>,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.push_step(name, StepStatus::Fail, uid, detail, elapsed_ms);
    }

    fn skip(
        &mut self,
        name: impl Into<String>,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.push_step(name, StepStatus::Skip, uid, detail, elapsed_ms);
    }

    fn push_step(
        &mut self,
        name: impl Into<String>,
        status: StepStatus,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.steps.push(LabStep {
            name: name.into(),
            status,
            uid: uid.map(|uid| format!("{uid:08X}")),
            detail: detail.into(),
            elapsed_ms,
        });
    }

    fn fail_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == StepStatus::Fail)
            .count()
    }

    fn print_summary(&self) {
        println!();
        println!("Lab summary");
        for step in &self.steps {
            let uid = step
                .uid
                .as_ref()
                .map(|u| format!(" {u}"))
                .unwrap_or_default();
            println!(
                "  {:<4} {:<24}{}  {}",
                step.status.as_str(),
                step.name,
                uid,
                step.detail
            );
        }
        let pass = self
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Pass)
            .count();
        let skip = self
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Skip)
            .count();
        println!(
            "summary: {} pass, {} fail, {} skip",
            pass,
            self.fail_count(),
            skip
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum StepStatus {
    Pass,
    Fail,
    Skip,
}

impl StepStatus {
    fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Pass => "PASS",
            StepStatus::Fail => "FAIL",
            StepStatus::Skip => "SKIP",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct LabStep {
    name: String,
    status: StepStatus,
    uid: Option<String>,
    detail: String,
    elapsed_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
struct LabDevice {
    mode: String,
    uid: Option<String>,
    board: Option<String>,
    address: Option<String>,
    detail: Option<String>,
}

impl LabDevice {
    fn from_setup_probe(probe: &SetupLabProbe) -> Self {
        Self {
            mode: "setup-usb".to_string(),
            uid: Some(probe.uid_hex()),
            board: Some(probe.board_label().to_string()),
            address: Some(probe.port.clone()),
            detail: Some(format!(
                "fw v{} creds={} self_test={} log_bytes={} lost={}",
                probe.hello.firmware_version(),
                if probe.hello.creds_present() {
                    "present"
                } else {
                    "absent"
                },
                if probe.self_test.passed {
                    "pass"
                } else {
                    "fail"
                },
                probe.log_bytes,
                probe.log_lost
            )),
        }
    }

    fn from_wifi_target(target: &cmd_run::PicoTarget) -> Self {
        Self {
            mode: "wifi-run".to_string(),
            uid: Some(format!("{:08X}", target.info.unique_id_short)),
            board: Some(target.board_label().to_string()),
            address: Some(target.peer.to_string()),
            detail: Some(format!(
                "fw v{} uptime={}s",
                target.info.firmware_version(),
                target.info.uptime_seconds
            )),
        }
    }

    fn from_bootsel((mount, board): &(PathBuf, cmd_flash::BootselBoard)) -> Self {
        Self {
            mode: "bootsel".to_string(),
            uid: None,
            board: Some(board.label().to_string()),
            address: Some(mount.display().to_string()),
            detail: Some("ROM bootloader does not expose CouchLink UID".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selectors_accepts_uid_ip_and_board_names() {
        assert_eq!(
            parse_selector("07D37EB6").unwrap(),
            PicoSelector::Uid(0x07D37EB6)
        );
        assert!(matches!(
            parse_selector("192.168.50.4").unwrap(),
            PicoSelector::Ip(_)
        ));
        assert_eq!(
            parse_selector("pico2w").unwrap(),
            PicoSelector::Board(protocol::BOARD_PICO_2_W)
        );
        assert_eq!(
            parse_selector("rp2040").unwrap(),
            PicoSelector::Board(protocol::BOARD_PICO_W_RP2040)
        );
        assert!(parse_selector("not a pico").is_err());
    }

    #[test]
    fn report_counts_step_statuses() {
        let opts = LabOptions {
            all: true,
            picos: Vec::new(),
            scenario: LabScenario::Full,
            cycles: 1,
            power: LabPower::Auto,
            uf2: None,
            json: None,
            no_flash: false,
        };
        let mut report = LabReport::new(&opts);
        report.pass("a", None, "ok", 1);
        report.fail("b", Some(0x1234ABCD), "bad", 2);
        report.skip("c", None, "skip", 3);
        assert_eq!(report.fail_count(), 1);
        assert_eq!(report.steps[1].uid.as_deref(), Some("1234ABCD"));
    }

    #[test]
    fn power_backend_auto_prefers_reset_without_external_config() {
        let opts = LabOptions {
            all: true,
            picos: Vec::new(),
            scenario: LabScenario::PowerCycle,
            cycles: 1,
            power: LabPower::Auto,
            uf2: None,
            json: None,
            no_flash: false,
        };
        let mut report = LabReport::new(&opts);
        let selected = select_power_backend(LabPower::Auto, None, &mut report);
        assert_eq!(selected.kind, SelectedPowerKind::Reset);
        assert_eq!(selected.name(), "reset");
    }

    #[test]
    fn lab_signal_states_are_distinct() {
        let states: Vec<_> = (0..4).map(lab_signal_state).collect();
        for (idx, state) in states.iter().enumerate() {
            assert_ne!(*state, protocol::GamepadState::default());
            assert!(!states[..idx].contains(state));
        }
    }

    #[test]
    fn command_label_preserves_argv_order() {
        let cmd = vec![
            "hubctl.exe".to_string(),
            "--port".to_string(),
            "2".to_string(),
            "off".to_string(),
        ];
        assert_eq!(command_label(&cmd), "hubctl.exe --port 2 off");
    }
}
