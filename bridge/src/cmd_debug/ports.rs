use super::{board_type_label, creds_label};
use crate::{cdc, cmd_flash, cmd_run};
use anyhow::{Result, bail};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) async fn probe_setup_ports(ports: Vec<String>) -> Vec<SetupProbe> {
    let mut probes = Vec::new();
    for port in ports {
        let port_for_task = port.clone();
        let probe =
            tokio::task::spawn_blocking(move || -> Result<(cdc::HelloAck, cdc::SelfTestAck)> {
                let mut pico = cdc::PicoSetup::open_named(&port_for_task)?;
                let hello = pico.hello()?;
                let self_test = pico.self_test()?;
                Ok((hello, self_test))
            })
            .await;
        match probe {
            Ok(Ok((hello, self_test))) => probes.push(SetupProbe {
                port,
                hello: Some(hello),
                self_test: Some(self_test),
                error: None,
            }),
            Ok(Err(e)) => probes.push(SetupProbe {
                port,
                hello: None,
                self_test: None,
                error: Some(format!("{e:#}")),
            }),
            Err(e) => probes.push(SetupProbe {
                port,
                hello: None,
                self_test: None,
                error: Some(format!("probe task failed: {e}")),
            }),
        }
    }
    probes
}

pub(super) async fn reboot_setup_port_to_wifi(port: String) -> Result<cdc::HelloAck> {
    tokio::task::spawn_blocking(move || -> Result<cdc::HelloAck> {
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
        if !hello.creds_present() {
            bail!("Pico has no saved Wi-Fi credentials. Choose `Set up or change Wi-Fi` first.");
        }
        pico.reboot_to_run()?;
        Ok(hello)
    })
    .await?
}

pub(super) async fn reboot_setup_port_to_bootsel(port: String) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }
        outln!(
            "  {port} fw v{} {} -> BOOTSEL",
            hello.firmware_version(),
            board_type_label(hello.board_type)
        );
        pico.reboot_to_bootsel()?;
        Ok(())
    })
    .await?
}

pub(super) async fn read_setup_log(port: String) -> Result<(String, u32)> {
    tokio::task::spawn_blocking(move || -> Result<(String, u32)> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        pico.get_log_buffer()
    })
    .await?
}

pub(super) fn setup_port_set() -> Result<BTreeSet<String>> {
    Ok(cdc::find_setup_ports()?.into_iter().collect())
}

pub(super) fn bootsel_mount_set() -> BTreeSet<String> {
    cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(path, _board)| path.display().to_string())
        .collect()
}

pub(super) async fn wait_for_setup_ports_after(
    before: &BTreeSet<String>,
    timeout: Duration,
) -> Result<Vec<String>> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    loop {
        let ports = cdc::find_setup_ports()?;
        let current: BTreeSet<String> = ports.iter().cloned().collect();
        if !ports.is_empty() && &current != before {
            return Ok(ports);
        }
        if before.is_empty() && !ports.is_empty() {
            return Ok(ports);
        }
        let now = Instant::now();
        if now >= deadline {
            bail!(
                "the Pico did not reappear in USB debug mode within {} s",
                timeout.as_secs()
            );
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            outln!(
                "  ... still waiting for USB debug mode ({elapsed}/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(super) async fn wait_for_bootsel_mounts_after(
    before: &BTreeSet<String>,
    timeout: Duration,
) -> Result<Vec<(PathBuf, cmd_flash::BootselBoard)>> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    loop {
        let mounts = cmd_flash::visible_bootsel_mounts();
        let new_mounts: Vec<(PathBuf, cmd_flash::BootselBoard)> = mounts
            .iter()
            .filter(|(path, _board)| !before.contains(&path.display().to_string()))
            .cloned()
            .collect();
        if !new_mounts.is_empty() {
            return Ok(new_mounts);
        }
        if before.is_empty() && !mounts.is_empty() {
            return Ok(mounts);
        }
        let now = Instant::now();
        if now >= deadline {
            bail!(
                "the Pico did not reappear as a BOOTSEL drive within {} s",
                timeout.as_secs()
            );
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            outln!(
                "  ... still waiting for BOOTSEL ({elapsed}/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub(super) enum WifiWaitResult {
    Found(Vec<cmd_run::PicoTarget>),
    TimedOut,
}

pub(super) async fn wifi_uid_set() -> Result<BTreeSet<u32>> {
    Ok(cmd_run::discover_picos(Duration::from_secs(2))
        .await?
        .into_iter()
        .map(|pico| pico.info.unique_id_short)
        .collect())
}

pub(super) async fn wait_for_wifi_picos_after(
    before: &BTreeSet<u32>,
    timeout: Duration,
) -> Result<WifiWaitResult> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    tokio::time::sleep(Duration::from_secs(3)).await;
    loop {
        let picos = cmd_run::discover_picos(Duration::from_secs(2)).await?;
        let current: BTreeSet<u32> = picos.iter().map(|pico| pico.info.unique_id_short).collect();
        if !picos.is_empty() && &current != before {
            return Ok(WifiWaitResult::Found(picos));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(WifiWaitResult::TimedOut);
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            outln!(
                "  ... still waiting for Wi-Fi reply ({elapsed}/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
    }
}

pub(super) struct SetupProbe {
    port: String,
    hello: Option<cdc::HelloAck>,
    self_test: Option<cdc::SelfTestAck>,
    error: Option<String>,
}

impl SetupProbe {
    pub(super) fn status_line(&self) -> String {
        match (self.hello, &self.self_test, &self.error) {
            (Some(hello), Some(self_test), _) => format!(
                "{}  fw v{}  {}  {}  SELF_TEST {} {}",
                self.port,
                hello.firmware_version(),
                board_type_label(hello.board_type),
                creds_label(hello),
                if self_test.passed { "PASS" } else { "FAIL" },
                self_test.message
            ),
            (_, _, Some(error)) => format!("{}  visible, but HELLO failed: {error}", self.port),
            _ => format!("{}  visible, but probe did not finish", self.port),
        }
    }
}
