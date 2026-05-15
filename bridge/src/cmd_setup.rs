//! `couchlink setup` -- end-to-end first-run wizard.
//!
//! Walks the operator through: hardware confirmation, BOOTSEL flash,
//! Wi-Fi provisioning over CDC, run-mode reboot, LAN discovery, XInput
//! smoke test, and Startup-folder shortcut install. Each stage is
//! re-entrant: re-running setup is safe.
//!
//! All `dialoguer` calls live on a blocking task so they don't stall the
//! Tokio runtime.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password};
use tokio::net::UdpSocket;
use zeroize::Zeroize;

use crate::{cdc, config, protocol};

pub async fn run(uf2_override: Option<PathBuf>) -> Result<()> {
    println!("couchlink setup");
    println!();
    println!(
        "This walks through preparing your Pico and connecting it to the \
         bridge. Pico 2 W and Pico W (or Pico WH) are both supported -- \
         the wizard picks the matching firmware automatically. Re-running \
         setup is safe."
    );
    println!();

    stage_preflight().await?;
    let uf2 = stage_pick_uf2(uf2_override).await?;
    stage_flash(uf2).await?;
    stage_wifi_provisioning().await?;
    let (peer_ip, identity) = stage_lan_discovery().await?;
    stage_smoke_test().await?;
    stage_install_autostart().await?;

    let mut cfg = config::load().unwrap_or_default();
    cfg.setup_complete = true;
    cfg.last_pico = Some(config::PicoIdentity {
        unique_id_short: identity.unique_id_short,
        board_type: identity.board_type,
        fw_major: identity.fw_major,
        fw_minor: identity.fw_minor,
        fw_patch: identity.fw_patch,
        last_ip: Some(peer_ip),
        device_name: None,
    });
    config::save(&cfg).context("saving config")?;

    println!();
    println!("Setup is complete. From now on, couchlink runs at logon.");
    println!("Have the remote player connect via Parsec to start using the bridge.");
    Ok(())
}

async fn stage_preflight() -> Result<()> {
    println!("[1/7] Pre-flight");
    config::ensure_dirs().context("creating config/log dirs")?;
    println!("  config dir: {}", config::config_dir()?.display());
    println!("  log dir:    {}", config::log_dir()?.display());

    let ok = ask_yes_no(
        "You should have on hand: a Raspberry Pi Pico 2 W or Pico W (RP2040), \
         a micro-USB data cable, your USB4MAPLE adapter, and your console. Ready?",
        true,
    )
    .await?;
    if !ok {
        bail!("setup aborted at pre-flight");
    }
    Ok(())
}

async fn stage_pick_uf2(uf2_override: Option<PathBuf>) -> Result<Option<PathBuf>> {
    println!();
    println!("[2/7] Pico firmware (.uf2)");

    if let Some(p) = uf2_override {
        if !p.exists() {
            bail!("UF2 not found: {}", p.display());
        }
        if p.is_file() {
            let ext_ok = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("uf2"))
                .unwrap_or(false);
            if !ext_ok {
                bail!("not a .uf2 file: {}", p.display());
            }
        }
        println!("  Using firmware from {}", p.display());
        return Ok(Some(p));
    }

    // No override: defer to the flash step, which detects which Pico
    // (RP2040 vs RP2350) is in BOOTSEL and picks the matching UF2 from
    // the release folder.
    println!("  Firmware will be auto-selected once the Pico is in BOOTSEL.");
    println!("  (couchlink-pico2w.uf2 for Pico 2 W, couchlink-picow.uf2 for Pico W / WH.)");
    Ok(None)
}

async fn stage_flash(uf2: Option<PathBuf>) -> Result<()> {
    println!();
    println!("[3/7] Flash the Pico");
    println!(
        "  Disconnect the Pico if it's already plugged in, then hold the BOOTSEL \
         button and plug it back in."
    );
    ask_press_enter("Press Enter when you've plugged the Pico in with BOOTSEL held.").await?;
    crate::cmd_flash::run(uf2).await?;
    println!("  Flash complete. The Pico is rebooting into setup mode.");
    Ok(())
}

async fn stage_wifi_provisioning() -> Result<()> {
    println!();
    println!("[4/7] Wi-Fi provisioning over USB-CDC");
    println!(
        "  Wait a few seconds for the Pico to come back as a USB serial device, \
         then continue."
    );

    let port = wait_for_setup_cdc(Duration::from_secs(60))
        .await
        .context("Pico did not appear as a USB serial device")?;
    println!("  Pico in setup mode on {port}");

    let mut pico = cdc::PicoSetup::open_named(&port).context("opening CDC port for setup")?;
    let hello = pico.hello().context("CDC HELLO failed")?;
    println!(
        "  Pico firmware v{}.{}.{} (proto v{}, board 0x{:02X})",
        hello.fw_major, hello.fw_minor, hello.fw_patch, hello.proto_version, hello.board_type,
    );
    if hello.proto_version != cdc::PROTO_VERSION {
        bail!(
            "CDC protocol mismatch: Pico v{}, bridge v{}",
            hello.proto_version,
            cdc::PROTO_VERSION,
        );
    }
    if hello.creds_present() {
        let overwrite = ask_yes_no(
            "This Pico already has Wi-Fi credentials stored. Overwrite them?",
            true,
        )
        .await?;
        if !overwrite {
            println!("  Keeping existing credentials. Skipping ahead to discovery.");
            pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
            return Ok(());
        }
    }

    let mut creds = tokio::task::spawn_blocking(prompt_wifi_credentials).await??;

    // mem::take out of the Drop-implementing struct (destructure-move is
    // disallowed; this leaves an empty struct for Drop to clean up).
    let ssid = std::mem::take(&mut creds.ssid);
    let mut password = std::mem::take(&mut creds.password);
    drop(creds);
    println!("  Sending credentials...");
    let result = pico.set_wifi(&ssid, &mut password);
    password.zeroize();
    result.context("SET_WIFI failed")?;
    println!("  Stored. Rebooting Pico into run mode.");
    pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
    Ok(())
}

async fn stage_lan_discovery() -> Result<(String, protocol::AckInfo)> {
    println!();
    println!("[5/7] LAN discovery");
    println!("  Waiting for the Pico to join your Wi-Fi and answer a discover broadcast...");
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;

    let timeout = Duration::from_secs(60);
    let started = std::time::Instant::now();
    let mut seq: u8 = 0;
    let discover_addr = "255.255.255.255:4242";
    let mut buf = [0u8; 64];

    loop {
        if started.elapsed() > timeout {
            bail!(
                "no Pico answered within {} s. Check Wi-Fi credentials with \
                 `couchlink configure-wifi`.",
                timeout.as_secs(),
            );
        }
        let pkt = protocol::Packet::discover(seq).encode();
        seq = seq.wrapping_add(1);
        let _ = socket.send_to(&pkt, discover_addr).await;
        match tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => match protocol::Packet::decode(&buf[..n]) {
                Ok(p) => {
                    if let protocol::PacketKind::Ack(info) = p.kind {
                        println!(
                            "  Pico on the LAN: {from} fw v{}.{}.{} uid 0x{:08X} uptime {}s",
                            info.fw_major,
                            info.fw_minor,
                            info.fw_patch,
                            info.unique_id_short,
                            info.uptime_seconds,
                        );
                        return Ok((from.ip().to_string(), info));
                    }
                }
                Err(e) => tracing::debug!("ignored malformed reply: {e}"),
            },
            _ => continue,
        }
    }
}

async fn stage_smoke_test() -> Result<()> {
    println!();
    println!("[6/7] Smoke test");
    println!(
        "  Plug a controller into your PC (or have someone join your Parsec with \
         their gamepad). Then press a button."
    );
    println!(
        "  This step doesn't actually move bytes through the Pico; it just \
         verifies XInput sees a controller. End-to-end input/output is exercised \
         the first time you press a button after run mode starts."
    );

    // XInput one-shot: wait up to 30 s for any state change.
    let ok = wait_for_xinput_input(Duration::from_secs(30)).await;
    if ok {
        println!("  XInput input observed. Looks good.");
    } else {
        println!(
            "  No XInput change in 30 s -- skipping. You can re-run `couchlink test xinput` later."
        );
    }
    Ok(())
}

async fn stage_install_autostart() -> Result<()> {
    println!();
    println!("[7/7] Autostart on logon");

    let install = ask_yes_no(
        "Install a Startup-folder shortcut so couchlink runs at every logon?",
        true,
    )
    .await?;
    if !install {
        println!("  Skipped. You can install later by running `couchlink setup` again.");
        return Ok(());
    }

    #[cfg(windows)]
    {
        let exe = std::env::current_exe().context("can't resolve own .exe path")?;
        let working_dir = exe.parent().map(|p| p.to_path_buf());
        let link_path = crate::known_folders::shortcut_path_for("Parsec CouchLink")?;
        crate::known_folders::create_shortcut(
            &link_path,
            &exe,
            "run",
            working_dir.as_deref(),
            "Parsec CouchLink",
        )?;
        println!("  Shortcut created at {}", link_path.display());
    }
    #[cfg(not(windows))]
    {
        println!("  (non-Windows -- autostart install is unsupported here)");
    }
    Ok(())
}

async fn ask_yes_no(prompt: &str, default: bool) -> Result<bool> {
    let prompt = prompt.to_string();
    let r = tokio::task::spawn_blocking(move || {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(default)
            .interact()
    })
    .await??;
    Ok(r)
}

async fn ask_press_enter(prompt: &str) -> Result<()> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::{BufRead, Write};
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        write!(stdout, "{} ", prompt)?;
        stdout.flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        Ok(())
    })
    .await??;
    Ok(())
}

struct WifiCreds {
    ssid: String,
    password: String,
}

impl Drop for WifiCreds {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

fn prompt_wifi_credentials() -> Result<WifiCreds> {
    let theme = ColorfulTheme::default();
    let ssid: String = Input::with_theme(&theme)
        .with_prompt("Wi-Fi SSID (must be a 2.4 GHz network)")
        .validate_with(|s: &String| -> Result<(), &str> {
            if s.is_empty() {
                Err("SSID can't be empty")
            } else if s.len() > 32 {
                Err("SSID can't be longer than 32 bytes")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    let password: String = Password::with_theme(&theme)
        .with_prompt("Wi-Fi password (hidden)")
        .validate_with(|s: &String| -> Result<(), &str> {
            if s.len() > 63 {
                Err("password can't be longer than 63 bytes (WPA2 limit)")
            } else {
                Ok(())
            }
        })
        .interact()?;
    Ok(WifiCreds { ssid, password })
}

async fn wait_for_setup_cdc(timeout: Duration) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(name) = cdc::find_setup_port() {
            return Ok(name);
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "Pico did not appear in setup mode within {} s. \
                 Confirm the firmware really booted -- the on-board LED should be on.",
                timeout.as_secs(),
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_xinput_input(timeout: Duration) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::XboxController::{
            XInputGetState, XINPUT_STATE, XUSER_MAX_COUNT,
        };
        let deadline = std::time::Instant::now() + timeout;
        let mut baseline: [u32; 4] = [u32::MAX; 4];
        for s in 0..XUSER_MAX_COUNT {
            let mut state = XINPUT_STATE::default();
            if unsafe { XInputGetState(s, &mut state) } == 0 {
                baseline[s as usize] = state.dwPacketNumber;
            }
        }
        while std::time::Instant::now() < deadline {
            for s in 0..XUSER_MAX_COUNT {
                let mut state = XINPUT_STATE::default();
                if unsafe { XInputGetState(s, &mut state) } == 0
                    && state.dwPacketNumber != baseline[s as usize]
                {
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = timeout;
        false
    }
}
