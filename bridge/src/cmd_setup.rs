//! `couchlink setup` -- end-to-end first-run wizard.
//!
//! Walks the operator through: hardware confirmation, BOOTSEL flash,
//! Wi-Fi provisioning over CDC, run-mode reboot, LAN discovery, and
//! Startup-folder shortcut install. Each stage is re-entrant:
//! re-running setup is safe.
//!
//! All `dialoguer` calls live on a blocking task so they don't stall the
//! Tokio runtime.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Input, Password};
use zeroize::Zeroize;

use crate::{cdc, cmd_run, config, diag_usb, journal, pico_mode, protocol};

const SETUP_STAGE_NAMES: [&str; 6] = [
    "Pre-flight",
    "Pico firmware (.uf2)",
    "Flash the Pico",
    "Wi-Fi provisioning over USB-CDC",
    "LAN discovery",
    "Autostart on logon",
];

pub async fn run(uf2_override: Option<PathBuf>) -> Result<()> {
    journal!("setup", "wizard started");
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
    let pre_flash_run_uids = stage_flash(uf2).await?;
    stage_wifi_provisioning(pre_flash_run_uids.as_ref()).await?;
    let (peer_ip, identity) = stage_lan_discovery().await?;
    stage_install_autostart().await?;

    // Ask for a name while the physical board is unambiguous -- it is the
    // one Pico that just went through this wizard on the USB cable. With
    // several Picos saved, the name is how the home screen tells them
    // apart. Blank keeps any previously saved name.
    println!();
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

    println!();
    println!("Setup is complete. From now on, couchlink runs at logon.");
    println!("Confirmed Pico IP: {peer_ip}");
    println!(
        "If discovery fails later, enter this IP manually or run `couchlink run --pico {peer_ip}`."
    );
    println!("Have the remote player connect via Parsec, then run `couchlink` and choose `Start streaming`.");
    Ok(())
}

async fn stage_preflight() -> Result<()> {
    print_stage(0);
    tracing::info!("setup: stage 1/6 -- pre-flight");
    journal!("setup", "stage 1/6 pre-flight");
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
        journal!(
            "setup",
            "aborted at pre-flight (operator declined readiness check)"
        );
        bail!("setup aborted at pre-flight");
    }
    Ok(())
}

async fn stage_pick_uf2(uf2_override: Option<PathBuf>) -> Result<Option<PathBuf>> {
    println!();
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

async fn stage_flash(uf2: Option<PathBuf>) -> Result<Option<HashSet<u32>>> {
    println!();
    print_stage(2);
    tracing::info!("setup: stage 3/6 -- flash");
    println!("  How to put the Pico into BOOTSEL (flashing) mode:");
    println!();
    println!("    1. Unplug the Pico if it is currently connected.");
    println!("    2. Press and HOLD the BOOTSEL button on the Pico.");
    println!("    3. With BOOTSEL still held, plug the Pico into this PC");
    println!("       using a micro-USB DATA cable (charge-only cables fail).");
    println!("    4. Watch File Explorer for a new removable drive named");
    println!("       RPI-RP2 (Pico W or Pico WH) or RP2350 (Pico 2 W).");
    println!("    5. RELEASE the BOOTSEL button as soon as that drive appears.");
    println!("       The Pico stays in BOOTSEL mode after you let go -- you");
    println!("       do NOT need to keep the button held during the copy.");
    println!();
    println!("  After the copy finishes, the Pico will reboot into our");
    println!("  firmware automatically. Do NOT press BOOTSEL during that");
    println!("  reboot. The firmware reads BOOTSEL during its first three");
    println!("  seconds of run time as a \"wipe saved Wi-Fi credentials\"");
    println!("  signal -- a stray press during reboot will erase the");
    println!("  credentials you are about to enter.");
    println!();
    ask_press_enter("Press Enter once the RPI-RP2 or RP2350 drive has appeared in Windows.")
        .await?;
    let pre_flash_run_uids = discover_run_mode_uid_baseline().await;
    crate::cmd_flash::run(uf2, false, false).await?;
    println!("  Flash complete. The Pico is rebooting into setup mode --");
    println!("  leave the BOOTSEL button alone during this reboot.");
    tracing::info!("setup: stage 3/6 complete -- Pico rebooting into setup mode");
    Ok(pre_flash_run_uids)
}

async fn stage_wifi_provisioning(pre_flash_run_uids: Option<&HashSet<u32>>) -> Result<()> {
    println!();
    print_stage(3);
    tracing::info!("setup: stage 4/6 -- Wi-Fi provisioning");
    journal!("setup", "stage 4/6 Wi-Fi provisioning over USB-CDC");
    println!(
        "  Wait a few seconds for the Pico to come back as a USB serial device, \
         then continue."
    );

    let port = wait_for_setup_cdc(Duration::from_secs(120), pre_flash_run_uids)
        .await
        .context("Pico did not appear as a USB serial device")?;
    println!("  Pico in setup mode on {port}");
    tracing::info!("setup: setup-mode CDC port found at {port}");
    journal!("setup", "setup-mode CDC port enumerated at {port}");

    let mut pico = cdc::PicoSetup::open_named(&port).context("opening CDC port for setup")?;
    let hello = pico.hello().context("CDC HELLO failed")?;
    println!(
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
            println!("  Keeping existing credentials. Skipping ahead to discovery.");
            tracing::info!("setup: keeping existing creds, rebooting to run mode");
            pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
            return Ok(());
        }
        tracing::info!("setup: existing creds will be overwritten");
    }

    let mut creds = tokio::task::spawn_blocking(prompt_wifi_credentials).await??;

    // mem::take out of the Drop-implementing struct (destructure-move is
    // disallowed; this leaves an empty struct for Drop to clean up).
    let ssid = std::mem::take(&mut creds.ssid);
    let mut password = std::mem::take(&mut creds.password);
    drop(creds);
    println!("  Sending credentials...");
    tracing::debug!(
        "setup: sending SET_WIFI ssid_len={} password_len={}",
        ssid.len(),
        password.len(),
    );
    let result = pico.set_wifi(&ssid, &mut password);
    password.zeroize();
    result.context("SET_WIFI failed")?;
    println!("  Stored. Rebooting Pico into run mode.");
    tracing::info!("setup: SET_WIFI ack received, rebooting to run mode");
    pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
    Ok(())
}

async fn stage_lan_discovery() -> Result<(String, protocol::AckInfo)> {
    println!();
    print_stage(4);
    tracing::info!("setup: stage 5/6 -- LAN discovery");
    println!("  Waiting for the Pico to join your Wi-Fi and answer a discover broadcast...");
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
        let pkt = protocol::Packet::discover(seq).encode();
        seq = seq.wrapping_add(1);
        let _ = socket.send_to(&pkt, discover_addr).await;
        match tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => match protocol::Packet::decode(&buf[..n]) {
                Ok(p) => {
                    if let protocol::PacketKind::Ack(info) = p.kind {
                        println!(
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
                        return Ok((from.ip().to_string(), info));
                    }
                }
                Err(e) => tracing::debug!("ignored malformed reply: {e}"),
            },
            _ => continue,
        }
    }
}

async fn prompt_manual_discovery_ip() -> Result<Option<(String, protocol::AckInfo)>> {
    println!();
    println!(
        "No broadcast discovery reply arrived. If your router shows the Pico's IP address, enter it now to probe directly."
    );
    let text = prompt_optional_text("Pico IP address (blank to skip)").await?;
    let Some(ip) = cmd_run::parse_ip_selector(&text) else {
        if !text.trim().is_empty() {
            println!("  Not a valid IP address: {}", text.trim());
        }
        return Ok(None);
    };
    println!("  Probing {ip}:{}...", protocol::PORT);
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await {
        Ok(pico) => {
            println!(
                "  Pico replied at {} fw v{} uid 0x{:08X}",
                pico.peer,
                pico.info.firmware_version(),
                pico.info.unique_id_short,
            );
            println!("  Save this IP for manual routing: {}", pico.peer.ip());
            Ok(Some((pico.peer.ip().to_string(), pico.info)))
        }
        Err(e) => {
            println!("  Manual IP probe failed: {e:#}");
            Ok(None)
        }
    }
}

/// After a stage-5 LAN discovery timeout, wait for the Pico to re-appear
/// in setup mode (the firmware's Wi-Fi association watchdog auto-bounces
/// back after ~30 s of failed association) and pull its diag log via the
/// WinUSB vendor interface. Prints the last 50 lines with `assoc_result`
/// lines prefixed by `>>>`. If the port does not re-appear within the
/// wait window, prints a fallback message instead. Either way, returns
/// without error so the caller can `bail!()` normally.
async fn attempt_diag_recovery_after_lan_timeout() {
    const RECOVERY_WINDOW: Duration = Duration::from_secs(45);
    const POLL_INTERVAL: Duration = Duration::from_millis(500);

    println!();
    println!(
        "Pico did not answer on the LAN within 60 s. \
         Trying to retrieve the firmware diag log..."
    );
    journal!(
        "setup",
        "stage 5 timeout -- trying diag recovery (window 45 s)"
    );
    tracing::info!("setup: stage-5 timeout; waiting up to 45 s for setup-mode port to re-appear");

    let port = {
        let deadline = std::time::Instant::now() + RECOVERY_WINDOW;
        let mut found = None;
        let mut beat_at = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(p) = cdc::find_setup_port() {
                found = Some(p);
                break;
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            if now >= beat_at {
                let elapsed = (RECOVERY_WINDOW - deadline.saturating_duration_since(now)).as_secs();
                println!(
                    "  ... still waiting for Pico to re-appear in setup mode ({}/45 s)",
                    elapsed
                );
                beat_at = now + Duration::from_secs(10);
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        found
    };

    let Some(port) = port else {
        tracing::warn!("setup: diag recovery failed -- Pico did not re-appear in setup mode");
        journal!(
            "setup",
            "diag recovery failed -- no setup-mode port within 45 s"
        );
        println!();
        println!(
            "Could not retrieve diag log -- the Pico did not re-appear in setup mode. \
             This may mean the firmware predates the association watchdog (rebuild from \
             main and reflash), or the Pico has a more fundamental boot issue \
             (try `couchlink bundle` for a full system snapshot)."
        );
        print_lan_timeout_walkthrough();
        return;
    };

    tracing::info!("setup: setup-mode port re-appeared at {port}, pulling diag log via WinUSB");
    journal!(
        "setup",
        "diag recovery -- setup-mode port re-appeared at {port}"
    );
    println!("  Setup-mode port re-appeared at {port}. Pulling diag log...");

    let outcome = tokio::task::spawn_blocking(diag_usb::capture_diag_blocking)
        .await
        .unwrap_or_else(|e| diag_usb::VendorDiagOutcome::TransferFailed {
            step: "spawn",
            bytes_received: 0,
            error: format!("blocking task panicked: {e}"),
        });

    match outcome {
        diag_usb::VendorDiagOutcome::Captured { text, lost } => {
            tracing::info!(
                "setup: diag recovery succeeded -- {} bytes, {} lost",
                text.len(),
                lost,
            );
            journal!(
                "setup",
                "diag recovery succeeded -- {} bytes captured",
                text.len()
            );
            println!();
            if lost > 0 {
                println!(
                    "--- firmware diag log (last 50 lines; {lost} byte(s) dropped from ring) ---"
                );
            } else {
                println!("--- firmware diag log (last 50 lines) ---");
            }
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(50);
            for line in &lines[start..] {
                if line.contains("assoc_result") {
                    println!(">>> {line}");
                } else {
                    println!("    {line}");
                }
            }
            println!("--- end of diag log ---");
            println!();
            println!(
                "If the diag log above names a Wi-Fi error \
                 (BADAUTH / NONET / NOIP / timeout), that is the exact cause. \
                 Otherwise, try the following in order:"
            );
            print_lan_timeout_walkthrough();
        }
        diag_usb::VendorDiagOutcome::Empty => {
            tracing::info!("setup: diag recovery -- vendor pull returned empty ring");
            journal!("setup", "diag recovery -- vendor pull empty");
            println!("  Diag log was empty -- the Pico re-appeared but its log ring is clear.");
            print_lan_timeout_walkthrough();
        }
        other => {
            tracing::warn!("setup: diag recovery vendor pull failed: {other:?}");
            journal!("setup", "diag recovery failed -- vendor pull error");
            println!(
                "  Pico re-appeared in setup mode but the vendor diag pull did not succeed \
                 ({other:?}). Run `couchlink bundle` for a full system snapshot."
            );
            print_lan_timeout_walkthrough();
        }
    }
}

fn print_lan_timeout_walkthrough() {
    println!();
    println!("  1. Confirm your SSID is a real 2.4 GHz network on your router");
    println!("     (not a 5 GHz-only SSID -- many routers show only the 5 GHz SSID by default).");
    println!("  2. Confirm the Pico is powered on and within Wi-Fi range.");
    println!("  3. If you have changed Wi-Fi networks since setup, hold BOOTSEL for 3+ seconds");
    println!("     during plug-in to wipe the saved creds, then re-run `couchlink setup`.");
    println!("  4. If you have multiple network adapters, make sure the bridge is allowed");
    println!("     through Windows Firewall on the active profile.");
    println!("     `couchlink bundle` includes the firewall and network snapshot.");
    println!("  5. Run `couchlink bundle` and attach the resulting zip to a bug report.");
}

async fn stage_install_autostart() -> Result<()> {
    println!();
    print_stage(5);
    tracing::info!("setup: stage 6/6 -- autostart shortcut");

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

fn print_stage(index: usize) {
    println!(
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

async fn discover_run_mode_uid_baseline() -> Option<HashSet<u32>> {
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

async fn wait_for_setup_cdc(
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
            bail!(
                "Pico did not appear as a USB CDC serial device within {} s. \
                 Open Device Manager and check Ports (COM & LPT) for a new COM port, \
                 or Other devices for an entry with VID 2E8A:CAF0. \
                 If nothing shows at all, the firmware likely did not boot -- try a \
                 different micro-USB data cable or USB port and re-run setup. \
                 If a COM port does appear but setup still fails, run \
                 `couchlink.exe bundle` and attach the resulting zip to a bug report.",
                timeout.as_secs(),
            );
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            println!("  ... still waiting for USB enumeration ({elapsed}s/{total}s)");
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
                    println!(
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

enum SetupRecoveryProbe {
    One(cmd_run::PicoTarget),
    Ambiguous { total: usize, candidates: usize },
    None,
}

async fn find_reboot_to_setup_candidate(
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

fn setup_recovery_candidate_uids(
    picos: &[cmd_run::PicoTarget],
    pre_flash_run_uids: &HashSet<u32>,
) -> Vec<u32> {
    picos
        .iter()
        .filter(|p| !pre_flash_run_uids.contains(&p.info.unique_id_short))
        .map(|p| p.info.unique_id_short)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{setup_recovery_candidate_uids, SETUP_STAGE_NAMES};
    use crate::{cmd_run::PicoTarget, protocol};
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn first_run_setup_does_not_require_controller_input() {
        let joined = SETUP_STAGE_NAMES.join(" ").to_ascii_lowercase();
        assert_eq!(SETUP_STAGE_NAMES.len(), 6);
        assert!(!joined.contains("xinput"));
        assert!(!joined.contains("controller"));
        assert!(!joined.contains("smoke"));
    }

    #[test]
    fn setup_recovery_targets_only_new_run_mode_picos() {
        let old = pico_target(0x11111111);
        let flashed = pico_target(0x22222222);
        let mut baseline = HashSet::new();
        baseline.insert(old.info.unique_id_short);

        let candidates = setup_recovery_candidate_uids(&[old, flashed], &baseline);

        assert_eq!(candidates, vec![0x22222222]);
    }

    #[test]
    fn setup_recovery_does_not_target_baseline_picos() {
        let old = pico_target(0x11111111);
        let mut baseline = HashSet::new();
        baseline.insert(old.info.unique_id_short);

        let candidates = setup_recovery_candidate_uids(&[old], &baseline);

        assert!(candidates.is_empty());
    }

    fn pico_target(uid: u32) -> PicoTarget {
        PicoTarget {
            peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4242),
            info: protocol::AckInfo {
                proto_version: protocol::PROTO_VERSION,
                fw_major: 26,
                fw_minor: 6,
                fw_patch: 16,
                uptime_seconds: 1,
                unique_id_short: uid,
                board_type: protocol::BOARD_PICO_2_W,
                full_version: None,
            },
            persona: protocol::Persona::Ps4,
            ack_flags: protocol::ACK_FLAG_USB_DIAG_SUPPORTED,
        }
    }
}
