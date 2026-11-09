use super::pnp::{
    PnpPersona, pnp_instance_ids, pnp_instances_for_uids, pnp_remove_rescan_with_elevation,
};
use super::power_cycles::SelectedPowerKind;
use super::{
    BOOTSEL_WAIT_TIMEOUT, LabHarness, SETUP_WAIT_TIMEOUT, SetupLabProbe, SetupOrWifiMode,
    USB_READY_WAIT_TIMEOUT, WIFI_WAIT_TIMEOUT, bootsel_keys, dedup_setup_probes, ensure_wifi_ready,
    lab_signal_state, neutralize_targets, reboot_setup_to_bootsel, reboot_setup_to_run,
    verify_signal_roundtrip, wait_for_new_bootsel_mount, wait_for_setup_or_wifi_uid,
    wait_for_setup_uid, wait_for_usb_controller_ready, wait_for_wifi_uid, wait_for_wifi_uid_at,
};
use crate::{cmd_flash, cmd_run, pico_mode, xinput};
use anyhow::bail;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::time::Instant;

impl LabHarness {
    pub(super) async fn run_flash_cycles(
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
            outln!("Flash cycle for {} on {}...", probe.uid_hex(), probe.port);
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

        if let Err(e) = cmd_flash::flash_uf2_to_mount(&uf2, mount, board).await {
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

    pub(super) async fn run_final_run_checks(&mut self, probes: Vec<SetupLabProbe>) {
        let mut run_targets = Vec::new();
        for probe in probes {
            let uid = probe.uid;
            let started = Instant::now();
            outln!("Returning {} to run mode...", probe.uid_hex());
            let result = async {
                ensure_wifi_ready(&probe).await?;
                reboot_setup_to_run(probe.port.clone()).await?;
                let target = wait_for_wifi_uid(uid, WIFI_WAIT_TIMEOUT).await?;
                let diag = wait_for_usb_controller_ready(&target, USB_READY_WAIT_TIMEOUT).await?;
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
        if self.selected_power.kind == SelectedPowerKind::PnpRemove {
            let updated = self.run_run_mode_pnp_remove_cycle(run_targets).await;
            self.run_signal_checks(&updated).await;
        }
    }

    async fn run_run_mode_pnp_remove_cycle(
        &mut self,
        targets: Vec<cmd_run::PicoTarget>,
    ) -> Vec<cmd_run::PicoTarget> {
        let uids: BTreeSet<u32> = targets.iter().map(|p| p.info.unique_id_short).collect();
        let last_ips: BTreeMap<u32, IpAddr> = targets
            .iter()
            .map(|p| (p.info.unique_id_short, p.peer.ip()))
            .collect();
        if uids.is_empty() {
            return targets;
        }

        let started = Instant::now();
        let result = async {
            let instances = pnp_instances_for_uids(&uids, &[PnpPersona::Xinput])?;
            if instances.len() != uids.len() {
                bail!(
                    "found {} XInput instance(s) for {} selected Pico(s)",
                    instances.len(),
                    uids.len()
                );
            }
            pnp_remove_rescan_with_elevation(&pnp_instance_ids(&instances))?;
            let mut wifi_after_remove = Vec::new();
            for uid in &uids {
                let target =
                    wait_for_wifi_uid_at(*uid, last_ips.get(uid).copied(), WIFI_WAIT_TIMEOUT)
                        .await?;
                wifi_after_remove.push(target);
            }
            let mut refreshed = Vec::new();
            for target in wifi_after_remove {
                let uid = target.info.unique_id_short;
                pico_mode::request_reboot_to_setup(&target).await?;
                let probe = wait_for_setup_uid(uid, SETUP_WAIT_TIMEOUT).await?;
                reboot_setup_to_run(probe.port).await?;
                let target =
                    wait_for_wifi_uid_at(uid, Some(target.peer.ip()), WIFI_WAIT_TIMEOUT).await?;
                wait_for_usb_controller_ready(&target, USB_READY_WAIT_TIMEOUT).await?;
                refreshed.push(target);
            }
            let present = pnp_instances_for_uids(&uids, &[PnpPersona::Xinput])?;
            if present.len() != uids.len() {
                bail!(
                    "after reconnect, found {} XInput instance(s) for {} selected Pico(s)",
                    present.len(),
                    uids.len()
                );
            }
            Ok::<_, anyhow::Error>(refreshed)
        }
        .await;

        match result {
            Ok(updated) => {
                self.report.pass(
                    "run reconnect cycle",
                    None,
                    "PNP remove/rescan plus firmware USB re-enumeration completed for XInput controller devices; USB power was not cut",
                    started.elapsed().as_millis(),
                );
                updated
            }
            Err(e) => {
                self.report.fail(
                    "run reconnect cycle",
                    None,
                    format!("{e:#}"),
                    started.elapsed().as_millis(),
                );
                targets
            }
        }
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
