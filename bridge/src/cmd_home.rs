//! Guided no-argument entrypoint. Direct subcommands remain available
//! for scripts, startup shortcuts, and third-party launchers.

use std::time::Duration;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};

use crate::{
    cmd_configure_wifi, cmd_doctor, cmd_flash, cmd_run, cmd_setup, config, support, xinput,
};

pub async fn run() -> Result<()> {
    loop {
        print_header();
        let choices = vec![
            "Start or route controllers",
            "Flash or update Pico firmware",
            "Set or change Wi-Fi",
            "Run diagnostics",
            "Show direct commands",
            "Quit",
        ];
        match select("What do you want to do?", &choices, 0).await? {
            0 => start_routing().await?,
            1 => flash_menu().await?,
            2 => cmd_configure_wifi::run().await?,
            3 => cmd_doctor::run().await?,
            4 => show_direct_commands().await?,
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

async fn start_routing() -> Result<()> {
    loop {
        println!("Looking for running Pico boards on Wi-Fi...");
        let mut picos = cmd_run::discover_picos(Duration::from_secs(5)).await?;
        if picos.is_empty() {
            support::print_no_pico_wifi_help(5);
            let choices = vec![
                "Try discovery again",
                "Enter Pico IP manually",
                "Set or change Wi-Fi",
                "Run diagnostics",
                "Flash/update firmware",
                "Back",
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
                3 => cmd_doctor::run().await?,
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
        let mut choices = Vec::<String>::new();
        let mut saved_choice = None;
        if !saved.is_empty() {
            saved_choice = Some(choices.len());
            choices.push(saved_layout_label(&saved));
        }
        choices.extend([
            "Route one controller to one Pico".to_string(),
            "Auto-route controllers to every detected Pico".to_string(),
            "Custom routing".to_string(),
            "Back".to_string(),
        ]);

        let default = saved_choice.unwrap_or(0);
        let picked = select("Controller layout", &choices, default).await?;
        if Some(picked) == saved_choice {
            let routes = match saved
                .iter()
                .map(|route| {
                    let selector = format!("{:08X}", route.pico_uid);
                    let pico = cmd_run::match_pico_selector(&selector, &picos)?;
                    Ok(cmd_run::StreamRoute {
                        source_slot: route.source_slot,
                        pico,
                    })
                })
                .collect::<Result<Vec<_>>>()
            {
                Ok(routes) => routes,
                Err(e) => {
                    println!("Saved layout cannot be used with the Picos currently online:");
                    println!("  {e:#}");
                    continue;
                }
            };
            return stream(routes, true).await;
        }

        let offset = if saved_choice.is_some() { 1 } else { 0 };
        match picked - offset {
            0 => return route_one(picos).await,
            1 => {
                let routes = cmd_run::auto_routes(picos, Some((0..4).collect()))?;
                return stream(routes, true).await;
            }
            2 => return route_custom(picos).await,
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
        "Update firmware (recommended)",
        "Advanced flash options",
        "Back",
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
    println!("If this Pico is new or Wi-Fi was changed, set Wi-Fi before routing controllers.");
    if confirm("Set or change Wi-Fi now?", true).await? {
        cmd_configure_wifi::run().await?;
    } else {
        println!("Next: choose `Start or route controllers` from the main menu.");
    }
    Ok(())
}

async fn advanced_flash_menu() -> Result<()> {
    println!();
    println!("Advanced flash options");
    println!("Use these only when you already know which USB state the Pico is in.");
    let choices = vec![
        "No-button update from setup-mode USB",
        "Flash BOOTSEL drive",
        "Run full first-time setup",
        "Back",
    ];
    match select("Advanced flash path", &choices, 0).await? {
        0 => cmd_flash::run(None, true, true).await,
        1 => cmd_flash::run(None, true, false).await,
        2 => cmd_setup::run(None).await,
        _ => Ok(()),
    }
}

async fn show_direct_commands() -> Result<()> {
    println!();
    println!("Direct commands");
    println!("  couchlink                         guided menu");
    println!("  couchlink setup                   first-time setup wizard");
    println!("  couchlink flash --from-usb --all  no-button reflash for setup-mode Picos");
    println!("  couchlink flash --all             flash every BOOTSEL drive");
    println!("  couchlink run                     stream using the saved layout, or one Pico if no layout is saved");
    println!(
        "  couchlink run --all               map Controller 1, 2, ... to every discovered Pico"
    );
    println!("  couchlink run --route 1=UID       route Controller 1 to a specific Pico UID");
    println!("  couchlink run --pico 192.168.50.4 route to a Pico by manual IP");
    println!("  couchlink test discover --ip 192.168.50.4  probe a Pico by manual IP");
    println!("  couchlink test discover --all     show every Pico answering on Wi-Fi");
    println!("  couchlink test cdc --all          show every setup-mode Pico over USB");
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

fn saved_layout_label(routes: &[config::RouteConfig]) -> String {
    let parts: Vec<String> = routes
        .iter()
        .map(|route| {
            format!(
                "{} -> {:08X}",
                xinput::user_slot_label(route.source_slot),
                route.pico_uid
            )
        })
        .collect();
    format!("Use saved layout ({})", parts.join(", "))
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
