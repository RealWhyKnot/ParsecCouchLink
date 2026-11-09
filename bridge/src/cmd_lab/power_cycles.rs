use super::pnp::{
    PnpPersona, pnp_disable_enable_with_elevation, pnp_instance_ids, pnp_instances_for_uids,
    pnp_remove_rescan_with_elevation,
};
use super::report::LabReport;
use super::{
    LabHarness, LabPower, POWER_OFF_WAIT_TIMEOUT, SETUP_WAIT_TIMEOUT, SetupLabProbe,
    dedup_setup_probes, wait_for_setup_uids, wait_for_setup_uids_absent,
};
use crate::config;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::process::Command;
use std::time::Instant;

impl LabHarness {
    pub(super) async fn run_power_cycle(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        match self.selected_power.kind {
            SelectedPowerKind::External => self.run_external_power_cycle(probes, cycle).await,
            SelectedPowerKind::PnpRestart => self.run_pnp_restart_cycle(probes, cycle).await,
            SelectedPowerKind::PnpRemove => self.run_pnp_remove_cycle(probes, cycle).await,
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
            let instances = pnp_instances_for_uids(&uids, &[PnpPersona::Setup])?;
            if instances.len() != uids.len() {
                bail!(
                    "found {} setup USB instance(s) for {} selected Pico(s)",
                    instances.len(),
                    uids.len()
                );
            }
            pnp_disable_enable_with_elevation(&pnp_instance_ids(&instances))?;
            wait_for_setup_uids(&uids, SETUP_WAIT_TIMEOUT).await
        }
        .await;
        match result {
            Ok(updated) => {
                self.report.pass(
                    format!("power cycle {cycle}"),
                    None,
                    "PNP disable/enable completed for setup USB devices; USB power was not cut",
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

    async fn run_pnp_remove_cycle(
        &mut self,
        probes: Vec<SetupLabProbe>,
        cycle: u32,
    ) -> Vec<SetupLabProbe> {
        let uids: BTreeSet<u32> = probes.iter().map(|p| p.uid).collect();
        let started = Instant::now();
        let result = async {
            let instances = pnp_instances_for_uids(&uids, &[PnpPersona::Setup])?;
            if instances.len() != uids.len() {
                bail!(
                    "found {} setup USB instance(s) for {} selected Pico(s)",
                    instances.len(),
                    uids.len()
                );
            }
            pnp_remove_rescan_with_elevation(&pnp_instance_ids(&instances))?;
            wait_for_setup_uids(&uids, SETUP_WAIT_TIMEOUT).await
        }
        .await;
        match result {
            Ok(updated) => {
                self.report.pass(
                    format!("power cycle {cycle}"),
                    None,
                    "PNP remove/rescan completed for setup USB devices; USB power was not cut",
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
}

#[derive(Clone, Debug)]
pub(super) struct SelectedPower {
    pub(super) kind: SelectedPowerKind,
    label: &'static str,
}

impl SelectedPower {
    pub(super) fn name(&self) -> &'static str {
        self.label
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectedPowerKind {
    Reset,
    External,
    PnpRestart,
    PnpRemove,
}

pub(super) fn select_power_backend(
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
        LabPower::PnpRemove => SelectedPower {
            kind: SelectedPowerKind::PnpRemove,
            label: "pnp-remove",
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

pub(super) fn external_power_probe_ok(cfg: Option<&config::LabPowerConfig>) -> bool {
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

pub(super) fn external_power_configured(cfg: Option<&config::LabPowerConfig>) -> bool {
    cfg.is_some_and(|cfg| !cfg.off.is_empty() && !cfg.on.is_empty())
}

pub(super) fn run_configured_command(command: &[String]) -> Result<()> {
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

pub(super) fn command_label(command: &[String]) -> String {
    command.join(" ")
}
