//! Guided no-argument entrypoint. Direct subcommands remain available
//! for scripts, startup shortcuts, and third-party launchers.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::tui::{confirm, input_text, press_enter, select};
use crate::{
    cdc, cmd_auto, cmd_bundle, cmd_configure_wifi, cmd_debug, cmd_doctor, cmd_flash, cmd_logs,
    cmd_persona, cmd_run, cmd_setup, cmd_usb_diag, config, pico_mode, protocol, support, xinput,
};

const HOME_SCAN_SECONDS: u64 = 3;

pub async fn run() -> Result<()> {
    loop {
        print_header();
        match basic_tab().await? {
            BasicNav::Stay => {}
            BasicNav::Advanced => tools_menu().await?,
            BasicNav::Quit => return Ok(()),
        }
    }
}

fn print_header() {
    println!();
    println!("Parsec CouchLink");
    println!("Remote Parsec controllers -> Wi-Fi -> Pico -> console adapter");
    println!();
}

#[derive(Clone, Debug, Default)]
struct PicoInventory {
    wifi: Vec<cmd_run::PicoTarget>,
    usb: Vec<SetupUsbPico>,
    usb_errors: Vec<(String, String)>,
    bootsel: Vec<BootselPico>,
    scan_errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SetupUsbPico {
    port: String,
    unique_id_short: Option<u32>,
    board_type: u8,
    firmware: String,
    fw_major: u8,
    fw_minor: u8,
    fw_patch: u8,
    creds_present: bool,
    self_test_passed: bool,
    self_test_message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BootselPico {
    mount: PathBuf,
    board: cmd_flash::BootselBoard,
}

enum BasicNav {
    Stay,
    Advanced,
    Quit,
}

#[derive(Clone, Debug)]
struct PicoCard {
    title: String,
    status: String,
    details: Vec<String>,
    actions: Vec<PicoAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PicoAction {
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
    },
    RemoveSaved {
        identity: config::PicoIdentity,
    },
}

#[derive(Clone, Debug)]
enum BasicSelection {
    PicoAction(PicoAction),
    Refresh,
    AddNew,
    Advanced,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputModeChoice {
    Auto,
    Persona(protocol::Persona),
    Family(&'static [protocol::Persona]),
}

async fn scan_pico_inventory() -> Result<PicoInventory> {
    let mut inventory = PicoInventory::default();
    match cmd_run::discover_picos(Duration::from_secs(HOME_SCAN_SECONDS)).await {
        Ok(wifi) => inventory.wifi = wifi,
        Err(e) => inventory
            .scan_errors
            .push(format!("Wi-Fi discovery failed: {e:#}")),
    }
    match scan_setup_usb_picos().await {
        Ok((usb, usb_errors)) => {
            inventory.usb = usb;
            inventory.usb_errors = usb_errors;
        }
        Err(e) => inventory
            .scan_errors
            .push(format!("setup USB scan failed: {e:#}")),
    }
    inventory.bootsel = cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(mount, board)| BootselPico { mount, board })
        .collect();
    Ok(inventory)
}

async fn scan_setup_usb_picos() -> Result<(Vec<SetupUsbPico>, Vec<(String, String)>)> {
    let ports = cdc::find_setup_ports()?;
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for port in ports {
        match setup_usb_info(port.clone()).await {
            Ok(info) => found.push(info),
            Err(e) => errors.push((port, format!("{e:#}"))),
        }
    }
    Ok((found, errors))
}

async fn setup_usb_info(port: String) -> Result<SetupUsbPico> {
    tokio::task::spawn_blocking(move || -> Result<SetupUsbPico> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        let self_test = pico.self_test()?;
        let unique_id_short = pico.unique_id_short().ok();
        Ok(SetupUsbPico {
            port,
            unique_id_short,
            board_type: hello.board_type,
            firmware: hello.firmware_version().to_string(),
            fw_major: hello.fw_major,
            fw_minor: hello.fw_minor,
            fw_patch: hello.fw_patch,
            creds_present: hello.creds_present(),
            self_test_passed: self_test.passed,
            self_test_message: self_test.message,
        })
    })
    .await?
}

async fn basic_tab() -> Result<BasicNav> {
    let inventory = scan_pico_inventory().await?;
    let mut cfg = config::load().unwrap_or_default();
    let mut changed = seed_saved_picos_from_legacy_last(&mut cfg);
    if refresh_saved_observations(&mut cfg, &inventory) {
        changed = true;
    }
    if changed {
        config::save(&cfg)?;
    }

    let cards = build_pico_cards(&cfg, &inventory);
    print_basic_home(&cards, &inventory);

    let mut selections = Vec::new();
    let mut choices = Vec::new();
    for card in &cards {
        for action in &card.actions {
            choices.push(menu_item(
                &format!("{}: {}", card.title, action.label()),
                &action.help(),
            ));
            selections.push(BasicSelection::PicoAction(action.clone()));
        }
    }

    choices.push(menu_item("Refresh scan", "look for Pico boards again"));
    selections.push(BasicSelection::Refresh);
    choices.push(menu_item(
        "Add or reinstall a Pico",
        "flash firmware, set Wi-Fi, and save it",
    ));
    selections.push(BasicSelection::AddNew);
    choices.push(menu_item(
        "Advanced tab",
        "one-off diagnostics, fixes, logs, and command reference",
    ));
    selections.push(BasicSelection::Advanced);
    choices.push(menu_item("Quit", "close this menu"));
    selections.push(BasicSelection::Quit);

    let idx = select("Basic action", &choices, 0).await?;
    match selections[idx].clone() {
        BasicSelection::PicoAction(action) => {
            run_pico_action(action).await?;
            Ok(BasicNav::Stay)
        }
        BasicSelection::Refresh => Ok(BasicNav::Stay),
        BasicSelection::AddNew => {
            add_new_pico().await?;
            Ok(BasicNav::Stay)
        }
        BasicSelection::Advanced => Ok(BasicNav::Advanced),
        BasicSelection::Quit => Ok(BasicNav::Quit),
    }
}

fn print_basic_home(cards: &[PicoCard], inventory: &PicoInventory) {
    println!("Tabs: [Basic] Advanced");
    println!();
    println!("Basic");
    println!("Each Pico has its own commands. Basic actions target one Pico at a time.");
    println!();

    if cards.is_empty() {
        println!("No Pico was found on Wi-Fi, setup USB, or BOOTSEL.");
        println!("Choose `Add or reinstall a Pico` to start from scratch.");
    } else {
        for (idx, card) in cards.iter().enumerate() {
            println!("{}. {}", idx + 1, card.title);
            println!("   {}", card.status);
            for detail in &card.details {
                println!("   {detail}");
            }
            if card.actions.is_empty() {
                println!("   Commands: none available from Basic for this state.");
            } else {
                println!("   Commands:");
                for action in &card.actions {
                    println!("     - {} ({})", action.label(), action.target_hint());
                }
            }
            println!();
        }
    }

    if !inventory.usb_errors.is_empty() {
        println!("Setup USB devices needing attention:");
        for (port, error) in &inventory.usb_errors {
            println!("  {port}: {error}");
        }
        println!();
    }
    if !inventory.scan_errors.is_empty() {
        println!("Scan warnings:");
        for error in &inventory.scan_errors {
            println!("  {error}");
        }
        println!();
    }
}

fn build_pico_cards(cfg: &config::Config, inventory: &PicoInventory) -> Vec<PicoCard> {
    let saved_ids: HashSet<u32> = cfg.picos.iter().map(|p| p.unique_id_short).collect();
    let mut cards = Vec::new();

    for saved in &cfg.picos {
        if let Some(wifi) = inventory
            .wifi
            .iter()
            .find(|p| p.info.unique_id_short == saved.unique_id_short)
        {
            cards.push(wifi_card(wifi.clone(), true));
            continue;
        }
        if let Some(usb) = inventory
            .usb
            .iter()
            .find(|p| p.unique_id_short == Some(saved.unique_id_short))
        {
            cards.push(setup_usb_card(usb.clone(), true));
            continue;
        }
        cards.push(missing_saved_card(saved.clone()));
    }

    for pico in &inventory.wifi {
        if !saved_ids.contains(&pico.info.unique_id_short) {
            cards.push(wifi_card(pico.clone(), false));
        }
    }
    for pico in &inventory.usb {
        if pico
            .unique_id_short
            .map(|uid| saved_ids.contains(&uid))
            .unwrap_or(false)
        {
            continue;
        }
        cards.push(setup_usb_card(pico.clone(), false));
    }
    for pico in &inventory.bootsel {
        cards.push(bootsel_card(pico.clone()));
    }

    cards
}

fn wifi_card(pico: cmd_run::PicoTarget, saved: bool) -> PicoCard {
    let mut actions = vec![
        PicoAction::StartStreaming {
            target: pico.clone(),
        },
        PicoAction::ChooseRouting {
            target: pico.clone(),
        },
        PicoAction::CheckUsbAdapter {
            target: pico.clone(),
        },
        PicoAction::SwitchToUsbDebug {
            target: pico.clone(),
        },
    ];
    if !saved {
        actions.push(PicoAction::SaveIdentity {
            identity: cmd_run::identity_from_target(&pico),
        });
    }

    PicoCard {
        title: format!("{} {}", pico.board_label(), pico.uid_hex()),
        status: format!(
            "Wi-Fi ready at {} (fw v{})",
            pico.peer.ip(),
            pico.info.firmware_version()
        ),
        details: vec![format!(
            "{}",
            if saved {
                "Saved Pico"
            } else {
                "Connected but not saved"
            }
        )],
        actions,
    }
}

fn setup_usb_card(pico: SetupUsbPico, saved: bool) -> PicoCard {
    let uid = pico
        .unique_id_short
        .map(|uid| format!(" {uid:08X}"))
        .unwrap_or_default();
    let mut actions = Vec::new();
    if pico.creds_present {
        actions.push(PicoAction::RecoverToWifi {
            port: pico.port.clone(),
        });
    }
    actions.push(PicoAction::ConfigureWifi {
        port: pico.port.clone(),
    });
    actions.push(PicoAction::UpdateFirmwareFromSetupUsb {
        port: pico.port.clone(),
    });
    actions.push(PicoAction::ReadUsbLog {
        port: pico.port.clone(),
    });
    if !saved {
        if let Some(identity) = identity_from_usb_pico(&pico) {
            actions.push(PicoAction::SaveIdentity { identity });
        }
    }

    PicoCard {
        title: format!("{}{}", setup_board_label(pico.board_type), uid),
        status: format!("USB debug on {} (fw v{})", pico.port, pico.firmware),
        details: vec![
            format!("saved Wi-Fi: {}", yes_no(pico.creds_present)),
            format!(
                "SELF_TEST {} {}",
                if pico.self_test_passed {
                    "PASS"
                } else {
                    "FAIL"
                },
                pico.self_test_message
            ),
            if saved {
                "Saved Pico".to_string()
            } else {
                "Connected but not saved".to_string()
            },
        ],
        actions,
    }
}

fn bootsel_card(pico: BootselPico) -> PicoCard {
    PicoCard {
        title: pico.board.label().to_string(),
        status: format!("BOOTSEL firmware mode at {}", pico.mount.display()),
        details: vec!["Identity is not available in BOOTSEL mode.".to_string()],
        actions: vec![PicoAction::FlashBootsel {
            mount: pico.mount,
            board: pico.board,
        }],
    }
}

fn missing_saved_card(pico: config::PicoIdentity) -> PicoCard {
    let mut actions = Vec::new();
    if let Some(ip) = &pico.last_ip {
        actions.push(PicoAction::FindLastIp {
            identity: pico.clone(),
            ip: ip.clone(),
        });
    }
    actions.push(PicoAction::RemoveSaved {
        identity: pico.clone(),
    });
    let last_ip = pico
        .last_ip
        .as_ref()
        .map(|ip| format!("last IP {ip}"))
        .unwrap_or_else(|| "no last IP saved".to_string());
    PicoCard {
        title: format!("{} {}", pico.board_label(), pico.uid_hex()),
        status: format!(
            "Not seen right now ({last_ip}, fw v{})",
            pico.firmware_version()
        ),
        details: vec!["Saved Pico".to_string()],
        actions,
    }
}

impl PicoAction {
    fn label(&self) -> String {
        match self {
            Self::StartStreaming { .. } => "Start streaming with Controller 1".to_string(),
            Self::ChooseRouting { .. } => "Choose controller and stream".to_string(),
            Self::CheckUsbAdapter { .. } => "Check console USB adapter".to_string(),
            Self::SwitchToUsbDebug { .. } => "Switch to USB debug mode".to_string(),
            Self::RecoverToWifi { .. } => "Recover to Wi-Fi/input mode".to_string(),
            Self::ConfigureWifi { .. } => "Set up or change Wi-Fi".to_string(),
            Self::UpdateFirmwareFromSetupUsb { .. } => "Update firmware".to_string(),
            Self::ReadUsbLog { .. } => "Read USB debug log".to_string(),
            Self::FlashBootsel { .. } => "Flash or reinstall firmware".to_string(),
            Self::FindLastIp { ip, .. } => format!("Find at last IP {ip}"),
            Self::SaveIdentity { .. } => "Save this Pico".to_string(),
            Self::RemoveSaved { .. } => "Remove saved Pico".to_string(),
        }
    }

    fn help(&self) -> String {
        match self {
            Self::StartStreaming { .. } => "stream this Pico only".to_string(),
            Self::ChooseRouting { .. } => "pick one Windows controller for this Pico".to_string(),
            Self::CheckUsbAdapter { .. } => "query this Pico's USB adapter status".to_string(),
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
            Self::RemoveSaved { .. } => "forget this saved Pico and its routes".to_string(),
        }
    }

    fn target_hint(&self) -> String {
        match self {
            Self::StartStreaming { target }
            | Self::ChooseRouting { target }
            | Self::CheckUsbAdapter { target }
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
            Self::SaveIdentity { identity } | Self::RemoveSaved { identity } => {
                format!("target {}", identity.uid_hex())
            }
        }
    }
}

async fn run_pico_action(action: PicoAction) -> Result<()> {
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
        PicoAction::SaveIdentity { identity } => save_identity(identity),
        PicoAction::RemoveSaved { identity } => remove_saved_pico_identity(identity).await,
    }
}

async fn update_setup_usb_firmware(port: String) -> Result<()> {
    let mounts = cmd_debug::switch_setup_port_to_bootsel_target(port).await?;
    match mounts.as_slice() {
        [(mount, board)] => flash_bootsel_mount(mount.clone(), *board).await,
        [] => {
            println!("No new BOOTSEL drive appeared.");
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

async fn flash_bootsel_mount(mount: PathBuf, board: cmd_flash::BootselBoard) -> Result<()> {
    println!();
    println!("Flashing {} at {}", board.label(), mount.display());
    let uf2_path =
        cmd_flash::resolve_uf2_path(None, board).context("resolving which UF2 to flash")?;
    println!("Using firmware: {}", uf2_path.display());
    println!("Copying {} -> {} ...", uf2_path.display(), mount.display());
    let outcome = cmd_flash::flash_uf2_to_mount(&uf2_path, mount, board, 0).await?;
    if outcome.rebooted_during_copy {
        println!(
            "Pico rebooted mid-write. Approximately {} bytes transferred before reboot.",
            outcome.bytes_written,
        );
    } else {
        println!(
            "Wrote {} bytes. The Pico should now reboot into the new firmware.",
            outcome.bytes_written,
        );
    }
    Ok(())
}

async fn find_saved_pico_by_last_ip(identity: config::PicoIdentity, ip: String) -> Result<()> {
    let Some(addr) = cmd_run::parse_ip_selector(&ip) else {
        println!("Saved last IP is not valid: {ip}");
        return Ok(());
    };
    println!(
        "Probing {addr}:{} for {}...",
        protocol::PORT,
        identity.uid_hex()
    );
    match cmd_run::probe_pico_ip(addr, Duration::from_secs(8)).await {
        Ok(pico) if pico.info.unique_id_short == identity.unique_id_short => {
            println!("Found {}", pico.detail_label());
            save_identity(cmd_run::identity_from_target(&pico))?;
        }
        Ok(pico) => {
            println!(
                "A Pico replied at {ip}, but it was {} instead of {}.",
                pico.uid_hex(),
                identity.uid_hex()
            );
            println!("Refresh the Basic tab to see the connected Pico separately.");
        }
        Err(e) => println!("No saved Pico replied at {ip}: {e:#}"),
    }
    Ok(())
}

fn save_identity(identity: config::PicoIdentity) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    let label = saved_identity_label(&identity);
    cfg.remember_pico(identity);
    config::save(&cfg)?;
    println!("Saved {label}.");
    Ok(())
}

async fn remove_saved_pico_identity(identity: config::PicoIdentity) -> Result<()> {
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
    println!("Removed saved Pico {}", identity.uid_hex());
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn refresh_saved_observations(cfg: &mut config::Config, inventory: &PicoInventory) -> bool {
    let saved_ids: HashSet<u32> = cfg.picos.iter().map(|p| p.unique_id_short).collect();
    let mut changed = false;
    for pico in &inventory.wifi {
        if !saved_ids.contains(&pico.info.unique_id_short) {
            continue;
        }
        let identity = cmd_run::identity_from_target(pico);
        if cfg
            .picos
            .iter()
            .find(|p| p.unique_id_short == identity.unique_id_short)
            .map(|p| p != &identity)
            .unwrap_or(false)
        {
            cfg.remember_pico(identity);
            changed = true;
        }
    }
    changed
}

fn seed_saved_picos_from_legacy_last(cfg: &mut config::Config) -> bool {
    if !cfg.picos.is_empty() {
        return false;
    }
    let Some(last) = cfg.last_pico.clone() else {
        return false;
    };
    cfg.remember_pico(last);
    true
}

async fn prompt_manual_pico_ip() -> Result<Option<cmd_run::PicoTarget>> {
    let text = input_text("Pico IP address (blank to cancel)").await?;
    let Some(ip) = cmd_run::parse_ip_selector(&text) else {
        if !text.trim().is_empty() {
            println!("That does not look like an IP address: {}", text.trim());
        }
        return Ok(None);
    };
    println!("Probing {ip}:4242 directly...");
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(5)).await {
        Ok(pico) => {
            println!(
                "Pico replied at {}  fw v{}  uid 0x{:08X}",
                pico.peer,
                pico.info.firmware_version(),
                pico.info.unique_id_short,
            );
            println!("Save this IP for manual routing: {}", pico.peer.ip());
            Ok(Some(pico))
        }
        Err(e) => {
            println!("No Pico replied at {ip}: {e:#}");
            Ok(None)
        }
    }
}

async fn add_new_pico() -> Result<()> {
    println!();
    println!("Add new Pico");
    println!("Use this for a fresh Pico or a Pico you want to completely reinstall.");
    println!("CouchLink will guide BOOTSEL flashing, Wi-Fi setup, discovery, and saving the Pico.");
    if !confirm("Start the full new-Pico setup now?", true).await? {
        return Ok(());
    }
    cmd_setup::run(None).await
}

fn identity_from_usb_pico(pico: &SetupUsbPico) -> Option<config::PicoIdentity> {
    Some(config::PicoIdentity {
        unique_id_short: pico.unique_id_short?,
        board_type: pico.board_type,
        fw_major: pico.fw_major,
        fw_minor: pico.fw_minor,
        fw_patch: pico.fw_patch,
        last_ip: None,
        device_name: Some(setup_board_label(pico.board_type).to_string()),
    })
}

fn saved_identity_label(pico: &config::PicoIdentity) -> String {
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

fn setup_board_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W / WH",
        _ => "Pico",
    }
}

fn print_xinput_sources() {
    println!();
    println!("Source controllers Windows can currently see:");
    let slots = xinput::connected_slots();
    if slots.is_empty() {
        println!("  No XInput controllers are connected yet.");
        println!("  You can still route Controller 1-4; CouchLink will start sending neutral state until that slot appears.");
    } else {
        for slot in slots {
            println!(
                "  {} live  buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
                xinput::user_slot_label(slot.slot),
                slot.state.buttons,
                slot.state.left_trigger,
                slot.state.right_trigger,
                slot.state.left_x,
                slot.state.left_y,
                slot.state.right_x,
                slot.state.right_y,
            );
        }
    }
    println!("  If a Pico is plugged into this same PC only for testing, Windows may list it here as an Xbox controller.");
    println!("  In normal use, the Pico USB side should be plugged into the console adapter, not used as the source controller.");
}

async fn route_one(picos: Vec<cmd_run::PicoTarget>) -> Result<()> {
    let pico_items: Vec<String> = picos.iter().map(|p| p.detail_label()).collect();
    let pico_index = select("Which Pico should receive the controller?", &pico_items, 0).await?;
    let source_slot =
        choose_source_slot("Which Windows controller should feed that Pico?", None).await?;
    let routes = vec![cmd_run::StreamRoute {
        source_slot,
        pico: picos[pico_index].clone(),
    }];
    stream(routes, true).await
}

async fn choose_source_slot(prompt: &str, default_slot: Option<u32>) -> Result<u32> {
    let items = vec![slot_item(0), slot_item(1), slot_item(2), slot_item(3)];
    let default = default_slot.unwrap_or(0).min(3) as usize;
    Ok(select(prompt, &items, default).await? as u32)
}

fn slot_item(slot: u32) -> String {
    if xinput::read_slot(slot).is_some() {
        format!("{} (live)", xinput::user_slot_label(slot))
    } else {
        format!("{} (waiting)", xinput::user_slot_label(slot))
    }
}

async fn stream(routes: Vec<cmd_run::StreamRoute>, save: bool) -> Result<()> {
    let mode = choose_input_mode().await?;
    let routes = prepare_routes_for_input_mode(routes, mode).await?;
    println!();
    println!("Ready to stream:");
    for route in &routes {
        println!("  {}", route.label());
    }
    if !confirm("Start streaming now?", true).await? {
        return Ok(());
    }
    cmd_run::stream_routes(
        routes,
        cmd_run::StreamOptions {
            status_seconds: 2,
            quiet: false,
            save_routes: save,
        },
    )
    .await
}

async fn choose_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item(
            "Auto",
            "try gamepad USB modes until the adapter accepts reports",
        ),
        menu_item("Xbox", "choose Xbox 360 or Xbox One USB mode"),
        menu_item("DInput / PlayStation", "choose PS3 or PS4 HID mode"),
        menu_item(
            "Maple",
            "Xbox-compatible mode labelled for Dreamcast adapters",
        ),
        menu_item("Keyboard", "USB HID keyboard mode"),
        menu_item("Debug", "XInput mode with raw USB packet capture"),
    ];
    match select("Pico input mode", &choices, 0).await? {
        0 => Ok(InputModeChoice::Auto),
        1 => choose_xbox_input_mode().await,
        2 => choose_playstation_input_mode().await,
        3 => Ok(InputModeChoice::Persona(protocol::Persona::Maple)),
        4 => Ok(InputModeChoice::Persona(protocol::Persona::Keyboard)),
        _ => Ok(InputModeChoice::Persona(protocol::Persona::Debug)),
    }
}

async fn choose_xbox_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item("Auto Xbox", "try Xbox 360, then Xbox One"),
        menu_item("Xbox 360", "wired Xbox 360 / XInput USB mode"),
        menu_item("Xbox One", "Xbox One-compatible USB mode"),
    ];
    match select("Xbox input mode", &choices, 0).await? {
        0 => Ok(InputModeChoice::Family(cmd_auto::XBOX_FAMILY)),
        1 => Ok(InputModeChoice::Persona(protocol::Persona::Xinput)),
        _ => Ok(InputModeChoice::Persona(protocol::Persona::XboxOne)),
    }
}

async fn choose_playstation_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item("Auto DInput", "try PS3, then PS4"),
        menu_item("PS3", "DualShock 3 / PS3 HID mode"),
        menu_item("PS4", "DualShock 4 / PS4 HID mode"),
    ];
    match select("DInput input mode", &choices, 0).await? {
        0 => Ok(InputModeChoice::Family(cmd_auto::PLAYSTATION_FAMILY)),
        1 => Ok(InputModeChoice::Persona(protocol::Persona::Ps3)),
        _ => Ok(InputModeChoice::Persona(protocol::Persona::Ps4)),
    }
}

async fn prepare_routes_for_input_mode(
    routes: Vec<cmd_run::StreamRoute>,
    mode: InputModeChoice,
) -> Result<Vec<cmd_run::StreamRoute>> {
    match mode {
        InputModeChoice::Auto => {
            let targets: Vec<_> = routes.iter().map(|route| route.pico.clone()).collect();
            let ready = cmd_auto::select_gamepad_targets(targets).await?;
            let routes = replace_route_targets(routes, &ready);
            require_all_routes_in_targets(routes, &ready)
        }
        InputModeChoice::Family(candidates) => {
            let targets: Vec<_> = routes.iter().map(|route| route.pico.clone()).collect();
            let ready =
                cmd_auto::select_gamepad_targets_from_candidates(targets, candidates).await?;
            let routes = replace_route_targets(routes, &ready);
            require_all_routes_in_targets(routes, &ready)
        }
        InputModeChoice::Persona(persona) => switch_routes_to_persona(routes, persona).await,
    }
}

async fn switch_routes_to_persona(
    mut routes: Vec<cmd_run::StreamRoute>,
    desired: protocol::Persona,
) -> Result<Vec<cmd_run::StreamRoute>> {
    let mut switched_uids = Vec::new();
    for route in &routes {
        if route.pico.persona == desired {
            continue;
        }
        println!(
            "Switching {} to {} mode...",
            route.pico.short_label(),
            desired.label()
        );
        pico_mode::request_set_persona(&route.pico, desired).await?;
        switched_uids.push(route.pico.info.unique_id_short);
    }

    if !switched_uids.is_empty() {
        println!(
            "Waiting up to {}s for the Pico(s) to reboot into {} mode...",
            cmd_persona::REBOOT_WAIT.as_secs(),
            desired.label()
        );
        let reappeared =
            cmd_persona::wait_for_persona(&switched_uids, desired, cmd_persona::REBOOT_WAIT)
                .await?;
        routes = replace_route_targets(routes, &reappeared);
    }

    let pending: Vec<_> = routes
        .iter()
        .filter(|route| route.pico.persona != desired)
        .map(|route| route.pico.uid_hex())
        .collect();
    if !pending.is_empty() {
        anyhow::bail!(
            "{} did not confirm {} mode",
            pending.join(", "),
            desired.label()
        );
    }
    Ok(routes)
}

fn replace_route_targets(
    mut routes: Vec<cmd_run::StreamRoute>,
    targets: &[cmd_run::PicoTarget],
) -> Vec<cmd_run::StreamRoute> {
    for route in &mut routes {
        if let Some(target) = targets
            .iter()
            .find(|target| target.info.unique_id_short == route.pico.info.unique_id_short)
        {
            route.pico = target.clone();
        }
    }
    routes
}

fn require_all_routes_in_targets(
    routes: Vec<cmd_run::StreamRoute>,
    targets: &[cmd_run::PicoTarget],
) -> Result<Vec<cmd_run::StreamRoute>> {
    let missing: Vec<_> = routes
        .iter()
        .filter(|route| {
            !targets
                .iter()
                .any(|target| target.info.unique_id_short == route.pico.info.unique_id_short)
        })
        .map(|route| route.pico.uid_hex())
        .collect();
    if !missing.is_empty() {
        anyhow::bail!(
            "{} did not return in the selected input mode",
            missing.join(", ")
        );
    }
    Ok(routes)
}

async fn flash_menu() -> Result<()> {
    println!();
    println!("Firmware update");
    println!("Flashing does not require a controller. Controller routing is tested when streaming starts.");
    let choices = vec![
        menu_item(
            "Update firmware (recommended)",
            "tries USB no-button update, then guides BOOTSEL if needed",
        ),
        menu_item(
            "Advanced flash options",
            "choose setup USB, BOOTSEL, or full setup directly",
        ),
        menu_item("Back", "return to Advanced"),
    ];
    match select("Firmware update", &choices, 0).await? {
        0 => guided_firmware_update().await,
        1 => advanced_flash_menu().await,
        _ => Ok(()),
    }
}

async fn guided_firmware_update() -> Result<()> {
    println!();
    println!("CouchLink will update the Pico firmware.");
    println!("If the Pico is already in setup mode, this can happen without pressing BOOTSEL.");
    println!("If not, the app will walk you through BOOTSEL flashing.");
    println!();

    let result = cmd_flash::run(None, true, true).await;
    if let Err(e) = result {
        println!();
        println!("Automatic USB update did not complete:");
        println!("  {e:#}");
        println!();
        println!("Next step: put the Pico in BOOTSEL when prompted.");
        if !confirm("Continue with guided BOOTSEL flashing?", true).await? {
            return Ok(());
        }
        cmd_flash::run(None, true, false).await?;
    }

    println!();
    println!("Firmware update complete.");
    println!("If the Pico already had working Wi-Fi, keep it and start streaming.");
    println!("If this Pico is new or the Wi-Fi changed, set Wi-Fi before routing controllers.");
    if confirm("Do you need to set up or change Wi-Fi now?", false).await? {
        cmd_configure_wifi::run().await?;
    } else {
        println!("Next: return to Basic and choose this Pico's streaming command.");
    }
    Ok(())
}

async fn advanced_flash_menu() -> Result<()> {
    println!();
    println!("Advanced flash options");
    println!("Use these only when you already know which USB state the Pico is in.");
    let choices = vec![
        menu_item(
            "No-button update from setup-mode USB",
            "ask visible USB setup firmware to enter BOOTSEL",
        ),
        menu_item("Flash BOOTSEL drive", "copy firmware to RPI-RP2 or RP2350"),
        menu_item(
            "Run full first-time setup",
            "flash firmware, send Wi-Fi, and check discovery",
        ),
        menu_item("Back", "return to firmware update"),
    ];
    match select("Advanced flash path", &choices, 0).await? {
        0 => cmd_flash::run(None, true, true).await,
        1 => cmd_flash::run(None, true, false).await,
        2 => cmd_setup::run(None).await,
        _ => Ok(()),
    }
}

async fn tools_menu() -> Result<()> {
    loop {
        println!();
        println!("Tabs: Basic [Advanced]");
        println!();
        println!("Advanced");
        let choices = vec![
            menu_item(
                "Quick status dashboard",
                "show Pico state, controller sources, and next steps",
            ),
            menu_item(
                "Firmware update",
                "update from USB when possible, or guide BOOTSEL",
            ),
            menu_item(
                "Set up or change Wi-Fi",
                "send 2.4 GHz Wi-Fi credentials over USB",
            ),
            menu_item(
                "Run health check",
                "check paths, controllers, firewall, Wi-Fi, Pico",
            ),
            menu_item(
                "Pico debug and recovery",
                "see mode status and switch Wi-Fi/USB/BOOTSEL",
            ),
            menu_item(
                "Check Pico USB adapter",
                "ask the Pico whether the console adapter accepted its input mode",
            ),
            menu_item(
                "Auto recover for streaming",
                "move setup-mode USB Pico back to Wi-Fi when possible",
            ),
            menu_item(
                "Find Picos on Wi-Fi",
                "discover all Picos or probe one by manual IP",
            ),
            menu_item(
                "Check Windows controllers",
                "show which XInput controller slots are live",
            ),
            menu_item(
                "Create support bundle",
                "zip logs and diagnostics for a bug report",
            ),
            menu_item("Show log folder", "print where logs are stored"),
            menu_item("Follow live log", "tail the active log file"),
            menu_item(
                "Command reference",
                "copyable command lines for scripts and power users",
            ),
            menu_item("Basic tab", "return to the device-first view"),
        ];
        match select("Tool", &choices, 0).await? {
            0 => {
                quick_status_dashboard().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            1 => {
                flash_menu().await?;
            }
            2 => {
                cmd_configure_wifi::run().await?;
            }
            3 => {
                cmd_doctor::run_interactive().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            4 => {
                cmd_debug::run(cmd_debug::DebugOptions::default()).await?;
            }
            5 => {
                cmd_usb_diag::run_interactive().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            6 => {
                auto_recovery_tool().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            7 => {
                pico_finder_tool().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            8 => {
                controller_tool().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            9 => cmd_bundle::run(None).await?,
            10 => cmd_logs::run(false).await?,
            11 => cmd_logs::run(true).await?,
            12 => show_direct_commands().await?,
            _ => return Ok(()),
        }
    }
}

async fn quick_status_dashboard() -> Result<()> {
    println!();
    println!("Quick status dashboard");
    println!("This is the fastest way to decide the next tool to use.");
    cmd_debug::run(cmd_debug::DebugOptions {
        status: true,
        ..cmd_debug::DebugOptions::default()
    })
    .await?;
    print_xinput_sources();
    println!();
    println!("Suggested next steps:");
    println!("  If a Pico appears on Wi-Fi, return to Basic and choose its streaming or USB adapter command.");
    println!("  If a Pico appears in USB debug mode, choose `Set up or change Wi-Fi` or `Pico debug and recovery`.");
    println!("  If a Pico appears in BOOTSEL, choose `Firmware update`.");
    println!("  If no Pico appears anywhere, check the cable, then use BOOTSEL firmware mode.");
    Ok(())
}

async fn pico_finder_tool() -> Result<()> {
    loop {
        println!();
        println!("Find Picos on Wi-Fi");
        println!("This checks the same discovery path used by streaming.");
        let picos = cmd_run::discover_picos(Duration::from_secs(5)).await?;
        if picos.is_empty() {
            support::print_no_pico_wifi_help(5);
        } else {
            println!("Discovered Pico boards:");
            for pico in &picos {
                println!("  {}", pico.detail_label());
                println!("    manual IP: {}", pico.peer.ip());
            }
        }

        let choices = vec![
            menu_item("Scan again", "repeat Wi-Fi discovery"),
            menu_item("Probe manual IP", "test the IP shown in your router"),
            menu_item("Back", "return to Advanced"),
        ];
        match select("Wi-Fi finder", &choices, 0).await? {
            0 => continue,
            1 => {
                let _ = prompt_manual_pico_ip().await?;
            }
            _ => return Ok(()),
        }
    }
}

async fn auto_recovery_tool() -> Result<()> {
    println!();
    println!("Auto recover for streaming");
    println!("CouchLink will scan Wi-Fi first, then recover setup-mode USB Picos that already have saved Wi-Fi.");
    let picos = cmd_run::discover_picos_with_auto_recovery(Duration::from_secs(5), false).await?;
    if picos.is_empty() {
        println!("No Pico is ready for streaming yet.");
        println!("Next step: use `Set up or change Wi-Fi` if a Pico is in USB debug mode, or `Firmware update` if it is in BOOTSEL.");
    } else {
        println!("Ready Pico board(s):");
        for pico in picos {
            println!("  {}", pico.detail_label());
            println!("    manual IP: {}", pico.peer.ip());
        }
        println!("Next step: return to Basic and choose this Pico's streaming command.");
    }
    Ok(())
}

async fn controller_tool() -> Result<()> {
    println!();
    println!("Windows controller check");
    println!("This checks the source controllers that CouchLink reads from Windows.");
    print_xinput_sources();
    println!();
    match cmd_doctor::check_xinput().await {
        cmd_doctor::CheckResult::Pass(message) => println!("PASS  {message}"),
        cmd_doctor::CheckResult::Warn(message) => {
            println!("WARN  {message}");
            println!("Hint: start Parsec with a guest gamepad connected, or plug in a wired Xbox controller for bench testing.");
        }
        cmd_doctor::CheckResult::Skip(message) => println!("SKIP  {message}"),
        cmd_doctor::CheckResult::Fail(message, hint) => {
            println!("FAIL  {message}");
            println!("Hint: {hint}");
        }
    }
    Ok(())
}

async fn show_direct_commands() -> Result<()> {
    println!();
    println!("Advanced commands");
    println!("Use the guided menu for normal use. These commands stay available for scripts, shortcuts, and power users.");
    println!();
    println!("Guided setup");
    print_command("couchlink", "open this menu");
    print_command("couchlink setup", "first-time setup wizard");
    println!();
    println!("Streaming");
    print_command(
        "couchlink run",
        "stream using the saved layout, or one Pico if no layout is saved",
    );
    print_command(
        "couchlink run --all",
        "map Controller 1, 2, ... to every discovered Pico",
    );
    print_command(
        "couchlink run --route 1=UID",
        "route Controller 1 to a specific Pico UID",
    );
    print_command(
        "couchlink run --pico 192.168.50.4",
        "route to a Pico by manual IP",
    );
    println!();
    println!("Input modes");
    print_command(
        "couchlink auto",
        "try gamepad modes and keep the first one the adapter polls",
    );
    print_command("couchlink xinput", "switch to wired Xbox 360 USB mode");
    print_command("couchlink xbox", "try Xbox 360 and Xbox One modes");
    print_command("couchlink xbox360", "switch to wired Xbox 360 USB mode");
    print_command("couchlink xboxone", "switch to Xbox One USB mode");
    print_command("couchlink dinput", "try PS3 and PS4 DInput-family modes");
    print_command("couchlink ps3", "switch to PS3 HID mode");
    print_command("couchlink ps4", "switch to PS4 HID mode");
    print_command(
        "couchlink maple",
        "switch to Dreamcast adapter-labelled XInput mode",
    );
    print_command("couchlink keyboard", "switch to USB keyboard mode");
    print_command(
        "couchlink debug-input",
        "switch to XInput mode with raw USB packet capture",
    );
    println!();
    println!("Pico recovery");
    print_command(
        "couchlink recover",
        "auto-check Wi-Fi, setup USB, and BOOTSEL before streaming",
    );
    print_command("couchlink debug", "Pico USB/Wi-Fi recovery menu");
    print_command(
        "couchlink debug --status",
        "show USB debug, Wi-Fi, and BOOTSEL state",
    );
    print_command(
        "couchlink debug --to-usb-debug",
        "switch a Wi-Fi Pico to USB debug mode",
    );
    print_command(
        "couchlink debug --to-wifi --port COM3",
        "switch USB debug mode back to Wi-Fi/input mode",
    );
    print_command(
        "couchlink bootsel --port COM3",
        "switch USB debug mode to BOOTSEL mode",
    );
    print_command(
        "couchlink flash --from-usb --all",
        "no-button reflash for setup-mode Picos",
    );
    print_command("couchlink flash --all", "flash every BOOTSEL drive");
    println!();
    println!("Diagnostics");
    print_command(
        "couchlink test discover --all",
        "show every Pico answering on Wi-Fi",
    );
    print_command(
        "couchlink test discover --ip 192.168.50.4",
        "probe a Pico by manual IP",
    );
    print_command(
        "couchlink test usb --all",
        "check Pico USB host status over Wi-Fi",
    );
    print_command(
        "couchlink test usb --ip 192.168.50.4",
        "check one Pico by manual IP",
    );
    print_command(
        "couchlink test cdc --all",
        "show every setup-mode Pico over USB",
    );
    print_command("couchlink doctor", "run the full health check");
    print_command("couchlink bundle", "create a support bundle");
    print_command("couchlink logs --tail", "follow the live log");
    println!();
    println!("Detected Pico UID examples:");
    match cmd_run::discover_picos(Duration::from_secs(3)).await {
        Ok(picos) if !picos.is_empty() => {
            for pico in picos {
                println!("  {}  {}", pico.uid_hex(), pico.short_label());
            }
        }
        Ok(_) => println!("  No running Pico replied on Wi-Fi right now."),
        Err(e) => println!("  Discovery failed: {e:#}"),
    }
    press_enter("Press Enter to return to the menu.").await
}

fn menu_item(label: &str, help: &str) -> String {
    format!("{label:<36} {help}")
}

fn print_command(command: &str, help: &str) {
    println!("  {command:<42} {help}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol;

    fn pico(uid: u32, ip: &str, board: u8) -> cmd_run::PicoTarget {
        cmd_run::PicoTarget {
            peer: format!("{ip}:4242").parse().unwrap(),
            info: protocol::AckInfo {
                proto_version: protocol::PROTO_VERSION,
                fw_major: 26,
                fw_minor: 5,
                fw_patch: 30,
                board_type: board,
                uptime_seconds: 12,
                unique_id_short: uid,
                full_version: None,
            },
            persona: protocol::Persona::Xinput,
            ack_flags: 0,
        }
    }

    fn saved_pico(uid: u32, ip: Option<&str>) -> config::PicoIdentity {
        config::PicoIdentity {
            unique_id_short: uid,
            board_type: protocol::BOARD_PICO_2_W,
            fw_major: 26,
            fw_minor: 5,
            fw_patch: 30,
            last_ip: ip.map(|s| s.to_string()),
            device_name: None,
        }
    }

    fn setup_usb(port: &str, uid: Option<u32>, creds_present: bool) -> SetupUsbPico {
        SetupUsbPico {
            port: port.to_string(),
            unique_id_short: uid,
            board_type: protocol::BOARD_PICO_2_W,
            firmware: "2026.5.30.9-E56A".to_string(),
            fw_major: 26,
            fw_minor: 5,
            fw_patch: 30,
            creds_present,
            self_test_passed: true,
            self_test_message: "pass".to_string(),
        }
    }

    #[test]
    fn basic_cards_prefer_live_wifi_for_saved_pico() {
        let cfg = config::Config {
            picos: vec![saved_pico(0x07D37EB6, Some("192.168.50.1"))],
            ..config::Config::default()
        };
        let inventory = PicoInventory {
            wifi: vec![pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W)],
            ..PicoInventory::default()
        };

        let cards = build_pico_cards(&cfg, &inventory);

        assert_eq!(cards.len(), 1);
        assert!(cards[0].status.contains("Wi-Fi ready at 192.168.50.226"));
        assert!(cards[0].actions.iter().any(|action| matches!(
            action,
            PicoAction::StartStreaming { target }
                if target.info.unique_id_short == 0x07D37EB6
        )));
        assert!(!cards[0]
            .actions
            .iter()
            .any(|action| matches!(action, PicoAction::SaveIdentity { .. })));
    }

    #[test]
    fn basic_cards_expose_setup_usb_and_bootsel_targets() {
        let cfg = config::Config::default();
        let inventory = PicoInventory {
            usb: vec![
                setup_usb("COM4", Some(0x07D37EB6), true),
                setup_usb("COM5", Some(0x523861E6), false),
            ],
            bootsel: vec![BootselPico {
                mount: PathBuf::from("I:\\"),
                board: cmd_flash::BootselBoard::Rp2040,
            }],
            ..PicoInventory::default()
        };

        let cards = build_pico_cards(&cfg, &inventory);

        let com4 = cards
            .iter()
            .find(|card| card.status.contains("COM4"))
            .expect("COM4 card");
        assert!(com4.actions.iter().any(|action| {
            matches!(action, PicoAction::RecoverToWifi { port } if port == "COM4")
        }));
        assert!(com4.actions.iter().any(|action| {
            matches!(action, PicoAction::UpdateFirmwareFromSetupUsb { port } if port == "COM4")
        }));

        let com5 = cards
            .iter()
            .find(|card| card.status.contains("COM5"))
            .expect("COM5 card");
        assert!(!com5
            .actions
            .iter()
            .any(|action| matches!(action, PicoAction::RecoverToWifi { .. })));
        assert!(com5.actions.iter().any(|action| {
            matches!(action, PicoAction::ConfigureWifi { port } if port == "COM5")
        }));

        assert!(cards.iter().any(|card| {
            card.actions.iter().any(|action| {
                matches!(
                    action,
                    PicoAction::FlashBootsel { mount, board }
                        if mount == &PathBuf::from("I:\\")
                            && *board == cmd_flash::BootselBoard::Rp2040
                )
            })
        }));
    }

    #[test]
    fn basic_cards_keep_multiple_wifi_picos_targeted() {
        let cfg = config::Config::default();
        let inventory = PicoInventory {
            wifi: vec![
                pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
                pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
            ],
            ..PicoInventory::default()
        };

        let cards = build_pico_cards(&cfg, &inventory);
        let streaming_targets: Vec<u32> = cards
            .iter()
            .flat_map(|card| &card.actions)
            .filter_map(|action| match action {
                PicoAction::StartStreaming { target } => Some(target.info.unique_id_short),
                _ => None,
            })
            .collect();

        assert_eq!(streaming_targets, vec![0x07D37EB6, 0x523861E6]);
        assert!(cards.iter().all(|card| {
            card.actions.iter().all(|action| {
                !matches!(
                    action,
                    PicoAction::RecoverToWifi { port } if port.eq_ignore_ascii_case("all")
                )
            })
        }));
    }

    #[test]
    fn missing_saved_pico_actions_are_targeted_to_saved_identity() {
        let cfg = config::Config {
            picos: vec![saved_pico(0x07D37EB6, Some("192.168.50.226"))],
            ..config::Config::default()
        };

        let cards = build_pico_cards(&cfg, &PicoInventory::default());

        assert_eq!(cards.len(), 1);
        assert!(cards[0].actions.iter().any(|action| matches!(
            action,
            PicoAction::FindLastIp { identity, ip }
                if identity.unique_id_short == 0x07D37EB6 && ip == "192.168.50.226"
        )));
        assert!(cards[0].actions.iter().any(|action| matches!(
            action,
            PicoAction::RemoveSaved { identity }
                if identity.unique_id_short == 0x07D37EB6
        )));
    }

    #[test]
    fn seed_saved_picos_migrates_legacy_last_pico_once() {
        let mut cfg = config::Config {
            last_pico: Some(saved_pico(0x07D37EB6, None)),
            ..config::Config::default()
        };

        assert!(seed_saved_picos_from_legacy_last(&mut cfg));
        assert_eq!(cfg.picos.len(), 1);
        assert!(!seed_saved_picos_from_legacy_last(&mut cfg));
        assert_eq!(cfg.picos.len(), 1);
    }
}
