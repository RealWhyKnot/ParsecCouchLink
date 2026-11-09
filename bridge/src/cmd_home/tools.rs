use super::actions::prompt_manual_pico_ip;
use super::streaming::print_xinput_sources;
use crate::modes::{BootselEntry, PicoScope};
use crate::tui::{confirm, press_enter, select};
use crate::{
    cmd_bundle, cmd_configure_wifi, cmd_debug, cmd_doctor, cmd_flash, cmd_run, cmd_setup,
    cmd_usb_diag, support,
};
use anyhow::Result;
use std::time::Duration;

pub(super) async fn flash_menu() -> Result<()> {
    outln!();
    outln!("Firmware update");
    outln!(
        "Flashing does not require a controller. Controller routing is tested when streaming starts."
    );
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

pub(super) async fn guided_firmware_update() -> Result<()> {
    outln!();
    outln!("CouchLink will update the Pico firmware.");
    outln!("If the Pico is already in setup mode, this can happen without pressing BOOTSEL.");
    outln!("If not, the app will walk you through BOOTSEL flashing.");
    outln!();

    let result = cmd_flash::run(None, PicoScope::All, BootselEntry::OverUsb).await;
    if let Err(e) = result {
        outln!();
        outln!("Automatic USB update did not complete:");
        outln!("  {e:#}");
        outln!();
        outln!("Next step: put the Pico in BOOTSEL when prompted.");
        if !confirm("Continue with guided BOOTSEL flashing?", true).await? {
            return Ok(());
        }
        cmd_flash::run(None, PicoScope::All, BootselEntry::Manual).await?;
    }

    outln!();
    outln!("Firmware update complete.");
    outln!("If the Pico already had working Wi-Fi, keep it and start streaming.");
    outln!("If this Pico is new or the Wi-Fi changed, set Wi-Fi before routing controllers.");
    if confirm("Do you need to set up or change Wi-Fi now?", false).await? {
        cmd_configure_wifi::run().await?;
    } else {
        outln!("Next: return to Basic and choose this Pico's streaming command.");
    }
    Ok(())
}

pub(super) async fn advanced_flash_menu() -> Result<()> {
    outln!();
    outln!("Advanced flash options");
    outln!("Use these only when you already know which USB state the Pico is in.");
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
        0 => cmd_flash::run(None, PicoScope::All, BootselEntry::OverUsb).await,
        1 => cmd_flash::run(None, PicoScope::All, BootselEntry::Manual).await,
        2 => cmd_setup::run(None).await,
        _ => Ok(()),
    }
}

pub(super) async fn tools_menu() -> Result<()> {
    loop {
        outln!();
        outln!("Tabs: Basic [Advanced]");
        outln!();
        outln!("Advanced");
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
                "Check Pico USB adapter",
                "ask the Pico whether the console adapter accepted its input mode",
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
                cmd_usb_diag::run_interactive().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            4 => {
                pico_finder_tool().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            5 => {
                controller_tool().await?;
                press_enter("Press Enter to return to Advanced.").await?;
            }
            6 => cmd_bundle::run(None).await?,
            7 => show_direct_commands().await?,
            _ => return Ok(()),
        }
    }
}

pub(super) async fn quick_status_dashboard() -> Result<()> {
    outln!();
    outln!("Quick status dashboard");
    outln!("This is the fastest way to decide the next tool to use.");
    cmd_debug::run(cmd_debug::DebugOptions {
        actions: vec![cmd_debug::DebugAction::Status],
        ..cmd_debug::DebugOptions::default()
    })
    .await?;
    print_xinput_sources();
    outln!();
    outln!("Suggested next steps:");
    outln!(
        "  If a Pico appears on Wi-Fi, return to Basic and choose its streaming or USB adapter command."
    );
    outln!("  If a Pico appears in USB debug mode, choose `Set up or change Wi-Fi`.");
    outln!("  If a Pico appears in BOOTSEL, choose `Firmware update`.");
    outln!("  If no Pico appears anywhere, check the cable, then choose `Create support bundle`.");
    Ok(())
}

pub(super) async fn pico_finder_tool() -> Result<()> {
    loop {
        outln!();
        outln!("Find Picos on Wi-Fi");
        outln!("This checks the same discovery path used by streaming.");
        let picos = cmd_run::discover_picos(Duration::from_secs(5)).await?;
        if picos.is_empty() {
            support::print_no_pico_wifi_help(5);
        } else {
            outln!("Discovered Pico boards:");
            for pico in &picos {
                outln!("  {}", pico.detail_label());
                outln!("    manual IP: {}", pico.peer.ip());
            }
        }

        let choices = vec![
            menu_item("Scan again", "repeat Wi-Fi discovery"),
            menu_item("Probe manual IP", "test the IP shown in your router"),
            menu_item("Back", "return to Advanced"),
        ];
        match select("Wi-Fi finder", &choices, 0).await? {
            0 => {}
            1 => {
                let _ = prompt_manual_pico_ip().await?;
            }
            _ => return Ok(()),
        }
    }
}

pub(super) async fn controller_tool() -> Result<()> {
    outln!();
    outln!("Windows controller check");
    outln!("This checks the source controllers that CouchLink reads from Windows.");
    print_xinput_sources();
    outln!();
    match cmd_doctor::check_xinput().await {
        cmd_doctor::CheckResult::Pass(message) => outln!("PASS  {message}"),
        cmd_doctor::CheckResult::Warn(message) => {
            outln!("WARN  {message}");
            outln!(
                "Hint: start Parsec with a guest gamepad connected, or plug in a wired Xbox controller for bench testing."
            );
        }
        cmd_doctor::CheckResult::Skip(message) => outln!("SKIP  {message}"),
        cmd_doctor::CheckResult::Fail(message, hint) => {
            outln!("FAIL  {message}");
            outln!("Hint: {hint}");
        }
    }
    Ok(())
}

pub(super) async fn show_direct_commands() -> Result<()> {
    outln!();
    outln!("Advanced commands");
    outln!(
        "Use the guided menu for normal use. These commands stay available for scripts, shortcuts, and power users."
    );
    outln!();
    outln!("Guided setup");
    print_command("couchlink", "open this menu");
    print_command("couchlink setup", "first-time setup wizard");
    outln!();
    outln!("Streaming");
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
    outln!();
    outln!("Input modes");
    print_command(
        "couchlink auto",
        "try gamepad modes and keep the first one the adapter polls",
    );
    print_command("couchlink xinput", "switch to wired Xbox 360 USB mode");
    print_command("couchlink xbox", "try Xbox 360 and Xbox One modes");
    print_command("couchlink xbox360", "switch to wired Xbox 360 USB mode");
    print_command("couchlink xboxone", "switch to Xbox One USB mode");
    print_command(
        "couchlink dinput",
        "try PS3, generic HID, and PS4 DInput-family modes",
    );
    print_command("couchlink ps3", "switch to PS3 HID mode");
    print_command(
        "couchlink generic-hid",
        "switch to generic HID gamepad mode",
    );
    print_command("couchlink ps4", "switch to PS4 HID mode");
    print_command(
        "couchlink maple",
        "switch to Dreamcast adapter-labelled XInput mode",
    );
    print_command("couchlink keyboard", "switch to USB keyboard mode");
    outln!();
    outln!("Firmware update");
    print_command(
        "couchlink flash --from-usb --all",
        "no-button reflash for setup-mode Picos",
    );
    print_command("couchlink flash --all", "flash every BOOTSEL drive");
    outln!();
    outln!("Diagnostics");
    print_command("couchlink bundle", "create a support bundle");
    outln!();
    outln!("Detected Pico UID examples:");
    match cmd_run::discover_picos(Duration::from_secs(3)).await {
        Ok(picos) if !picos.is_empty() => {
            for pico in picos {
                outln!("  {}  {}", pico.uid_hex(), pico.short_label());
            }
        }
        Ok(_) => outln!("  No running Pico replied on Wi-Fi right now."),
        Err(e) => outln!("  Discovery failed: {e:#}"),
    }
    press_enter("Press Enter to return to the menu.").await
}

pub(super) fn menu_item(label: &str, help: &str) -> String {
    format!("{label:<36} {help}")
}

pub(super) fn print_command(command: &str, help: &str) {
    outln!("  {command:<42} {help}");
}
