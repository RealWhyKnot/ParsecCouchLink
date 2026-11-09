use super::inventory::{PicoInventory, SetupUsbPico};
use super::{route_one, stream};
use crate::tui::{confirm, input_text, press_enter, select};
use crate::{
    cmd_configure_wifi, cmd_debug, cmd_flash, cmd_run, cmd_setup, cmd_usb_diag, config, pico_mode,
    protocol,
};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PicoAction {
    StartStreaming {
        target: cmd_run::PicoTarget,
    },
    ChooseRouting {
        target: cmd_run::PicoTarget,
    },
    CheckUsbAdapter {
        target: cmd_run::PicoTarget,
    },
    SwitchToUsbDebug {
        target: cmd_run::PicoTarget,
    },
    BlinkLed {
        target: cmd_run::PicoTarget,
    },
    RecoverToWifi {
        port: String,
    },
    ConfigureWifi {
        port: String,
    },
    UpdateFirmwareFromSetupUsb {
        port: String,
    },
    ReadUsbLog {
        port: String,
    },
    FlashBootsel {
        mount: PathBuf,
        board: cmd_flash::BootselBoard,
    },
    FindLastIp {
        identity: config::PicoIdentity,
        ip: String,
    },
    SaveIdentity {
        identity: config::PicoIdentity,
        nickname_suggestion: Option<String>,
    },
    Rename {
        identity: config::PicoIdentity,
        nickname_suggestion: Option<String>,
    },
    RemoveSaved {
        identity: config::PicoIdentity,
    },
}

impl PicoAction {
    pub(super) fn label(&self) -> String {
        match self {
            Self::StartStreaming { .. } => "Start streaming".to_string(),
            Self::ChooseRouting { .. } => "Choose controller and stream".to_string(),
            Self::CheckUsbAdapter { .. } => "Check console USB adapter".to_string(),
            Self::SwitchToUsbDebug { .. } => "Switch to USB debug mode".to_string(),
            Self::BlinkLed { .. } => "Blink the LED".to_string(),
            Self::RecoverToWifi { .. } => "Recover to Wi-Fi/input mode".to_string(),
            Self::ConfigureWifi { .. } => "Set up or change Wi-Fi".to_string(),
            Self::UpdateFirmwareFromSetupUsb { .. } => "Update firmware".to_string(),
            Self::ReadUsbLog { .. } => "Read USB debug log".to_string(),
            Self::FlashBootsel { .. } => "Flash or reinstall firmware".to_string(),
            Self::FindLastIp { ip, .. } => format!("Find at last IP {ip}"),
            Self::SaveIdentity { .. } => "Save this Pico".to_string(),
            Self::Rename { .. } => "Name or rename this Pico".to_string(),
            Self::RemoveSaved { .. } => "Remove saved Pico".to_string(),
        }
    }

    pub(super) fn help(&self) -> String {
        match self {
            Self::StartStreaming { .. } => "stream this Pico only".to_string(),
            Self::ChooseRouting { .. } => "pick one Windows controller for this Pico".to_string(),
            Self::CheckUsbAdapter { .. } => "query this Pico's USB adapter status".to_string(),
            Self::BlinkLed { .. } => {
                "flash this Pico's onboard LED so you can spot the board".to_string()
            }
            Self::SwitchToUsbDebug { .. } => "move this Wi-Fi Pico to setup USB".to_string(),
            Self::RecoverToWifi { .. } => "move this USB Pico back to normal mode".to_string(),
            Self::ConfigureWifi { .. } => "send Wi-Fi credentials to this USB Pico".to_string(),
            Self::UpdateFirmwareFromSetupUsb { .. } => {
                "reboot this USB Pico to BOOTSEL and flash it".to_string()
            }
            Self::ReadUsbLog { .. } => "show recent firmware messages from this Pico".to_string(),
            Self::FlashBootsel { .. } => "copy the matching UF2 to this BOOTSEL drive".to_string(),
            Self::FindLastIp { .. } => "probe the saved IP for this Pico".to_string(),
            Self::SaveIdentity { .. } => "add this Pico to the saved list".to_string(),
            Self::Rename { .. } => "set the name shown for this Pico (e.g. Dreamcast)".to_string(),
            Self::RemoveSaved { .. } => "forget this saved Pico and its routes".to_string(),
        }
    }

    pub(super) fn target_hint(&self) -> String {
        match self {
            Self::StartStreaming { target }
            | Self::ChooseRouting { target }
            | Self::CheckUsbAdapter { target }
            | Self::BlinkLed { target }
            | Self::SwitchToUsbDebug { target } => {
                format!("target {} / {}", target.uid_hex(), target.peer.ip())
            }
            Self::RecoverToWifi { port }
            | Self::ConfigureWifi { port }
            | Self::UpdateFirmwareFromSetupUsb { port }
            | Self::ReadUsbLog { port } => format!("target {port}"),
            Self::FlashBootsel { mount, .. } => format!("target {}", mount.display()),
            Self::FindLastIp { identity, ip } => {
                format!("target {} at {ip}", identity.uid_hex())
            }
            Self::SaveIdentity { identity, .. }
            | Self::Rename { identity, .. }
            | Self::RemoveSaved { identity } => {
                format!("target {}", identity.uid_hex())
            }
        }
    }
}

pub(super) async fn run_pico_action(action: PicoAction) -> Result<()> {
    match action {
        PicoAction::StartStreaming { target } => {
            stream(
                vec![cmd_run::StreamRoute {
                    source_slot: 0,
                    pico: target,
                }],
                true,
            )
            .await
        }
        PicoAction::ChooseRouting { target } => route_one(vec![target]).await,
        PicoAction::CheckUsbAdapter { target } => {
            cmd_usb_diag::run_for_targets(&[target]).await?;
            press_enter("Press Enter to return to Basic.").await
        }
        PicoAction::SwitchToUsbDebug { target } => {
            cmd_debug::switch_wifi_target_to_usb_debug(target).await
        }
        PicoAction::BlinkLed { target } => blink_pico_led(target).await,
        PicoAction::RecoverToWifi { port } => {
            cmd_debug::switch_setup_port_to_wifi_target(port).await
        }
        PicoAction::ConfigureWifi { port } => cmd_configure_wifi::run_for_port(port).await,
        PicoAction::UpdateFirmwareFromSetupUsb { port } => update_setup_usb_firmware(port).await,
        PicoAction::ReadUsbLog { port } => {
            cmd_debug::print_setup_port_log(port).await?;
            press_enter("Press Enter to return to Basic.").await
        }
        PicoAction::FlashBootsel { mount, board } => flash_bootsel_mount(mount, board).await,
        PicoAction::FindLastIp { identity, ip } => find_saved_pico_by_last_ip(identity, ip).await,
        PicoAction::SaveIdentity {
            identity,
            nickname_suggestion,
        } => {
            save_identity(identity.clone())?;
            prompt_nickname(identity.unique_id_short, nickname_suggestion).await
        }
        PicoAction::Rename {
            identity,
            nickname_suggestion,
        } => rename_pico(identity, nickname_suggestion).await,
        PicoAction::RemoveSaved { identity } => remove_saved_pico_identity(identity).await,
    }
}

pub(super) const IDENTIFY_BLINK_SECONDS: u8 = 10;

pub(super) async fn blink_pico_led(target: cmd_run::PicoTarget) -> Result<()> {
    outln!();
    outln!(
        "Asking {} to blink its onboard LED for {IDENTIFY_BLINK_SECONDS} seconds...",
        target.short_label()
    );
    match pico_mode::request_identify(&target, IDENTIFY_BLINK_SECONDS).await {
        Ok(true) => {
            outln!("Confirmed. Watch for the fast-blinking green LED on the Pico board.");
        }
        Ok(false) => {
            outln!("The Pico did not confirm the blink request.");
            outln!(
                "Its firmware likely predates the blink feature; run `Firmware update` and try again."
            );
        }
        Err(e) => outln!("Could not send the blink request: {e:#}"),
    }
    press_enter("Press Enter to return to Basic.").await
}

pub(super) async fn prompt_nickname(
    unique_id_short: u32,
    suggestion: Option<String>,
) -> Result<()> {
    let example = suggestion.as_deref().unwrap_or("Dreamcast");
    let name = input_text(&format!(
        "Name this Pico so it's easy to identify, e.g. {example} (blank to skip)"
    ))
    .await?;
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    set_saved_nickname(unique_id_short, Some(name.to_string()))
}

pub(super) async fn rename_pico(
    identity: config::PicoIdentity,
    suggestion: Option<String>,
) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    outln!();
    if let Some(current) = cfg.nickname_for(identity.unique_id_short) {
        outln!("Current name for {}: {current}", identity.uid_hex());
    } else {
        outln!("{} has no name yet.", identity.uid_hex());
    }
    let example = suggestion.as_deref().unwrap_or("Dreamcast");
    let name = input_text(&format!(
        "New name, e.g. {example} (blank to cancel, `-` to remove the name)"
    ))
    .await?;
    let name = name.trim();
    if name.is_empty() {
        outln!("Name unchanged.");
        return Ok(());
    }
    let value = (name != "-").then(|| name.to_string());
    set_saved_nickname(identity.unique_id_short, value)
}

pub(super) fn set_saved_nickname(unique_id_short: u32, nickname: Option<String>) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    let label = nickname.clone();
    if !cfg.set_nickname(unique_id_short, nickname) {
        outln!("This Pico is not in the saved list yet; choose `Save this Pico` first.");
        return Ok(());
    }
    config::save(&cfg)?;
    if let Some(name) = label {
        outln!("Saved name \"{name}\".");
    } else {
        outln!("Removed the saved name.");
    }
    Ok(())
}

pub(super) async fn update_setup_usb_firmware(port: String) -> Result<()> {
    let mounts = cmd_debug::switch_setup_port_to_bootsel_target(port).await?;
    match mounts.as_slice() {
        [(mount, board)] => flash_bootsel_mount(mount.clone(), *board).await,
        [] => {
            outln!("No new BOOTSEL drive appeared.");
            Ok(())
        }
        _ => {
            let choices: Vec<String> = mounts
                .iter()
                .map(|(mount, board)| format!("{} at {}", board.label(), mount.display()))
                .collect();
            let idx = select("Which BOOTSEL drive should be flashed?", &choices, 0).await?;
            let (mount, board) = mounts[idx].clone();
            flash_bootsel_mount(mount, board).await
        }
    }
}

pub(super) async fn flash_bootsel_mount(
    mount: PathBuf,
    board: cmd_flash::BootselBoard,
) -> Result<()> {
    outln!();
    outln!("Flashing {} at {}", board.label(), mount.display());
    let uf2_path =
        cmd_flash::resolve_uf2_path(None, board).context("resolving which UF2 to flash")?;
    outln!("Using firmware: {}", uf2_path.display());
    outln!("Copying {} -> {} ...", uf2_path.display(), mount.display());
    let outcome = cmd_flash::flash_uf2_to_mount(&uf2_path, mount, board).await?;
    if outcome.rebooted_during_copy {
        outln!(
            "Pico rebooted mid-write. Approximately {} bytes transferred before reboot.",
            outcome.bytes_written,
        );
    } else {
        outln!(
            "Wrote {} bytes. The Pico should now reboot into the new firmware.",
            outcome.bytes_written,
        );
    }
    Ok(())
}

pub(super) async fn find_saved_pico_by_last_ip(
    identity: config::PicoIdentity,
    ip: String,
) -> Result<()> {
    let Some(addr) = cmd_run::parse_ip_selector(&ip) else {
        outln!("Saved last IP is not valid: {ip}");
        return Ok(());
    };
    outln!(
        "Probing {addr}:{} for {}...",
        protocol::PORT,
        identity.uid_hex()
    );
    match cmd_run::probe_pico_ip(addr, Duration::from_secs(8)).await {
        Ok(pico) if pico.info.unique_id_short == identity.unique_id_short => {
            outln!("Found {}", pico.detail_label());
            save_identity(cmd_run::identity_from_target(&pico))?;
        }
        Ok(pico) => {
            outln!(
                "A Pico replied at {ip}, but it was {} instead of {}.",
                pico.uid_hex(),
                identity.uid_hex()
            );
            outln!("Refresh the Basic tab to see the connected Pico separately.");
        }
        Err(e) => outln!("No saved Pico replied at {ip}: {e:#}"),
    }
    Ok(())
}

pub(super) fn save_identity(identity: config::PicoIdentity) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    let label = saved_identity_label(&identity);
    cfg.remember_pico(identity);
    config::save(&cfg)?;
    outln!("Saved {label}.");
    Ok(())
}

pub(super) async fn remove_saved_pico_identity(identity: config::PicoIdentity) -> Result<()> {
    if !confirm(
        &format!("Remove {} {}?", identity.board_label(), identity.uid_hex()),
        false,
    )
    .await?
    {
        return Ok(());
    }
    let mut cfg = config::load().unwrap_or_default();
    cfg.forget_pico(identity.unique_id_short);
    config::save(&cfg)?;
    outln!("Removed saved Pico {}", identity.uid_hex());
    Ok(())
}

pub(super) fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub(super) fn refresh_saved_observations(
    cfg: &mut config::Config,
    inventory: &PicoInventory,
) -> bool {
    let saved_ids: HashSet<u32> = cfg.picos.iter().map(|p| p.unique_id_short).collect();
    let mut changed = false;
    for pico in &inventory.wifi {
        if !saved_ids.contains(&pico.info.unique_id_short) {
            continue;
        }
        let mut identity = cmd_run::identity_from_target(pico);
        let Some(saved) = cfg
            .picos
            .iter()
            .find(|p| p.unique_id_short == identity.unique_id_short)
        else {
            continue;
        };
        identity.nickname.clone_from(&saved.nickname);
        if saved != &identity {
            cfg.remember_pico(identity);
            changed = true;
        }
    }
    changed
}

pub(super) fn seed_saved_picos_from_legacy_last(cfg: &mut config::Config) -> bool {
    if !cfg.picos.is_empty() {
        return false;
    }
    let Some(last) = cfg.last_pico.clone() else {
        return false;
    };
    cfg.remember_pico(last);
    true
}

pub(super) async fn prompt_manual_pico_ip() -> Result<Option<cmd_run::PicoTarget>> {
    let text = input_text("Pico IP address (blank to cancel)").await?;
    let Some(ip) = cmd_run::parse_ip_selector(&text) else {
        if !text.trim().is_empty() {
            outln!("That does not look like an IP address: {}", text.trim());
        }
        return Ok(None);
    };
    outln!("Probing {ip}:4242 directly...");
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(5)).await {
        Ok(pico) => {
            outln!(
                "Pico replied at {}  fw v{}  uid 0x{:08X}",
                pico.peer,
                pico.info.firmware_version(),
                pico.info.unique_id_short,
            );
            outln!("Save this IP for manual routing: {}", pico.peer.ip());
            Ok(Some(pico))
        }
        Err(e) => {
            outln!("No Pico replied at {ip}: {e:#}");
            Ok(None)
        }
    }
}

pub(super) async fn add_new_pico() -> Result<()> {
    outln!();
    outln!("Add new Pico");
    outln!("Use this for a fresh Pico or a Pico you want to completely reinstall.");
    outln!("CouchLink will guide BOOTSEL flashing, Wi-Fi setup, discovery, and saving the Pico.");
    if !confirm("Start the full new-Pico setup now?", true).await? {
        return Ok(());
    }
    cmd_setup::run(None).await
}

pub(super) fn identity_from_usb_pico(pico: &SetupUsbPico) -> Option<config::PicoIdentity> {
    Some(config::PicoIdentity {
        unique_id_short: pico.unique_id_short?,
        board_type: pico.board_type,
        fw_major: pico.fw_major,
        fw_minor: pico.fw_minor,
        fw_patch: pico.fw_patch,
        last_ip: None,
        device_name: Some(setup_board_label(pico.board_type).to_string()),
        nickname: None,
    })
}

pub(super) fn saved_identity_label(pico: &config::PicoIdentity) -> String {
    let ip = pico
        .last_ip
        .as_ref()
        .map(|ip| format!(" last IP {ip}"))
        .unwrap_or_default();
    format!(
        "{} {} fw v{}{}",
        pico.board_label(),
        pico.uid_hex(),
        pico.firmware_version(),
        ip
    )
}

pub(super) fn setup_board_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W / WH",
        _ => "Pico",
    }
}
