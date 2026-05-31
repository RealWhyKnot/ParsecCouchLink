//! Guided no-argument entrypoint. Direct subcommands remain available
//! for scripts, startup shortcuts, and third-party launchers.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};

use crate::{
    cdc, cmd_bundle, cmd_configure_wifi, cmd_debug, cmd_doctor, cmd_flash, cmd_logs, cmd_run,
    cmd_setup, cmd_usb_diag, config, protocol, support, xinput,
};

const HOME_SCAN_SECONDS: u64 = 3;

pub async fn run() -> Result<()> {
    loop {
        print_header();
        let inventory = scan_pico_inventory().await?;
        let mut cfg = config::load().unwrap_or_default();
        let mut changed = seed_saved_picos_from_legacy_last(&mut cfg);
        if refresh_saved_observations(&mut cfg, &inventory) {
            changed = true;
        }
        if changed {
            config::save(&cfg)?;
        }
        print_simple_home(&cfg, &inventory);

        let choices = vec![
            menu_item(
                "Start streaming",
                "use saved routing or pick a controller layout",
            ),
            menu_item("Add new Pico", "flash firmware, set Wi-Fi, and save it"),
            menu_item(
                "Add existing Pico",
                "scan Wi-Fi, USB, and BOOTSEL to save a Pico",
            ),
            menu_item(
                "Manage saved Picos",
                "view saved devices, Wi-Fi setup, or remove one",
            ),
            menu_item(
                "Advanced tools",
                "diagnostics, recovery, firmware update, logs",
            ),
            menu_item(
                "Advanced commands",
                "copyable commands for scripts and power users",
            ),
            menu_item("Quit", "close this menu"),
        ];
        match select("Simple mode", &choices, 0).await? {
            0 => start_routing().await?,
            1 => add_new_pico().await?,
            2 => add_existing_pico().await?,
            3 => manage_saved_picos().await?,
            4 => tools_menu().await?,
            5 => show_direct_commands().await?,
            _ => return Ok(()),
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
    bootsel: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
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

async fn scan_pico_inventory() -> Result<PicoInventory> {
    let wifi = cmd_run::discover_picos(Duration::from_secs(HOME_SCAN_SECONDS)).await?;
    let (usb, usb_errors) = scan_setup_usb_picos().await?;
    let bootsel = cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(path, board)| (path.display().to_string(), board.label().to_string()))
        .collect();
    Ok(PicoInventory {
        wifi,
        usb,
        usb_errors,
        bootsel,
    })
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

fn print_simple_home(cfg: &config::Config, inventory: &PicoInventory) {
    println!("Simple mode");
    println!("This view treats each Pico like a saved device.");
    println!();

    if cfg.picos.is_empty() {
        println!("My Picos");
        println!("  No saved Pico yet. Choose `Add new Pico` or `Add existing Pico`.");
    } else {
        println!("My Picos");
        for pico in &cfg.picos {
            println!("  {}", saved_pico_status_line(pico, inventory));
        }
    }

    let unsaved = unsaved_connected_summary(cfg, inventory);
    if !unsaved.is_empty() {
        println!();
        println!("Connected but not saved");
        for line in unsaved {
            println!("  {line}");
        }
    }
    println!();
}

fn saved_pico_status_line(pico: &config::PicoIdentity, inventory: &PicoInventory) -> String {
    if let Some(wifi) = inventory
        .wifi
        .iter()
        .find(|p| p.info.unique_id_short == pico.unique_id_short)
    {
        return format!(
            "{} {} - Wi-Fi ready at {} (fw v{})",
            wifi.board_label(),
            wifi.uid_hex(),
            wifi.peer.ip(),
            wifi.info.firmware_version()
        );
    }
    if let Some(usb) = inventory
        .usb
        .iter()
        .find(|p| p.unique_id_short == Some(pico.unique_id_short))
    {
        let creds = if usb.creds_present {
            "saved Wi-Fi"
        } else {
            "no saved Wi-Fi"
        };
        return format!(
            "{} {} - USB debug on {} ({creds}, fw v{})",
            pico.board_label(),
            pico.uid_hex(),
            usb.port,
            usb.firmware
        );
    }
    if let Some(ip) = &pico.last_ip {
        format!(
            "{} {} - not seen right now (last IP {ip}, fw v{})",
            pico.board_label(),
            pico.uid_hex(),
            pico.firmware_version()
        )
    } else {
        format!(
            "{} {} - not seen right now (fw v{})",
            pico.board_label(),
            pico.uid_hex(),
            pico.firmware_version()
        )
    }
}

fn unsaved_connected_summary(cfg: &config::Config, inventory: &PicoInventory) -> Vec<String> {
    let saved_ids: HashSet<u32> = cfg.picos.iter().map(|p| p.unique_id_short).collect();
    let mut lines = Vec::new();
    for pico in &inventory.wifi {
        if !saved_ids.contains(&pico.info.unique_id_short) {
            lines.push(format!(
                "{} {} - Wi-Fi ready at {}",
                pico.board_label(),
                pico.uid_hex(),
                pico.peer.ip()
            ));
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
        lines.push(format!(
            "{} - USB debug on {} ({}, SELF_TEST {})",
            setup_board_label(pico.board_type),
            pico.port,
            if pico.creds_present {
                "saved Wi-Fi"
            } else {
                "no saved Wi-Fi"
            },
            if pico.self_test_passed {
                "PASS"
            } else {
                "FAIL"
            }
        ));
    }
    for (port, error) in &inventory.usb_errors {
        lines.push(format!(
            "{port} - setup USB detected, but status failed: {error}"
        ));
    }
    for (mount, board) in &inventory.bootsel {
        lines.push(format!("{board} - BOOTSEL drive at {mount}"));
    }
    lines
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

async fn start_routing() -> Result<()> {
    loop {
        println!("Looking for running Pico boards on Wi-Fi...");
        let mut picos =
            cmd_run::discover_picos_with_auto_recovery(Duration::from_secs(5), false).await?;
        if picos.is_empty() {
            support::print_no_pico_wifi_help(5);
            let choices = vec![
                menu_item("Try discovery again", "repeat Wi-Fi broadcast discovery"),
                menu_item("Enter Pico IP manually", "use the IP shown by your router"),
                menu_item("Set up or change Wi-Fi", "re-send Wi-Fi credentials"),
                menu_item("Advanced tools", "open status, checks, and recovery"),
                menu_item("Update firmware", "flash the Pico firmware"),
                menu_item("Back", "return to the main menu"),
            ];
            match select("Next step", &choices, 0).await? {
                0 => continue,
                1 => {
                    let Some(pico) = prompt_manual_pico_ip().await? else {
                        continue;
                    };
                    picos.push(pico);
                }
                2 => cmd_configure_wifi::run().await?,
                3 => tools_menu().await?,
                4 => flash_menu().await?,
                _ => return Ok(()),
            }
        }

        println!();
        println!("Detected Pico boards:");
        for (idx, pico) in picos.iter().enumerate() {
            println!("  {}. {}", idx + 1, pico.detail_label());
        }

        print_xinput_sources();

        let saved = config::load().unwrap_or_default().routes;
        let recommended = recommended_routes(&picos, &saved);
        let choices = vec![
            menu_item(
                &recommended_route_label(&recommended),
                "recommended for most sessions",
            ),
            menu_item(
                "Change controller routing",
                "choose which controller feeds which Pico",
            ),
            menu_item("Back", "return to the main menu"),
        ];
        match select("Streaming setup", &choices, 0).await? {
            0 => match recommended {
                Ok(routes) => return stream(routes, true).await,
                Err(e) => {
                    println!();
                    println!("CouchLink needs a routing choice before streaming:");
                    println!("  {e:#}");
                    return routing_options(picos).await;
                }
            },
            1 => return routing_options(picos).await,
            _ => return Ok(()),
        }
    }
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

async fn add_existing_pico() -> Result<()> {
    loop {
        println!();
        println!("Add existing Pico");
        println!("Scanning Wi-Fi, setup USB, and BOOTSEL...");
        let inventory = scan_pico_inventory().await?;
        print_existing_scan(&inventory);

        let saveable = saveable_identities(&inventory);
        let has_recoverable_usb = inventory.usb.iter().any(|p| p.creds_present);
        let choices = vec![
            menu_item(
                "Save detected Pico info",
                "save Wi-Fi or setup-USB Picos that can identify themselves",
            ),
            menu_item(
                "Recover USB Pico to Wi-Fi",
                "switch setup USB Picos with saved Wi-Fi back to Wi-Fi, then save them",
            ),
            menu_item("Scan again", "repeat Wi-Fi, USB, and BOOTSEL checks"),
            menu_item(
                "Set up Wi-Fi over USB",
                "send or replace 2.4 GHz Wi-Fi credentials",
            ),
            menu_item("Back", "return to simple mode"),
        ];
        let default = if saveable.is_empty() && has_recoverable_usb {
            1
        } else {
            0
        };
        match select("Existing Pico action", &choices, default).await? {
            0 => {
                if saveable.is_empty() {
                    println!("No identifiable Pico was found to save.");
                    println!(
                        "If it is in BOOTSEL, choose `Add new Pico` to install firmware first."
                    );
                    continue;
                }
                save_detected_identities(saveable).await?;
            }
            1 => {
                if !has_recoverable_usb {
                    println!("No setup-mode USB Pico with saved Wi-Fi was found.");
                    continue;
                }
                let picos =
                    cmd_run::discover_picos_with_auto_recovery(Duration::from_secs(3), false)
                        .await?;
                if picos.is_empty() {
                    println!("No Pico replied on Wi-Fi after recovery.");
                } else {
                    save_wifi_picos(&picos)?;
                    println!("Saved recovered Pico board(s):");
                    for pico in picos {
                        println!("  {}", pico.detail_label());
                    }
                }
            }
            2 => continue,
            3 => cmd_configure_wifi::run().await?,
            _ => return Ok(()),
        }
    }
}

fn print_existing_scan(inventory: &PicoInventory) {
    println!();
    if inventory.wifi.is_empty()
        && inventory.usb.is_empty()
        && inventory.usb_errors.is_empty()
        && inventory.bootsel.is_empty()
    {
        println!("No Pico was found on Wi-Fi, setup USB, or BOOTSEL.");
        return;
    }

    if !inventory.wifi.is_empty() {
        println!("Wi-Fi/controller mode:");
        for pico in &inventory.wifi {
            println!("  {}", pico.detail_label());
            println!("    manual IP: {}", pico.peer.ip());
        }
    }
    if !inventory.usb.is_empty() {
        println!("Setup USB/debug mode:");
        for pico in &inventory.usb {
            let uid = pico
                .unique_id_short
                .map(|uid| format!(" UID {uid:08X}"))
                .unwrap_or_else(|| " UID unavailable".to_string());
            println!(
                "  {}{} on {} fw v{} saved Wi-Fi: {} SELF_TEST {}",
                setup_board_label(pico.board_type),
                uid,
                pico.port,
                pico.firmware,
                if pico.creds_present { "yes" } else { "no" },
                if pico.self_test_passed {
                    "PASS"
                } else {
                    "FAIL"
                }
            );
            if !pico.self_test_message.trim().is_empty() {
                println!("    {}", pico.self_test_message);
            }
        }
    }
    if !inventory.usb_errors.is_empty() {
        println!("Setup USB with errors:");
        for (port, error) in &inventory.usb_errors {
            println!("  {port}: {error}");
        }
    }
    if !inventory.bootsel.is_empty() {
        println!("BOOTSEL firmware mode:");
        for (mount, board) in &inventory.bootsel {
            println!("  {board} at {mount}");
        }
    }
}

fn saveable_identities(inventory: &PicoInventory) -> Vec<config::PicoIdentity> {
    let mut identities = Vec::new();
    for pico in &inventory.wifi {
        identities.push(cmd_run::identity_from_target(pico));
    }
    for pico in &inventory.usb {
        if let Some(identity) = identity_from_usb_pico(pico) {
            if !identities
                .iter()
                .any(|p: &config::PicoIdentity| p.unique_id_short == identity.unique_id_short)
            {
                identities.push(identity);
            }
        }
    }
    identities
}

async fn save_detected_identities(identities: Vec<config::PicoIdentity>) -> Result<()> {
    let labels: Vec<String> = identities.iter().map(saved_identity_label).collect();
    let selected = if identities.len() == 1 {
        if confirm(&format!("Save {}?", labels[0]), true).await? {
            vec![0]
        } else {
            Vec::new()
        }
    } else {
        multiselect(
            "Which Picos should be saved?",
            &labels,
            &vec![true; identities.len()],
        )
        .await?
    };
    if selected.is_empty() {
        println!("No Pico saved.");
        return Ok(());
    }

    let mut cfg = config::load().unwrap_or_default();
    for idx in selected {
        cfg.remember_pico(identities[idx].clone());
    }
    config::save(&cfg)?;
    println!("Saved Pico inventory updated.");
    Ok(())
}

fn save_wifi_picos(picos: &[cmd_run::PicoTarget]) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    for pico in picos {
        cfg.remember_pico(cmd_run::identity_from_target(pico));
    }
    config::save(&cfg)
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

async fn manage_saved_picos() -> Result<()> {
    loop {
        let cfg = config::load().unwrap_or_default();
        println!();
        println!("Manage saved Picos");
        if cfg.picos.is_empty() {
            println!("No saved Pico yet.");
        } else {
            for pico in &cfg.picos {
                println!("  {}", saved_identity_label(pico));
            }
        }

        let choices = vec![
            menu_item("Add existing Pico", "scan Wi-Fi, USB, and BOOTSEL"),
            menu_item("Add new Pico", "flash firmware and run full setup"),
            menu_item(
                "Set up or change Wi-Fi",
                "send 2.4 GHz Wi-Fi credentials over USB",
            ),
            menu_item("Remove saved Pico", "remove a saved device and its routes"),
            menu_item("Back", "return to simple mode"),
        ];
        match select("Saved Pico action", &choices, 0).await? {
            0 => add_existing_pico().await?,
            1 => add_new_pico().await?,
            2 => cmd_configure_wifi::run().await?,
            3 => remove_saved_pico().await?,
            _ => return Ok(()),
        }
    }
}

async fn remove_saved_pico() -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    if cfg.picos.is_empty() {
        println!("No saved Pico to remove.");
        return Ok(());
    }
    let labels: Vec<String> = cfg.picos.iter().map(saved_identity_label).collect();
    let idx = select("Remove which saved Pico?", &labels, 0).await?;
    let pico = cfg.picos[idx].clone();
    if !confirm(
        &format!("Remove {} {}?", pico.board_label(), pico.uid_hex()),
        false,
    )
    .await?
    {
        return Ok(());
    }
    cfg.forget_pico(pico.unique_id_short);
    config::save(&cfg)?;
    println!("Removed saved Pico {}", pico.uid_hex());
    Ok(())
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

async fn routing_options(picos: Vec<cmd_run::PicoTarget>) -> Result<()> {
    println!();
    println!("Controller routing");
    println!("Most players should use the recommended streaming option. Change routing only when you need a specific layout.");
    let choices = vec![
        menu_item(
            "Use one controller",
            "send one Windows controller to one Pico",
        ),
        menu_item(
            "Use one controller per Pico",
            "Controller 1 -> Pico 1, Controller 2 -> Pico 2",
        ),
        menu_item(
            "Choose each controller manually",
            "build a custom controller-to-Pico layout",
        ),
        menu_item("Back", "return to streaming setup"),
    ];
    match select("Routing option", &choices, 0).await? {
        0 => route_one(picos).await,
        1 => {
            let routes = cmd_run::auto_routes(picos, Some((0..4).collect()))?;
            stream(routes, true).await
        }
        2 => route_custom(picos).await,
        _ => Ok(()),
    }
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

fn recommended_routes(
    picos: &[cmd_run::PicoTarget],
    saved: &[config::RouteConfig],
) -> Result<Vec<cmd_run::StreamRoute>> {
    if !saved.is_empty() {
        let routes = saved
            .iter()
            .map(|route| {
                let selector = format!("{:08X}", route.pico_uid);
                let pico = cmd_run::match_pico_selector(&selector, picos)?;
                Ok(cmd_run::StreamRoute {
                    source_slot: route.source_slot,
                    pico,
                })
            })
            .collect::<Result<Vec<_>>>();
        if routes.is_ok() {
            return routes;
        }
    }
    if picos.len() == 1 {
        return Ok(vec![cmd_run::StreamRoute {
            source_slot: 0,
            pico: picos[0].clone(),
        }]);
    }
    cmd_run::auto_routes(picos.to_vec(), Some((0..4).collect()))
}

fn recommended_route_label(routes: &Result<Vec<cmd_run::StreamRoute>>) -> String {
    match routes {
        Ok(routes) if routes.len() == 1 => {
            format!("Start streaming ({})", routes[0].label())
        }
        Ok(routes) => format!("Start streaming ({} Pico routes)", routes.len()),
        Err(_) => "Start streaming (needs routing fix)".to_string(),
    }
}

async fn route_custom(picos: Vec<cmd_run::PicoTarget>) -> Result<()> {
    let pico_items: Vec<String> = picos.iter().map(|p| p.detail_label()).collect();
    let selected = multiselect(
        "Which Picos should receive controller input?",
        &pico_items,
        &vec![true; pico_items.len()],
    )
    .await?;
    if selected.is_empty() {
        println!("No Pico selected.");
        return Ok(());
    }

    let mut routes = Vec::new();
    for idx in selected {
        let pico = picos[idx].clone();
        let prompt = format!("Source controller for {}", pico.short_label());
        let source_slot = choose_source_slot(&prompt, Some(routes.len() as u32)).await?;
        routes.push(cmd_run::StreamRoute { source_slot, pico });
    }
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
        menu_item("Back", "return to the main menu"),
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
        println!("Next: choose `Start streaming` from the main menu.");
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
        println!("Advanced tools");
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
                "ask the Pico whether the console adapter accepted XInput",
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
            menu_item("Back", "return to the main menu"),
        ];
        match select("Tool", &choices, 0).await? {
            0 => {
                quick_status_dashboard().await?;
                press_enter("Press Enter to return to tools.").await?;
            }
            1 => {
                flash_menu().await?;
            }
            2 => {
                cmd_configure_wifi::run().await?;
            }
            3 => {
                cmd_doctor::run_interactive().await?;
                press_enter("Press Enter to return to tools.").await?;
            }
            4 => {
                cmd_debug::run(cmd_debug::DebugOptions::default()).await?;
            }
            5 => {
                cmd_usb_diag::run_interactive().await?;
                press_enter("Press Enter to return to tools.").await?;
            }
            6 => {
                auto_recovery_tool().await?;
                press_enter("Press Enter to return to tools.").await?;
            }
            7 => {
                pico_finder_tool().await?;
                press_enter("Press Enter to return to tools.").await?;
            }
            8 => {
                controller_tool().await?;
                press_enter("Press Enter to return to tools.").await?;
            }
            9 => cmd_bundle::run(None).await?,
            10 => cmd_logs::run(false).await?,
            11 => cmd_logs::run(true).await?,
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
    println!("  If a Pico appears on Wi-Fi, choose `Start streaming` or `Check Pico USB adapter`.");
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
            menu_item("Back", "return to tools"),
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
        println!("Next step: choose `Start streaming`.");
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
        "switch USB debug mode back to Wi-Fi mode",
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
        "check Pico USB/XInput host status over Wi-Fi",
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

async fn input_text(prompt: &str) -> Result<String> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()
    })
    .await?
    .context("reading input")
}

fn menu_item(label: &str, help: &str) -> String {
    format!("{label:<36} {help}")
}

fn print_command(command: &str, help: &str) {
    println!("  {command:<42} {help}");
}

async fn select(prompt: &str, items: &[impl ToString], default: usize) -> Result<usize> {
    let prompt = prompt.to_string();
    let items: Vec<String> = items.iter().map(ToString::to_string).collect();
    tokio::task::spawn_blocking(move || {
        Select::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(&items)
            .default(default.min(items.len().saturating_sub(1)))
            .interact()
    })
    .await?
    .context("reading menu selection")
}

async fn multiselect(prompt: &str, items: &[String], defaults: &[bool]) -> Result<Vec<usize>> {
    let prompt = prompt.to_string();
    let items = items.to_vec();
    let defaults = defaults.to_vec();
    tokio::task::spawn_blocking(move || {
        MultiSelect::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(&items)
            .defaults(&defaults)
            .interact()
    })
    .await?
    .context("reading menu selection")
}

async fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(default)
            .interact()
    })
    .await?
    .context("reading confirmation")
}

async fn press_enter(prompt: &str) -> Result<()> {
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
    .await?
    .context("waiting for Enter")?;
    Ok(())
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
            },
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

    #[test]
    fn saved_pico_status_prefers_live_wifi_state() {
        let saved = saved_pico(0x07D37EB6, Some("192.168.50.1"));
        let inventory = PicoInventory {
            wifi: vec![pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W)],
            ..PicoInventory::default()
        };

        let line = saved_pico_status_line(&saved, &inventory);

        assert!(line.contains("Wi-Fi ready at 192.168.50.226"));
    }

    #[test]
    fn unsaved_connected_summary_reports_usb_and_bootsel() {
        let cfg = config::Config::default();
        let inventory = PicoInventory {
            usb: vec![SetupUsbPico {
                port: "COM4".to_string(),
                unique_id_short: Some(0x07D37EB6),
                board_type: protocol::BOARD_PICO_2_W,
                firmware: "2026.5.30.9-E56A".to_string(),
                fw_major: 26,
                fw_minor: 5,
                fw_patch: 30,
                creds_present: true,
                self_test_passed: true,
                self_test_message: "pass".to_string(),
            }],
            bootsel: vec![("I:\\".to_string(), "Pico W / WH (RP2040)".to_string())],
            ..PicoInventory::default()
        };

        let lines = unsaved_connected_summary(&cfg, &inventory);

        assert!(lines.iter().any(|line| line.contains("USB debug on COM4")));
        assert!(lines.iter().any(|line| line.contains("BOOTSEL drive")));
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

    #[test]
    fn recommended_routes_uses_valid_saved_layout() {
        let picos = vec![
            pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        ];
        let saved = vec![config::RouteConfig {
            source_slot: 2,
            pico_uid: 0x523861E6,
            label: None,
        }];

        let routes = recommended_routes(&picos, &saved).unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].source_slot, 2);
        assert_eq!(routes[0].pico.info.unique_id_short, 0x523861E6);
    }

    #[test]
    fn recommended_routes_falls_back_when_saved_layout_is_stale() {
        let picos = vec![pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W)];
        let saved = vec![config::RouteConfig {
            source_slot: 2,
            pico_uid: 0x523861E6,
            label: None,
        }];

        let routes = recommended_routes(&picos, &saved).unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].source_slot, 0);
        assert_eq!(routes[0].pico.info.unique_id_short, 0x07D37EB6);
    }

    #[test]
    fn recommended_routes_maps_multiple_picos_in_order() {
        let picos = vec![
            pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        ];

        let routes = recommended_routes(&picos, &[]).unwrap();

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].source_slot, 0);
        assert_eq!(routes[0].pico.info.unique_id_short, 0x07D37EB6);
        assert_eq!(routes[1].source_slot, 1);
        assert_eq!(routes[1].pico.info.unique_id_short, 0x523861E6);
    }

    #[test]
    fn recommended_route_label_reports_recovery_needed() {
        let label = recommended_route_label(&Err(anyhow::anyhow!("no route")));

        assert_eq!(label, "Start streaming (needs routing fix)");
    }
}
