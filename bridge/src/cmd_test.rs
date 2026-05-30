//! `couchlink test <which>` -- run a single check from the doctor ladder.

use std::time::Duration;

use anyhow::{anyhow, Result};
use tokio::net::UdpSocket;

use crate::cdc;
use crate::cmd_doctor::{
    check_24ghz_warning, check_cdc, check_discover, check_firewall, check_paths,
    check_startup_shortcut, check_xinput, CheckResult,
};
use crate::{discovery, support};

pub async fn run(which: &str, all: bool, reboot_to_run: bool) -> Result<()> {
    if reboot_to_run && which != "cdc" {
        return Err(anyhow!(
            "--reboot-to-run is only supported for `couchlink test cdc`"
        ));
    }
    if all {
        return match which {
            "cdc" => run_cdc_all(reboot_to_run).await,
            "discover" => run_discover_all().await,
            _ => Err(anyhow!(
                "--all is only supported for `couchlink test cdc` and `couchlink test discover`"
            )),
        };
    }
    if reboot_to_run {
        return run_cdc_one_reboot_to_run().await;
    }

    let result = match which {
        "paths" => check_paths().await,
        "xinput" => check_xinput().await,
        "startup" => check_startup_shortcut().await,
        "firewall" => check_firewall().await,
        "wifi-band" | "wifi" => check_24ghz_warning().await,
        "cdc" => check_cdc().await,
        "discover" => check_discover().await,
        other => {
            return Err(anyhow!(
                "unknown test: {other}. Available: paths, xinput, startup, firewall, wifi-band, cdc, discover"
            ));
        }
    };
    match result {
        CheckResult::Pass(m) => {
            println!("PASS  {}", m);
            Ok(())
        }
        CheckResult::Warn(m) => {
            println!("WARN  {}", m);
            std::process::exit(1);
        }
        CheckResult::Skip(m) => {
            println!("SKIP  {}", m);
            Ok(())
        }
        CheckResult::Fail(m, hint) => {
            println!("FAIL  {}", m);
            println!("hint: {}", hint);
            std::process::exit(2);
        }
    }
}

async fn run_cdc_all(reboot_to_run: bool) -> Result<()> {
    let ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        return Err(anyhow!(
            "no Pico in setup mode found (looking for VID 0x{:04X} PID 0x{:04X})",
            cdc::SETUP_VID,
            cdc::SETUP_PID,
        ));
    }

    let mut failures = 0usize;
    for port in ports {
        let port_for_task = port.clone();
        let probe =
            tokio::task::spawn_blocking(move || -> Result<(cdc::HelloAck, cdc::SelfTestAck)> {
                let mut pico = cdc::PicoSetup::open_named(&port_for_task)?;
                let hello = pico.hello()?;
                let self_test = pico.self_test()?;
                if reboot_to_run {
                    if !hello.creds_present() {
                        return Err(anyhow!(
                            "Pico has no saved Wi-Fi credentials; cannot reboot to run mode"
                        ));
                    }
                    pico.reboot_to_run()?;
                }
                Ok((hello, self_test))
            })
            .await;

        match probe {
            Ok(Ok((hello, self_test))) => {
                let status = if self_test.passed { "PASS" } else { "FAIL" };
                println!(
                    "{}  {} HELLO proto v{} fw v{} board=0x{:02X} creds={} SELF_TEST {}{}",
                    status,
                    port,
                    hello.proto_version,
                    hello.firmware_version(),
                    hello.board_type,
                    if hello.creds_present() {
                        "present"
                    } else {
                        "absent"
                    },
                    self_test.message,
                    if reboot_to_run { " -> RUN" } else { "" },
                );
                if !self_test.passed {
                    failures += 1;
                }
            }
            Ok(Err(e)) => {
                failures += 1;
                println!("FAIL  {} {}", port, e);
            }
            Err(e) => {
                failures += 1;
                println!("FAIL  {} CDC probe task failed: {}", port, e);
            }
        }
    }

    if failures > 0 {
        std::process::exit(2);
    }
    Ok(())
}

async fn run_cdc_one_reboot_to_run() -> Result<()> {
    let port = cdc::find_setup_port()?;
    let probe = tokio::task::spawn_blocking(move || -> Result<(String, cdc::HelloAck)> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        let self_test = pico.self_test()?;
        if !self_test.passed {
            return Err(anyhow!("SELF_TEST failed: {}", self_test.message));
        }
        if !hello.creds_present() {
            return Err(anyhow!(
                "Pico has no saved Wi-Fi credentials; cannot reboot to run mode"
            ));
        }
        pico.reboot_to_run()?;
        Ok((port, hello))
    })
    .await??;
    let (port, hello) = probe;
    println!(
        "PASS  {} HELLO proto v{} fw v{} board=0x{:02X} -> RUN",
        port,
        hello.proto_version,
        hello.firmware_version(),
        hello.board_type,
    );
    Ok(())
}

async fn run_discover_all() -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let replies = discovery::collect(&socket, Duration::from_secs(8)).await?;
    if replies.is_empty() {
        return Err(anyhow!("{}", support::no_pico_wifi_help(8)));
    }

    for (peer, info) in &replies {
        println!(
            "PASS  ack from {} proto v{} fw v{} board=0x{:02X} uid=0x{:08X} uptime={}s",
            peer,
            info.proto_version,
            info.firmware_version(),
            info.board_type,
            info.unique_id_short,
            info.uptime_seconds,
        );
    }
    println!("summary: {} unique Pico reply/replies", replies.len());
    Ok(())
}
