use super::{PicoTarget, discover_picos, discover_picos_with_auto_recovery};
use crate::protocol::{self};
use crate::{cdc, discovery, net, pico_cache, support};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub(crate) async fn run_recover_command() -> Result<()> {
    outln!("Recovering Pico boards for streaming...");
    let picos = discover_picos_with_auto_recovery(Duration::from_secs(5), false).await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(5));
    }
    outln!("Ready for streaming:");
    for pico in picos {
        outln!("  {}", pico.detail_label());
        outln!("    manual IP: {}", pico.peer.ip());
    }
    Ok(())
}

pub(crate) async fn probe_pico_ip(ip: IpAddr, timeout: Duration) -> Result<PicoTarget> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP manual-IP probe socket")?;
    let Some(reply) = discovery::probe_ip(&socket, ip, timeout)
        .await
        .with_context(|| format!("probing Pico at {ip}:{}", protocol::PORT))?
    else {
        bail!(
            "no Pico replied at {ip}:{} within {} s",
            protocol::PORT,
            timeout.as_secs()
        );
    };
    if !protocol::proto_supported(reply.info.proto_version) {
        bail!(
            "Pico at {} speaks protocol v{}, bridge speaks v{}. Update whichever side is older.",
            reply.peer,
            reply.info.proto_version,
            protocol::PROTO_VERSION,
        );
    }
    let target = PicoTarget {
        peer: reply.peer,
        info: reply.info,
        persona: reply.persona,
        ack_flags: reply.flags,
    };
    pico_cache::record_target("probe-ip", &target);
    Ok(target)
}

pub(super) async fn recover_setup_usb_to_wifi(quiet: bool) -> Result<usize> {
    let ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        return Ok(0);
    }

    let mut rebooted = 0usize;
    let mut blocked = 0usize;
    let mut printed_header = false;

    for port in ports {
        match setup_port_reboot_to_run(port.clone()).await {
            Ok(SetupRecovery::Rebooted { firmware, board }) => {
                rebooted += 1;
                if !quiet {
                    print_setup_recovery_header(&mut printed_header);
                    outln!("  {port}: fw v{firmware} {board} -> Wi-Fi/input mode");
                }
            }
            Ok(SetupRecovery::NoCredentials { firmware, board }) => {
                blocked += 1;
                if !quiet {
                    print_setup_recovery_header(&mut printed_header);
                    outln!(
                        "  {port}: fw v{firmware} {board} has no saved Wi-Fi; choose `Set up or change Wi-Fi`."
                    );
                }
            }
            Ok(SetupRecovery::AlreadyRunMode { firmware, board }) => {
                tracing::debug!(
                    "run: {port} fw v{firmware} {board} is already in run mode; skipping USB recovery"
                );
            }
            Err(e) => {
                blocked += 1;
                if !quiet {
                    print_setup_recovery_header(&mut printed_header);
                    outln!("  {port}: could not auto-recover: {e:#}");
                }
            }
        }
    }

    if rebooted == 0 && blocked > 0 && !quiet {
        outln!("Auto-recovery could not switch any setup-mode Pico back to Wi-Fi.");
    }

    Ok(rebooted)
}

pub(super) fn print_setup_recovery_header(printed: &mut bool) {
    if !*printed {
        outln!("Found recoverable setup-mode USB Pico port(s):");
        *printed = true;
    }
}

pub(super) enum SetupRecovery {
    Rebooted {
        firmware: String,
        board: &'static str,
    },
    AlreadyRunMode {
        firmware: String,
        board: &'static str,
    },
    NoCredentials {
        firmware: String,
        board: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SetupUsbMode {
    RunModeActive,
    SetupModeWithCredentials,
    SetupModeWithoutCredentials,
}

pub(super) fn classify_setup_usb_hello(hello: &cdc::HelloAck) -> SetupUsbMode {
    if hello.run_mode_active() {
        SetupUsbMode::RunModeActive
    } else if hello.creds_present() {
        SetupUsbMode::SetupModeWithCredentials
    } else {
        SetupUsbMode::SetupModeWithoutCredentials
    }
}

pub(super) async fn setup_port_reboot_to_run(port: String) -> Result<SetupRecovery> {
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

        let firmware = hello.firmware_version().to_string();
        let board = setup_board_label(hello.board_type);
        if classify_setup_usb_hello(&hello) == SetupUsbMode::RunModeActive {
            return Ok(SetupRecovery::AlreadyRunMode { firmware, board });
        }

        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }

        if classify_setup_usb_hello(&hello) == SetupUsbMode::SetupModeWithoutCredentials {
            return Ok(SetupRecovery::NoCredentials { firmware, board });
        }

        pico.reboot_to_run()?;
        Ok(SetupRecovery::Rebooted { firmware, board })
    })
    .await?
}

pub(super) async fn wait_for_wifi_after_recovery(
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
                outln!(
                    "  ... still waiting for Wi-Fi reply ({elapsed}/{})",
                    timeout.as_secs()
                );
            }
            next_beat = now + Duration::from_secs(10);
        }
    }
}

pub(super) fn merge_unique_picos(picos: &mut Vec<PicoTarget>, incoming: Vec<PicoTarget>) {
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

pub(super) fn recovered_target_count(picos: &[PicoTarget], baseline_ids: &HashSet<u32>) -> usize {
    picos
        .iter()
        .filter(|p| !baseline_ids.contains(&p.info.unique_id_short))
        .count()
}

pub(super) fn setup_board_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W / WH",
        _ => "Pico",
    }
}
