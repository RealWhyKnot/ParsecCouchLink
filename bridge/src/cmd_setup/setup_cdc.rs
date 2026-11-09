use crate::{cdc, cmd_run, pico_mode};
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::time::Duration;

pub(super) async fn discover_run_mode_uid_baseline() -> Option<HashSet<u32>> {
    match cmd_run::discover_picos(Duration::from_secs(2)).await {
        Ok(picos) => {
            let ids: HashSet<u32> = picos.iter().map(|p| p.info.unique_id_short).collect();
            tracing::info!(
                "setup: pre-flash run-mode baseline contains {} Pico board(s)",
                ids.len()
            );
            Some(ids)
        }
        Err(e) => {
            tracing::debug!("setup: pre-flash run-mode baseline discovery failed: {e:#}");
            None
        }
    }
}

pub(super) async fn wait_for_setup_cdc(
    timeout: Duration,
    pre_flash_run_uids: Option<&HashSet<u32>>,
) -> Result<String> {
    let started = std::time::Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    let mut next_recovery_probe = started + Duration::from_secs(3);
    let mut recovery_requested = false;
    let mut logged_ambiguous_recovery = false;
    let total = timeout.as_secs();
    loop {
        if let Ok(name) = cdc::find_setup_port() {
            return Ok(name);
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            let (vid, pid) = (cdc::SETUP_VID, cdc::SETUP_PID);
            bail!(
                "Pico did not appear as a USB CDC serial device within {} s. \
                 Open Device Manager and check Ports (COM & LPT) for a new COM port, \
                 or Other devices for an entry with VID {vid:04X}:{pid:04X}. \
                 If nothing shows at all, the firmware likely did not boot -- try a \
                 different micro-USB data cable or USB port and re-run setup. \
                 If a COM port does appear but setup still fails, run \
                 `couchlink.exe bundle` and attach the resulting zip to a bug report.",
                timeout.as_secs(),
            );
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            outln!("  ... still waiting for USB enumeration ({elapsed}s/{total}s)");
            tracing::info!("setup: still waiting for setup-mode CDC port ({elapsed}s/{total}s)");
            next_beat = now + Duration::from_secs(10);
        }
        if !recovery_requested && now >= next_recovery_probe {
            let Some(pre_flash_run_uids) = pre_flash_run_uids else {
                next_recovery_probe = now + Duration::from_secs(10);
                continue;
            };
            match find_reboot_to_setup_candidate(pre_flash_run_uids).await {
                Ok(SetupRecoveryProbe::One(pico)) => {
                    outln!(
                        "  Pico came back in {} mode instead of USB serial; asking it to switch to setup USB...",
                        pico.persona.label(),
                    );
                    tracing::info!(
                        "setup: run-mode Pico {} came back as {}; requesting setup-mode reboot",
                        pico.short_label(),
                        pico.persona.label(),
                    );
                    pico_mode::request_reboot_to_setup(&pico).await?;
                    recovery_requested = true;
                    next_beat = now + Duration::from_secs(10);
                }
                Ok(SetupRecoveryProbe::Ambiguous { total, candidates }) => {
                    if !logged_ambiguous_recovery {
                        tracing::warn!(
                            "setup: found {total} run-mode Pico board(s), {candidates} not in pre-flash baseline; not auto-switching stage-4 recovery"
                        );
                        logged_ambiguous_recovery = true;
                    }
                    next_recovery_probe = now + Duration::from_secs(10);
                }
                Ok(SetupRecoveryProbe::None) => {
                    next_recovery_probe = now + Duration::from_secs(5);
                }
                Err(e) => {
                    tracing::debug!("setup: run-mode recovery discovery failed: {e:#}");
                    next_recovery_probe = now + Duration::from_secs(5);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(super) enum SetupRecoveryProbe {
    One(cmd_run::PicoTarget),
    Ambiguous { total: usize, candidates: usize },
    None,
}

pub(super) async fn find_reboot_to_setup_candidate(
    pre_flash_run_uids: &HashSet<u32>,
) -> Result<SetupRecoveryProbe> {
    let picos = cmd_run::discover_picos(Duration::from_secs(2)).await?;
    if picos.is_empty() {
        return Ok(SetupRecoveryProbe::None);
    }
    let total = picos.len();
    let candidates = setup_recovery_candidate_uids(&picos, pre_flash_run_uids);
    if candidates.is_empty() {
        return Ok(SetupRecoveryProbe::None);
    }
    if candidates.len() != 1 {
        return Ok(SetupRecoveryProbe::Ambiguous {
            total,
            candidates: candidates.len(),
        });
    }
    let target_uid = candidates[0];
    let pico = picos
        .into_iter()
        .find(|p| p.info.unique_id_short == target_uid)
        .expect("candidate UID came from picos");
    Ok(SetupRecoveryProbe::One(pico))
}

pub(super) fn setup_recovery_candidate_uids(
    picos: &[cmd_run::PicoTarget],
    pre_flash_run_uids: &HashSet<u32>,
) -> Vec<u32> {
    picos
        .iter()
        .filter(|p| !pre_flash_run_uids.contains(&p.info.unique_id_short))
        .map(|p| p.info.unique_id_short)
        .collect()
}
