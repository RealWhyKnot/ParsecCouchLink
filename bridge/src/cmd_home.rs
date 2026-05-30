//! Guided no-argument entrypoint. Direct subcommands remain available
//! for scripts, startup shortcuts, and third-party launchers.

use std::time::Duration;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};

use crate::{
    cmd_bundle, cmd_configure_wifi, cmd_doctor, cmd_flash, cmd_logs, cmd_run, cmd_setup,
    cmd_usb_diag, config, support, xinput,
};

pub async fn run() -> Result<()> {
    loop {
        print_header();
        let choices = vec![
            "Start streaming",
            "Update Pico firmware",
            "Set up or change Wi-Fi",
            "Fix a problem",
            "Advanced commands",
            "Quit",
        ];
        match select("What do you want to do?", &choices, 0).await? {
            0 => start_routing().await?,
            1 => flash_menu().await?,
            2 => cmd_configure_wifi::run().await?,
            3 => support_menu().await?,
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
                "Set up or change Wi-Fi",
                "Fix a problem",
                "Update firmware",
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
                3 => support_menu().await?,
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
            recommended_route_label(&recommended),
            "Change controller routing".to_string(),
            "Back".to_string(),
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
        "Use one controller",
        "Use one controller per Pico",
        "Choose each controller manually",
        "Back",
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

async fn support_menu() -> Result<()> {
    loop {
        println!();
        println!("Fix a problem");
        let choices = vec![
            "Run health check",
            "Check Pico USB adapter",
            "Create support bundle",
            "Show log folder",
            "Follow live log",
            "Back",
        ];
        match select("Support option", &choices, 0).await? {
            0 => {
                cmd_doctor::run_interactive().await?;
                press_enter("Press Enter to return to support options.").await?;
            }
            1 => {
                cmd_usb_diag::run_interactive().await?;
                press_enter("Press Enter to return to support options.").await?;
            }
            2 => cmd_bundle::run(None).await?,
            3 => cmd_logs::run(false).await?,
            4 => cmd_logs::run(true).await?,
            _ => return Ok(()),
        }
    }
}

async fn show_direct_commands() -> Result<()> {
    println!();
    println!("Advanced commands");
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
    println!("  couchlink test usb --all          check Pico USB/XInput host status over Wi-Fi");
    println!("  couchlink test usb --ip 192.168.50.4  check one Pico by manual IP");
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
