mod flash_checks;
mod pnp;
mod power_cycles;
mod report;
mod selectors;
mod setup_probe;
mod signal;
mod waits;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;

use crate::{cmd_flash, cmd_run, config, pico_mode, xinput};

pub(crate) use pnp::{PnpHelperAction, run_pnp_helper};
use pnp::{PnpPersona, pnp_instance_ids, pnp_instances_for_uids, problem_usb_devices};
use power_cycles::{SelectedPower, select_power_backend};
use report::{LabDevice, LabReport};
use selectors::{
    PicoSelector, board_type_label, parse_selectors, selected_bootsel_mounts, setup_probe_selected,
    wifi_target_selected,
};
use setup_probe::{
    SetupLabProbe, SetupOrWifiMode, ensure_wifi_ready, probe_setup_ports, reboot_setup_to_bootsel,
    reboot_setup_to_run, wait_for_setup_or_wifi_uid, wait_for_setup_uid,
};
use signal::{lab_signal_state, neutralize_targets, verify_signal_roundtrip};
use waits::{
    bootsel_keys, dedup_setup_probes, wait_for_new_bootsel_mount, wait_for_setup_uids,
    wait_for_setup_uids_absent, wait_for_usb_controller_ready, wait_for_wifi_uid,
    wait_for_wifi_uid_at,
};

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(5);
const SETUP_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const WIFI_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const BOOTSEL_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const POWER_OFF_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const USB_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const SIGNAL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const SIGNAL_SEND_INTERVAL: Duration = Duration::from_millis(16);
const SIGNAL_NEUTRALIZE_DURATION: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LabScenario {
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
pub(crate) enum LabPower {
    Auto,
    Reset,
    External,
    PnpRestart,
    PnpRemove,
}

#[derive(Clone, Debug)]
pub(crate) struct LabOptions {
    pub all: bool,
    pub picos: Vec<String>,
    pub scenario: LabScenario,
    pub cycles: u32,
    pub power: LabPower,
    pub uf2: Option<PathBuf>,
    pub json: Option<PathBuf>,
    pub no_flash: bool,
}

pub(crate) async fn run(options: LabOptions) -> Result<()> {
    if options.cycles == 0 {
        bail!("--cycles must be at least 1");
    }

    let mut report = LabReport::new(&options);
    outln!("couchlink lab");
    outln!(
        "scenario={:?} cycles={} power={:?}",
        options.scenario,
        options.cycles,
        options.power
    );
    outln!();

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
        outln!("JSON report: {}", path.display());
    }

    if lab.report.fail_count() > 0 {
        bail!("lab failed with {} failed step(s)", lab.report.fail_count());
    }
    Ok(())
}

pub(crate) fn connected_xinput_instance_ids_for_uids(uids: &BTreeSet<u32>) -> Result<Vec<String>> {
    let instances = pnp_instances_for_uids(uids, &[PnpPersona::Xinput])?;
    Ok(pnp_instance_ids(&instances))
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
            outln!();
            outln!("Lab cycle {cycle}/{}", self.options.cycles);

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
                let outcome = cmd_flash::flash_uf2_to_mount(&uf2, mount, board).await?;
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
            outln!("Moving {} to setup USB...", target.short_label());
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
            outln!("Mode cycle for {} on {}...", probe.uid_hex(), probe.port);
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
}

#[cfg(test)]
mod tests;
