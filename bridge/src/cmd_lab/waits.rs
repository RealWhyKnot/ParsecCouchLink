use super::setup_probe::{SetupLabProbe, probe_known_ip_for_uid, probe_setup_ports};
use crate::{cmd_flash, cmd_run, cmd_usb_diag, protocol};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) async fn wait_for_setup_uids(
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

pub(super) async fn wait_for_setup_uids_absent(
    uids: &BTreeSet<u32>,
    timeout: Duration,
) -> Result<()> {
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

pub(super) async fn wait_for_wifi_uid(uid: u32, timeout: Duration) -> Result<cmd_run::PicoTarget> {
    wait_for_wifi_uid_at(uid, None, timeout).await
}

pub(super) async fn wait_for_wifi_uid_at(
    uid: u32,
    last_ip: Option<IpAddr>,
    timeout: Duration,
) -> Result<cmd_run::PicoTarget> {
    let started = Instant::now();
    let deadline = started + timeout;
    tokio::time::sleep(Duration::from_secs(2)).await;
    loop {
        let now = Instant::now();
        let remaining = deadline.saturating_duration_since(now);
        let discover_for = remaining.min(Duration::from_secs(2));
        let picos = cmd_run::discover_picos(discover_for).await?;
        if let Some(pico) = picos.into_iter().find(|p| p.info.unique_id_short == uid) {
            return Ok(pico);
        }

        if let Some(ip) = last_ip
            && let Some(target) = probe_known_ip_for_uid(ip, uid, discover_for).await?
        {
            return Ok(target);
        }

        if Instant::now() >= deadline {
            bail!(
                "Pico {uid:08X} did not answer on Wi-Fi within {} s",
                timeout.as_secs()
            );
        }
    }
}

pub(super) async fn wait_for_usb_controller_ready(
    target: &cmd_run::PicoTarget,
    timeout: Duration,
) -> Result<protocol::UsbDiag> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut last_diag: Option<protocol::UsbDiag> = None;
    loop {
        let now = Instant::now();
        if now >= deadline {
            if let Some(diag) = last_diag {
                bail!(
                    "{} did not become a ready USB controller within {} s (mounted={} reports_sent={})",
                    target.peer,
                    timeout.as_secs(),
                    diag.mounted(),
                    diag.xinput_report_sent()
                );
            }
            bail!(
                "{} did not answer USB diagnostics within {} s",
                target.peer,
                timeout.as_secs()
            );
        }

        let remaining = deadline.saturating_duration_since(now);
        match cmd_usb_diag::query_usb_diag(target, remaining.min(Duration::from_secs(3))).await {
            Ok(diag) if diag.mounted() && diag.xinput_report_sent() => return Ok(diag),
            Ok(diag) => last_diag = Some(diag),
            Err(e) => tracing::debug!(
                "lab: USB readiness probe for {:08X} failed: {e:#}",
                target.info.unique_id_short
            ),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(super) async fn wait_for_new_bootsel_mount(
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

pub(super) fn bootsel_keys() -> BTreeSet<String> {
    cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(path, _)| path.display().to_string())
        .collect()
}

pub(super) fn dedup_setup_probes(probes: Vec<SetupLabProbe>) -> Vec<SetupLabProbe> {
    let mut by_uid = BTreeMap::new();
    for probe in probes {
        by_uid.insert(probe.uid, probe);
    }
    by_uid.into_values().collect()
}
