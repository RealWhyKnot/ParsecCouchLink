//! `couchlink configure-wifi` -- re-provision a Pico that's in setup mode
//! over USB-CDC. Prompts for SSID + password, sends `SET_WIFI`, then
//! `REBOOT_TO_RUN`.
//!
//! The password is read with `dialoguer::Password` (no echo), held in a
//! `String` only as long as needed, and zeroized on `Drop` via the helper
//! in `cdc.rs`. Neither SSID nor password ever hits disk.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Input, Password};
use zeroize::Zeroize;

use crate::tui::select;
use crate::{cdc, cmd_run, pico_mode};

pub async fn run() -> Result<()> {
    println!("couchlink configure-wifi");
    println!();
    println!(
        "Looking for a Pico in setup mode (VID 0x{:04X}, PID 0x{:04X})...",
        cdc::SETUP_VID,
        cdc::SETUP_PID
    );

    let Some(port) = find_setup_port_or_recover().await? else {
        return Ok(());
    };
    configure_setup_port(port).await
}

pub async fn run_for_port(port: String) -> Result<()> {
    println!("couchlink configure-wifi");
    println!();
    configure_setup_port(port).await
}

async fn configure_setup_port(port: String) -> Result<()> {
    println!("Found Pico on {port}");

    // dialoguer is blocking; isolate it from the async runtime.
    let mut creds = tokio::task::spawn_blocking(prompt_credentials).await??;

    let mut pico = cdc::PicoSetup::open_named(&port).context("opening CDC port for setup")?;
    let hello = pico.hello().context("CDC HELLO failed")?;
    println!(
        "  -> Pico firmware v{} (proto v{}, board 0x{:02X})",
        hello.firmware_version(),
        hello.proto_version,
        hello.board_type,
    );
    if hello.proto_version != cdc::PROTO_VERSION {
        bail!(
            "Pico speaks CDC protocol v{}, bridge speaks v{}. Update the side that's older.",
            hello.proto_version,
            cdc::PROTO_VERSION,
        );
    }

    // Move fields out of `creds` without destructuring (Drop on the parent
    // would forbid the destructure-move). After the takes, `creds.password`
    // is an empty String and the eventual Drop is a no-op.
    let before_wifi = wifi_uid_set().await?;
    let ssid = std::mem::take(&mut creds.ssid);
    let mut password = std::mem::take(&mut creds.password);
    drop(creds);
    println!("Sending Wi-Fi credentials to Pico...");
    let result = pico.set_wifi(&ssid, &mut password);
    // set_wifi zeroizes on success; double-belt for the error path.
    password.zeroize();
    drop(password);
    result.context("SET_WIFI failed")?;
    println!("  -> stored. Asking Pico to reboot into run mode.");

    pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
    println!();
    println!("Pico will reboot. Waiting up to 60 s for a Wi-Fi reply...");
    print_discovered_pico_ips(before_wifi).await?;
    Ok(())
}

async fn find_setup_port_or_recover() -> Result<Option<String>> {
    match pico_mode::wait_for_setup_port(Duration::from_secs(8)).await {
        Ok(port) => return Ok(Some(port)),
        Err(e) => {
            tracing::debug!("configure-wifi: no setup-mode USB after initial wait: {e:#}");
        }
    }

    println!();
    println!("No setup-mode USB appeared yet.");
    println!("Checking whether the Pico is already running on Wi-Fi...");
    match cmd_run::discover_picos(Duration::from_secs(5)).await {
        Ok(picos) if !picos.is_empty() => return handle_running_picos(picos).await,
        Ok(_) => {}
        Err(e) => println!("Wi-Fi discovery check failed: {e:#}"),
    }

    println!("No running Pico replied on Wi-Fi either. Continuing to wait for setup-mode USB...");
    pico_mode::wait_for_setup_port(Duration::from_secs(47))
        .await
        .map(Some)
        .context(
            "no Pico in setup mode. If the Pico is already working on Wi-Fi, \
             Wi-Fi setup can be skipped. If you need to change Wi-Fi, update \
             firmware with the newest package and try again.",
        )
}

async fn handle_running_picos(mut picos: Vec<cmd_run::PicoTarget>) -> Result<Option<String>> {
    println!("Found running Pico board(s) on Wi-Fi:");
    for (idx, pico) in picos.iter().enumerate() {
        println!("  {}. {}", idx + 1, pico.detail_label());
    }
    println!();
    println!("That means the Pico kept saved Wi-Fi and booted into its saved input mode.");

    let pico = if picos.len() == 1 {
        picos.remove(0)
    } else {
        let items: Vec<String> = picos.iter().map(|p| p.detail_label()).collect();
        let idx = select("Which Pico should change Wi-Fi?", &items, 0).await?;
        picos.remove(idx)
    };

    let choices = vec![
        "Use current Wi-Fi and stop",
        "Reboot this Pico into setup mode and change Wi-Fi",
        "Back",
    ];
    match select("Next step", &choices, 0).await? {
        0 => {
            println!("Keeping current Wi-Fi. Next: choose `Start streaming` from the main menu.");
            Ok(None)
        }
        1 => {
            println!(
                "Asking {} to reboot into setup-mode USB...",
                pico.short_label()
            );
            pico_mode::request_reboot_to_setup(&pico).await?;
            println!("Waiting for setup-mode USB...");
            pico_mode::wait_for_setup_port(Duration::from_secs(60))
                .await
                .map(Some)
                .context(
                    "the Pico did not reappear as setup-mode USB. If this firmware is older, \
                     update it with the newest ZIP first; otherwise unplug/replug the Pico and \
                     run `couchlink bundle`.",
                )
        }
        _ => Ok(None),
    }
}

async fn print_discovered_pico_ips(before: BTreeSet<u32>) -> Result<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(60);
    let mut next_beat = started + Duration::from_secs(10);
    // The setup-mode COM port we just rebooted lingers in the OS for a
    // second or two after REBOOT_TO_RUN, before the board re-enumerates as
    // the XInput device. Only treat a setup port as a genuine bounce
    // back to setup once it has first disappeared -- otherwise the stale
    // pre-reboot port reads as an instant "didn't join" even when the board
    // went on to join Wi-Fi normally.
    let mut setup_port_gone = false;
    loop {
        let picos = cmd_run::discover_picos(Duration::from_secs(2)).await?;
        let current: BTreeSet<u32> = picos.iter().map(|pico| pico.info.unique_id_short).collect();
        if !picos.is_empty() && current != before {
            print_pico_ips(&picos);
            return Ok(());
        }

        match cdc::find_setup_port() {
            // No setup port right now: the reboot took effect. A setup port
            // that appears *after* this point is a real bounce-back.
            Err(_) => setup_port_gone = true,
            Ok(port) if setup_port_gone => {
                println!();
                println!("The Pico came back in setup-mode USB before joining Wi-Fi.");
                println!("That usually means the SSID was not found, the password was rejected, or DHCP never completed.");
                print_setup_mode_diag(&port).await;
                println!("Fix the Wi-Fi details, then run `couchlink configure-wifi` again.");
                return Ok(());
            }
            // Pre-reboot setup port still lingering; ignore and keep waiting.
            Ok(_) => {}
        }

        let now = Instant::now();
        if now >= deadline {
            println!("No Pico replied yet.");
            println!("If your router shows its IP, choose `Enter Pico IP manually` from the menu.");
            println!(
                "If it still fails, run `couchlink bundle` and attach the zip to a bug report."
            );
            return Ok(());
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            println!("  ... still waiting for Wi-Fi reply ({elapsed}/60s)");
            next_beat = now + Duration::from_secs(10);
        }
    }
}

async fn wifi_uid_set() -> Result<BTreeSet<u32>> {
    Ok(cmd_run::discover_picos(Duration::from_secs(2))
        .await?
        .into_iter()
        .map(|pico| pico.info.unique_id_short)
        .collect())
}

fn print_pico_ips(picos: &[cmd_run::PicoTarget]) {
    for pico in picos {
        println!(
            "Pico replied at {}  fw v{}  uid 0x{:08X}",
            pico.peer,
            pico.info.firmware_version(),
            pico.info.unique_id_short,
        );
    }
    if picos.len() == 1 {
        println!("Confirmed Pico IP: {}", picos[0].peer.ip());
    } else {
        println!("Confirmed Pico IPs:");
        for pico in picos {
            println!("  {}  {}", pico.peer.ip(), pico.short_label());
        }
    }
}

async fn print_setup_mode_diag(port: &str) {
    let port = port.to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<(String, u32)> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        pico.get_log_buffer()
    })
    .await;

    match result {
        Ok(Ok((text, lost))) if !text.trim().is_empty() => {
            println!();
            if lost > 0 {
                println!(
                    "--- firmware Wi-Fi log (last 40 lines; {lost} older byte(s) dropped) ---"
                );
            } else {
                println!("--- firmware Wi-Fi log (last 40 lines) ---");
            }
            let lines: Vec<&str> = text
                .lines()
                .filter(|line| {
                    line.contains("wifi:")
                        || line.contains("assoc")
                        || line.contains("boot:")
                        || line.contains("flash_creds:")
                })
                .collect();
            let start = lines.len().saturating_sub(40);
            for line in &lines[start..] {
                if line.contains("BADAUTH")
                    || line.contains("NONET")
                    || line.contains("timed out")
                    || line.contains("auth rejected")
                    || line.contains("SSID not found")
                {
                    println!(">>> {line}");
                } else {
                    println!("    {line}");
                }
            }
            println!("--- end of firmware Wi-Fi log ---");
            println!();
        }
        Ok(Ok((_text, _lost))) => {
            println!("The firmware log was empty.");
        }
        Ok(Err(e)) => {
            println!("Could not read firmware log over setup USB: {e:#}");
        }
        Err(e) => {
            println!("Could not read firmware log over setup USB: {e}");
        }
    }
}

struct Credentials {
    ssid: String,
    password: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

fn prompt_credentials() -> Result<Credentials> {
    // Non-interactive override for scripted / headless provisioning: when
    // both COUCHLINK_WIFI_SSID and COUCHLINK_WIFI_PASSWORD are set, use them
    // instead of prompting. Same length limits as the interactive path.
    // Env vars keep the password out of argv and shell history.
    if let Ok(ssid) = std::env::var("COUCHLINK_WIFI_SSID") {
        if !ssid.is_empty() {
            let password = std::env::var("COUCHLINK_WIFI_PASSWORD").unwrap_or_default();
            if ssid.len() > 32 {
                bail!("COUCHLINK_WIFI_SSID can't be longer than 32 bytes");
            }
            if password.len() > 63 {
                bail!("COUCHLINK_WIFI_PASSWORD can't be longer than 63 bytes (WPA2 limit)");
            }
            println!("Using Wi-Fi credentials from COUCHLINK_WIFI_SSID/PASSWORD.");
            return Ok(Credentials { ssid, password });
        }
    }
    let theme = ColorfulTheme::default();
    let ssid: String = Input::with_theme(&theme)
        .with_prompt("Wi-Fi SSID (2.4 GHz network)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.is_empty() {
                Err("SSID can't be empty")
            } else if input.len() > 32 {
                Err("SSID can't be longer than 32 bytes")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    let password: String = Password::with_theme(&theme)
        .with_prompt("Wi-Fi password (hidden)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.len() > 63 {
                Err("password can't be longer than 63 bytes (WPA2 limit)")
            } else {
                Ok(())
            }
        })
        .interact()?;
    Ok(Credentials { ssid, password })
}
