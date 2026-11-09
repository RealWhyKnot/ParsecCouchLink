mod ports;
mod targets;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::tui::{press_enter, select};
use crate::{cdc, cmd_configure_wifi, cmd_flash, cmd_run, cmd_usb_diag, pico_mode, protocol};
use ports::{
    WifiWaitResult, bootsel_mount_set, probe_setup_ports, read_setup_log,
    reboot_setup_port_to_bootsel, reboot_setup_port_to_wifi, setup_port_set,
    wait_for_bootsel_mounts_after, wait_for_setup_ports_after, wait_for_wifi_picos_after,
    wifi_uid_set,
};
use targets::{choose_setup_port, choose_wifi_target, resolve_setup_ports, resolve_wifi_targets};

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(5);
const MODE_SWITCH_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebugAction {
    Status,
    ToUsbDebug,
    ToWifi,
    ToBootsel,
    Logs,
}

impl DebugAction {
    pub(crate) fn chosen(flags: &[(bool, Self)]) -> Vec<Self> {
        flags
            .iter()
            .filter(|(requested, _)| *requested)
            .map(|(_, action)| *action)
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DebugOptions {
    pub actions: Vec<DebugAction>,
    pub all: bool,
    pub ips: Vec<String>,
    pub ports: Vec<String>,
}

pub(crate) async fn run(options: DebugOptions) -> Result<()> {
    match options.actions.as_slice() {
        [] => run_interactive().await,
        [DebugAction::Status] => {
            print_status().await?;
            Ok(())
        }
        [DebugAction::ToUsbDebug] => {
            let targets = resolve_wifi_targets(options.all, options.ips).await?;
            switch_wifi_targets_to_usb_debug(targets).await
        }
        [DebugAction::ToWifi] => {
            let ports = resolve_setup_ports(options.all, options.ports)?;
            switch_setup_ports_to_wifi(ports).await
        }
        [DebugAction::ToBootsel] => {
            let ports = resolve_setup_ports(options.all, options.ports)?;
            switch_setup_ports_to_bootsel(ports).await
        }
        [DebugAction::Logs] => {
            let ports = resolve_setup_ports(options.all, options.ports)?;
            print_setup_logs(ports).await
        }
        _ => bail!(
            "choose one debug action at a time: --status, --to-usb-debug, --to-wifi, --to-bootsel, or --logs"
        ),
    }
}

pub(crate) async fn switch_wifi_target_to_usb_debug(target: cmd_run::PicoTarget) -> Result<()> {
    switch_wifi_targets_to_usb_debug(vec![target]).await
}

pub(crate) async fn switch_setup_port_to_wifi_target(port: String) -> Result<()> {
    switch_setup_ports_to_wifi(vec![port]).await
}

pub(crate) async fn switch_setup_port_to_bootsel_target(
    port: String,
) -> Result<Vec<(PathBuf, cmd_flash::BootselBoard)>> {
    switch_setup_ports_to_bootsel_collect(vec![port]).await
}

pub(crate) async fn print_setup_port_log(port: String) -> Result<()> {
    print_setup_logs(vec![port]).await
}

async fn run_interactive() -> Result<()> {
    loop {
        outln!();
        outln!("Pico debug and recovery");
        outln!(
            "USB debug mode is the setup USB port. Wi-Fi/input mode is normal bridge operation."
        );
        print_status().await?;

        let choices = vec![
            menu_item(
                "Switch Wi-Fi Pico to USB debug mode",
                "use this before changing Wi-Fi or reading setup logs",
            ),
            menu_item(
                "Switch USB debug Pico to Wi-Fi/input mode",
                "return to normal bridge input operation",
            ),
            menu_item(
                "Switch USB debug Pico to BOOTSEL firmware mode",
                "prepare for firmware flashing without pressing BOOTSEL",
            ),
            menu_item(
                "Read USB debug log",
                "show recent firmware boot, Wi-Fi, and USB messages",
            ),
            menu_item(
                "Check controller USB side over Wi-Fi",
                "verify the console adapter accepted the Pico as XInput",
            ),
            menu_item(
                "Set up or change Wi-Fi",
                "send SSID and password while the Pico is in USB debug mode",
            ),
            menu_item("Refresh status", "scan USB debug, Wi-Fi, and BOOTSEL again"),
            menu_item("Back", "return to the previous menu"),
        ];
        let action = select("Debug action", &choices, 0).await?;
        let result = match action {
            0 => switch_one_wifi_pico_to_usb_debug().await,
            1 => switch_one_setup_port_to_wifi().await,
            2 => switch_one_setup_port_to_bootsel().await,
            3 => read_one_setup_log().await,
            4 => cmd_usb_diag::run_interactive().await,
            5 => cmd_configure_wifi::run().await,
            6 => Ok(()),
            _ => return Ok(()),
        };
        if let Err(e) = result {
            outln!();
            outln!("Debug action did not complete:");
            outln!("  {e:#}");
            outln!();
            print_recovery_steps();
        }
        if action != 6 {
            press_enter("Press Enter to return to Pico debug.").await?;
        }
    }
}

async fn print_status() -> Result<()> {
    let setup_ports = cdc::find_setup_ports().context("enumerating setup-mode USB ports")?;
    let setup = probe_setup_ports(setup_ports).await;
    let wifi = cmd_run::discover_picos(Duration::from_secs(3))
        .await
        .context("discovering Wi-Fi Pico boards")?;
    let bootsel = cmd_flash::visible_bootsel_mounts();

    outln!();
    outln!("Current Pico state");
    if setup.is_empty() && wifi.is_empty() && bootsel.is_empty() {
        outln!("  No Pico found in USB debug, Wi-Fi, or BOOTSEL mode.");
        print_recovery_steps();
        return Ok(());
    }

    if setup.is_empty() {
        outln!("  USB debug mode: none");
    } else {
        outln!("  USB debug mode:");
        for probe in &setup {
            outln!("    {}", probe.status_line());
        }
    }

    if wifi.is_empty() {
        outln!("  Wi-Fi/input mode: none");
    } else {
        outln!("  Wi-Fi/input mode:");
        for pico in &wifi {
            outln!("    {}", pico.detail_label());
            outln!("      manual IP: {}", pico.peer.ip());
        }
    }

    if bootsel.is_empty() {
        outln!("  BOOTSEL firmware mode: none");
    } else {
        outln!("  BOOTSEL firmware mode:");
        for (mount, board) in &bootsel {
            outln!("    {}  {}", mount.display(), board.label());
        }
    }

    Ok(())
}

async fn switch_one_wifi_pico_to_usb_debug() -> Result<()> {
    let Some(target) = choose_wifi_target().await? else {
        return Ok(());
    };
    switch_wifi_targets_to_usb_debug(vec![target]).await
}

async fn switch_wifi_targets_to_usb_debug(targets: Vec<cmd_run::PicoTarget>) -> Result<()> {
    if targets.is_empty() {
        bail!("no Wi-Fi Pico target selected");
    }
    let before = setup_port_set()?;
    for target in &targets {
        outln!(
            "Asking {} to reboot into USB debug mode...",
            target.short_label()
        );
        pico_mode::request_reboot_to_setup(target).await?;
    }
    outln!("Waiting for setup-mode USB...");
    let ports = wait_for_setup_ports_after(&before, MODE_SWITCH_TIMEOUT).await?;
    outln!("USB debug mode is available:");
    for probe in probe_setup_ports(ports).await {
        outln!("  {}", probe.status_line());
    }
    Ok(())
}

async fn switch_one_setup_port_to_wifi() -> Result<()> {
    let Some(port) = choose_setup_port("Which USB debug Pico should switch to Wi-Fi?").await?
    else {
        return Ok(());
    };
    switch_setup_ports_to_wifi(vec![port]).await
}

async fn switch_setup_ports_to_wifi(ports: Vec<String>) -> Result<()> {
    if ports.is_empty() {
        bail!("no setup-mode USB Pico selected");
    }
    let before_wifi = wifi_uid_set().await?;
    for port in &ports {
        outln!("Asking {port} to reboot into Wi-Fi/input mode...");
        let hello = reboot_setup_port_to_wifi(port.clone()).await?;
        outln!(
            "  {port} fw v{} {} -> Wi-Fi mode",
            hello.firmware_version(),
            board_type_label(hello.board_type)
        );
    }
    outln!("Waiting up to 60 s for a Wi-Fi reply...");
    match wait_for_wifi_picos_after(&before_wifi, MODE_SWITCH_TIMEOUT).await? {
        WifiWaitResult::Found(picos) => {
            outln!("Wi-Fi/input mode is available:");
            for pico in picos {
                outln!("  {}", pico.detail_label());
                outln!("  manual IP: {}", pico.peer.ip());
            }
            Ok(())
        }
        WifiWaitResult::TimedOut => {
            outln!("No Pico replied on Wi-Fi after switching modes.");
            if let Ok(ports) = cdc::find_setup_ports()
                && !ports.is_empty()
            {
                outln!("Setup-mode USB is still visible; reading recent debug log.");
                print_setup_logs(ports).await?;
            }
            bail!(
                "Wi-Fi mode did not answer. Check Wi-Fi credentials, router isolation, or use USB debug mode to reconfigure Wi-Fi."
            )
        }
    }
}

async fn switch_one_setup_port_to_bootsel() -> Result<()> {
    let Some(port) = choose_setup_port("Which USB debug Pico should switch to BOOTSEL?").await?
    else {
        return Ok(());
    };
    switch_setup_ports_to_bootsel(vec![port]).await
}

async fn switch_setup_ports_to_bootsel(ports: Vec<String>) -> Result<()> {
    switch_setup_ports_to_bootsel_collect(ports).await?;
    Ok(())
}

async fn switch_setup_ports_to_bootsel_collect(
    ports: Vec<String>,
) -> Result<Vec<(PathBuf, cmd_flash::BootselBoard)>> {
    if ports.is_empty() {
        bail!("no setup-mode USB Pico selected");
    }
    let before = bootsel_mount_set();
    for port in &ports {
        outln!("Asking {port} to reboot into BOOTSEL firmware mode...");
        reboot_setup_port_to_bootsel(port.clone()).await?;
    }
    outln!("Waiting for BOOTSEL drive...");
    let mounts = wait_for_bootsel_mounts_after(&before, MODE_SWITCH_TIMEOUT).await?;
    outln!("BOOTSEL firmware mode is available:");
    for (mount, board) in &mounts {
        outln!("  {}  {}", mount.display(), board.label());
    }
    Ok(mounts)
}

async fn read_one_setup_log() -> Result<()> {
    let Some(port) = choose_setup_port("Which USB debug Pico log should be read?").await? else {
        return Ok(());
    };
    print_setup_logs(vec![port]).await
}

async fn print_setup_logs(ports: Vec<String>) -> Result<()> {
    if ports.is_empty() {
        bail!("no setup-mode USB Pico selected");
    }
    for port in ports {
        outln!();
        outln!("USB debug log from {port}");
        match read_setup_log(port.clone()).await {
            Ok((text, lost)) if !text.trim().is_empty() => {
                if lost > 0 {
                    outln!("  {lost} older byte(s) were dropped from the firmware ring buffer.");
                }
                let lines: Vec<&str> = text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                let start = lines.len().saturating_sub(80);
                for line in &lines[start..] {
                    outln!("  {line}");
                }
            }
            Ok((_text, _lost)) => outln!("  Firmware log is empty."),
            Err(e) => outln!("  Could not read log: {e:#}"),
        }
    }
    Ok(())
}

fn print_recovery_steps() {
    outln!("Recovery paths:");
    outln!("  If the Pico is on Wi-Fi, choose `Switch Wi-Fi Pico to USB debug mode`.");
    outln!("  If USB debug mode is visible, choose `Switch USB debug Pico to Wi-Fi/input mode`.");
    outln!("  If neither mode is visible, use `Update Pico firmware` and BOOTSEL firmware mode.");
    outln!(
        "  BOOTSEL is the hardware fallback: hold BOOTSEL while plugging the Pico into this PC."
    );
}

fn board_type_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W / WH",
        _ => "Pico",
    }
}

fn creds_label(hello: cdc::HelloAck) -> &'static str {
    if hello.creds_present() {
        "saved Wi-Fi: yes"
    } else {
        "saved Wi-Fi: no"
    }
}

fn menu_item(label: &str, help: &str) -> String {
    format!("{label:<48} {help}")
}

#[cfg(test)]
mod tests;
