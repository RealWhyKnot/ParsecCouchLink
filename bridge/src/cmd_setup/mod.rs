mod diag_recovery;
mod setup_cdc;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use dialoguer::{Input, Password, theme::ColorfulTheme};
use zeroize::Zeroize;

use crate::modes::{BootselEntry, PicoScope};
use crate::{cdc, cmd_run, config, journal, protocol, udp_auth};
use diag_recovery::attempt_diag_recovery_after_lan_timeout;
use setup_cdc::{discover_run_mode_uid_baseline, wait_for_setup_cdc};

const SETUP_STAGE_NAMES: [&str; 6] = [
    "Pre-flight",
    "Pico firmware (.uf2)",
    "Flash the Pico",
    "Wi-Fi provisioning over USB-CDC",
    "LAN discovery",
    "Autostart on logon",
];

pub(crate) async fn run(uf2_override: Option<PathBuf>) -> Result<()> {
    journal!("setup", "wizard started");
    outln!("couchlink setup");
    outln!();
    outln!(
        "This walks through preparing your Pico and connecting it to the \
         bridge. Pico 2 W and Pico W (or Pico WH) are both supported -- \
         the wizard picks the matching firmware automatically. Re-running \
         setup is safe."
    );
    outln!();

    stage_preflight().await?;
    let uf2 = stage_pick_uf2(uf2_override)?;
    let pre_flash_run_uids = stage_flash(uf2).await?;
    stage_wifi_provisioning(pre_flash_run_uids.as_ref()).await?;
    let (peer_ip, identity) = stage_lan_discovery().await?;
    stage_install_autostart().await?;

    outln!();
    let nickname = crate::tui::input_text(
        "Name this Pico so it's easy to identify later, e.g. Dreamcast (blank to skip)",
    )
    .await
    .map(|s| s.trim().to_string())
    .ok()
    .filter(|s| !s.is_empty());

    let mut cfg = config::load().unwrap_or_default();
    cfg.setup_complete = true;
    cfg.remember_pico(config::PicoIdentity {
        unique_id_short: identity.unique_id_short,
        board_type: identity.board_type,
        fw_major: identity.fw_major,
        fw_minor: identity.fw_minor,
        fw_patch: identity.fw_patch,
        last_ip: Some(peer_ip.clone()),
        device_name: None,
        nickname,
    });
    config::save(&cfg).context("saving config")?;

    outln!();
    outln!("Setup is complete. From now on, couchlink runs at logon.");
    outln!("Confirmed Pico IP: {peer_ip}");
    outln!(
        "If discovery fails later, enter this IP manually or run `couchlink run --pico {peer_ip}`."
    );
    outln!(
        "Have the remote player connect via Parsec, then run `couchlink` and choose `Start streaming`."
    );
    Ok(())
}

async fn stage_preflight() -> Result<()> {
    print_stage(0);
    tracing::info!("setup: stage 1/6 -- pre-flight");
    journal!("setup", "stage 1/6 pre-flight");
    config::ensure_dirs().context("creating config/log dirs")?;
    outln!("  config dir: {}", config::config_dir()?.display());
    outln!("  log dir:    {}", config::log_dir()?.display());

    let ok = ask_yes_no(
        "You should have on hand: a Raspberry Pi Pico 2 W or Pico W (RP2040), \
         a micro-USB data cable, your USB4MAPLE adapter, and your console. Ready?",
        true,
    )
    .await?;
    if !ok {
        journal!(
            "setup",
            "aborted at pre-flight (operator declined readiness check)"
        );
        bail!("setup aborted at pre-flight");
    }
    Ok(())
}

fn stage_pick_uf2(uf2_override: Option<PathBuf>) -> Result<Option<PathBuf>> {
    outln!();
    print_stage(1);
    tracing::info!("setup: stage 2/6 -- pick UF2");

    if let Some(p) = uf2_override {
        if !p.exists() {
            bail!("UF2 not found: {}", p.display());
        }
        if p.is_file() {
            let ext_ok = p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("uf2"));
            if !ext_ok {
                bail!("not a .uf2 file: {}", p.display());
            }
        }
        outln!("  Using firmware from {}", p.display());
        return Ok(Some(p));
    }

    outln!("  Firmware will be auto-selected once the Pico is in BOOTSEL.");
    outln!("  (couchlink-pico2w.uf2 for Pico 2 W, couchlink-picow.uf2 for Pico W / WH.)");
    Ok(None)
}

async fn stage_flash(uf2: Option<PathBuf>) -> Result<Option<HashSet<u32>>> {
    outln!();
    print_stage(2);
    tracing::info!("setup: stage 3/6 -- flash");
    outln!("  How to put the Pico into BOOTSEL (flashing) mode:");
    outln!();
    outln!("    1. Unplug the Pico if it is currently connected.");
    outln!("    2. Press and HOLD the BOOTSEL button on the Pico.");
    outln!("    3. With BOOTSEL still held, plug the Pico into this PC");
    outln!("       using a micro-USB DATA cable (charge-only cables fail).");
    outln!("    4. Watch File Explorer for a new removable drive named");
    outln!("       RPI-RP2 (Pico W or Pico WH) or RP2350 (Pico 2 W).");
    outln!("    5. RELEASE the BOOTSEL button as soon as that drive appears.");
    outln!("       The Pico stays in BOOTSEL mode after you let go -- you");
    outln!("       do NOT need to keep the button held during the copy.");
    outln!();
    outln!("  After the copy finishes, the Pico will reboot into our");
    outln!("  firmware automatically. Do NOT press BOOTSEL during that");
    outln!("  reboot. The firmware reads BOOTSEL during its first three");
    outln!("  seconds of run time as a \"wipe saved Wi-Fi credentials\"");
    outln!("  signal -- a stray press during reboot will erase the");
    outln!("  credentials you are about to enter.");
    outln!();
    ask_press_enter("Press Enter once the RPI-RP2 or RP2350 drive has appeared in Windows.")
        .await?;
    let pre_flash_run_uids = discover_run_mode_uid_baseline().await;
    crate::cmd_flash::run(uf2, PicoScope::Selected, BootselEntry::Manual).await?;
    outln!("  Flash complete. The Pico is rebooting into setup mode --");
    outln!("  leave the BOOTSEL button alone during this reboot.");
    tracing::info!("setup: stage 3/6 complete -- Pico rebooting into setup mode");
    Ok(pre_flash_run_uids)
}

async fn stage_wifi_provisioning(pre_flash_run_uids: Option<&HashSet<u32>>) -> Result<()> {
    outln!();
    print_stage(3);
    tracing::info!("setup: stage 4/6 -- Wi-Fi provisioning");
    journal!("setup", "stage 4/6 Wi-Fi provisioning over USB-CDC");
    outln!(
        "  Wait a few seconds for the Pico to come back as a USB serial device, \
         then continue."
    );

    let port = wait_for_setup_cdc(Duration::from_secs(120), pre_flash_run_uids)
        .await
        .context("Pico did not appear as a USB serial device")?;
    outln!("  Pico in setup mode on {port}");
    tracing::info!("setup: setup-mode CDC port found at {port}");
    journal!("setup", "setup-mode CDC port enumerated at {port}");

    let mut pico = cdc::PicoSetup::open_named(&port).context("opening CDC port for setup")?;
    let hello = pico.hello().context("CDC HELLO failed")?;
    outln!(
        "  Pico firmware v{} (proto v{}, board 0x{:02X})",
        hello.firmware_version(),
        hello.proto_version,
        hello.board_type,
    );
    tracing::info!(
        "setup: HELLO ok -- fw v{} proto v{} board 0x{:02X} creds_present={}",
        hello.firmware_version(),
        hello.proto_version,
        hello.board_type,
        hello.creds_present(),
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
            outln!("  Keeping existing credentials. Skipping ahead to discovery.");
            pair_over_usb(&mut pico)?;
            tracing::info!("setup: keeping existing creds, rebooting to run mode");
            pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
            return Ok(());
        }
        tracing::info!("setup: existing creds will be overwritten");
    }

    let mut creds = tokio::task::spawn_blocking(prompt_wifi_credentials).await??;

    let ssid = std::mem::take(&mut creds.ssid);
    let mut password = std::mem::take(&mut creds.password);
    drop(creds);
    outln!("  Sending credentials...");
    tracing::debug!(
        "setup: sending SET_WIFI ssid_len={} password_len={}",
        ssid.len(),
        password.len(),
    );
    let result = pico.set_wifi(&ssid, &mut password);
    password.zeroize();
    result.context("SET_WIFI failed")?;
    pair_over_usb(&mut pico)?;
    outln!("  Stored. Rebooting Pico into run mode.");
    tracing::info!("setup: SET_WIFI ack received, rebooting to run mode");
    pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
    Ok(())
}

fn pair_over_usb(pico: &mut cdc::PicoSetup) -> Result<()> {
    let paired = udp_auth::provision(pico)?;
    tracing::info!("setup: udp auth key provisioned={paired}");
    outln!("  {}", udp_auth::pairing_summary(paired));
    Ok(())
}

async fn stage_lan_discovery() -> Result<(String, protocol::AckInfo)> {
    outln!();
    print_stage(4);
    tracing::info!("setup: stage 5/6 -- LAN discovery");
    outln!("  Waiting for the Pico to join your Wi-Fi and answer a discover broadcast...");
    let socket = crate::net::bind_udp("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;

    let timeout = Duration::from_secs(60);
    let started = std::time::Instant::now();
    let mut seq: u8 = 0;
    let discover_addr = "255.255.255.255:4242";
    let mut buf = [0u8; 64];

    loop {
        if started.elapsed() > timeout {
            tracing::error!(
                "setup: discovery timeout after {} s, {} broadcasts sent",
                timeout.as_secs(),
                seq,
            );
            if let Some((peer_ip, identity)) = prompt_manual_discovery_ip().await? {
                return Ok((peer_ip, identity));
            }
            attempt_diag_recovery_after_lan_timeout().await;
            bail!(
                "no Pico answered within {} s. Check Wi-Fi credentials with \
                 `couchlink configure-wifi`.",
                timeout.as_secs(),
            );
        }
        let pkt = udp_auth::discover_frame(seq);
        seq = seq.wrapping_add(1);
        let _ = socket.send_to(&pkt, discover_addr).await;
        let reply = tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await;
        if let Ok(Ok((n, from))) = reply {
            match udp_auth::decode_reply(from, &buf[..n]) {
                Ok(p) => {
                    if let protocol::PacketKind::Ack(info) = p.kind {
                        outln!(
                            "  Pico on the LAN: {from} fw v{} uid 0x{:08X} uptime {}s",
                            info.firmware_version(),
                            info.unique_id_short,
                            info.uptime_seconds,
                        );
                        tracing::info!(
                            "setup: discovery ack from {from} fw v{} uid 0x{:08X} \
                             uptime {}s after {} broadcasts",
                            info.firmware_version(),
                            info.unique_id_short,
                            info.uptime_seconds,
                            seq,
                        );
                        if let Err(e) = udp_auth::check_access(from) {
                            outln!("  Warning: {e}");
                        }
                        return Ok((from.ip().to_string(), info));
                    }
                }
                Err(e) => tracing::debug!("ignored malformed reply: {e}"),
            }
        }
    }
}

async fn prompt_manual_discovery_ip() -> Result<Option<(String, protocol::AckInfo)>> {
    outln!();
    outln!(
        "No broadcast discovery reply arrived. If your router shows the Pico's IP address, enter it now to probe directly."
    );
    let text = prompt_optional_text("Pico IP address (blank to skip)").await?;
    let Some(ip) = cmd_run::parse_ip_selector(&text) else {
        if !text.trim().is_empty() {
            outln!("  Not a valid IP address: {}", text.trim());
        }
        return Ok(None);
    };
    outln!("  Probing {ip}:{}...", protocol::PORT);
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await {
        Ok(pico) => {
            outln!(
                "  Pico replied at {} fw v{} uid 0x{:08X}",
                pico.peer,
                pico.info.firmware_version(),
                pico.info.unique_id_short,
            );
            outln!("  Save this IP for manual routing: {}", pico.peer.ip());
            Ok(Some((pico.peer.ip().to_string(), pico.info)))
        }
        Err(e) => {
            outln!("  Manual IP probe failed: {e:#}");
            Ok(None)
        }
    }
}

async fn stage_install_autostart() -> Result<()> {
    outln!();
    print_stage(5);
    tracing::info!("setup: stage 6/6 -- autostart shortcut");

    let install = ask_yes_no(
        "Install a Startup-folder shortcut so couchlink runs at every logon?",
        true,
    )
    .await?;
    if !install {
        outln!("  Skipped. You can install later by running `couchlink setup` again.");
        return Ok(());
    }

    #[cfg(windows)]
    {
        let exe = std::env::current_exe().context("can't resolve own .exe path")?;
        let working_dir = exe.parent().map(std::path::Path::to_path_buf);
        let link_path = crate::known_folders::shortcut_path_for("Parsec CouchLink")?;
        crate::known_folders::create_shortcut(
            &link_path,
            &exe,
            "run",
            working_dir.as_deref(),
            "Parsec CouchLink",
        )?;
        outln!("  Shortcut created at {}", link_path.display());
    }
    #[cfg(not(windows))]
    {
        outln!("  (non-Windows -- autostart install is unsupported here)");
    }
    Ok(())
}

fn print_stage(index: usize) {
    outln!(
        "[{}/{}] {}",
        index + 1,
        SETUP_STAGE_NAMES.len(),
        SETUP_STAGE_NAMES[index]
    );
}

async fn ask_yes_no(prompt: &str, default: bool) -> Result<bool> {
    crate::tui::confirm(prompt, default).await
}

async fn ask_press_enter(prompt: &str) -> Result<()> {
    crate::tui::press_enter(prompt).await
}

async fn prompt_optional_text(prompt: &str) -> Result<String> {
    crate::tui::input_text(prompt).await
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

#[cfg(test)]
mod tests;
