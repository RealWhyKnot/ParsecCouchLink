use super::board_type_label;
use crate::{cdc, cmd_run};
use anyhow::{Result, bail};
use std::net::IpAddr;
use std::time::{Duration, Instant};
use zeroize::Zeroize;

#[derive(Clone, Debug)]
pub(super) struct SetupLabProbe {
    pub(super) port: String,
    pub(super) hello: cdc::HelloAck,
    pub(super) self_test: cdc::SelfTestAck,
    pub(super) uid: u32,
    pub(super) last_ip: Option<IpAddr>,
    pub(super) log_bytes: usize,
    pub(super) log_lost: u32,
}

impl SetupLabProbe {
    pub(super) fn uid_hex(&self) -> String {
        format!("{:08X}", self.uid)
    }

    pub(super) fn board_label(&self) -> &'static str {
        board_type_label(self.hello.board_type)
    }
}

pub(super) async fn probe_setup_ports() -> Result<Vec<SetupLabProbe>> {
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

pub(super) async fn probe_setup_port(port: String) -> Result<SetupLabProbe> {
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

pub(super) async fn reboot_setup_to_run(port: String) -> Result<()> {
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

pub(super) async fn reboot_setup_to_bootsel(port: String) -> Result<()> {
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

pub(super) async fn ensure_wifi_ready(probe: &SetupLabProbe) -> Result<()> {
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

pub(super) async fn wait_for_setup_uid(uid: u32, timeout: Duration) -> Result<SetupLabProbe> {
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

pub(super) enum SetupOrWifiMode {
    Setup(SetupLabProbe),
    Wifi(cmd_run::PicoTarget),
}

pub(super) async fn wait_for_setup_or_wifi_uid(
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

        if let Some(ip) = last_ip
            && let Some(target) = probe_known_ip_for_uid(ip, uid, discover_for).await?
        {
            return Ok(SetupOrWifiMode::Wifi(target));
        }
    }
}

pub(super) async fn probe_known_ip_for_uid(
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
