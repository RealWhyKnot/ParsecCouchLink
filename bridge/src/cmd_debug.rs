//! Guided Pico recovery and mode-switching tools.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use crate::tui::{input_text, press_enter, select};
use crate::{
    cdc, cmd_configure_wifi, cmd_flash, cmd_run, cmd_usb_diag, pico_mode, protocol, support,
};

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(5);
const MODE_SWITCH_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default)]
pub struct DebugOptions {
    pub status: bool,
    pub to_usb_debug: bool,
    pub to_wifi: bool,
    pub to_bootsel: bool,
    pub logs: bool,
    pub all: bool,
    pub ips: Vec<String>,
    pub ports: Vec<String>,
}

impl DebugOptions {
    fn action_count(&self) -> usize {
        [
            self.status,
            self.to_usb_debug,
            self.to_wifi,
            self.to_bootsel,
            self.logs,
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
    }
}

pub async fn run(options: DebugOptions) -> Result<()> {
    match options.action_count() {
        0 => run_interactive().await,
        1 if options.status => {
            print_status().await?;
            Ok(())
        }
        1 if options.to_usb_debug => {
            let targets = resolve_wifi_targets(options.all, options.ips).await?;
            switch_wifi_targets_to_usb_debug(targets).await
        }
        1 if options.to_wifi => {
            let ports = resolve_setup_ports(options.all, options.ports)?;
            switch_setup_ports_to_wifi(ports).await
        }
        1 if options.to_bootsel => {
            let ports = resolve_setup_ports(options.all, options.ports)?;
            switch_setup_ports_to_bootsel(ports).await
        }
        1 if options.logs => {
            let ports = resolve_setup_ports(options.all, options.ports)?;
            print_setup_logs(ports).await
        }
        _ => bail!(
            "choose one debug action at a time: --status, --to-usb-debug, --to-wifi, --to-bootsel, or --logs"
        ),
    }
}

async fn run_interactive() -> Result<()> {
    loop {
        println!();
        println!("Pico debug and recovery");
        println!(
            "USB debug mode is the setup USB port. Wi-Fi mode is the normal controller bridge."
        );
        print_status().await?;

        let choices = vec![
            menu_item(
                "Switch Wi-Fi Pico to USB debug mode",
                "use this before changing Wi-Fi or reading setup logs",
            ),
            menu_item(
                "Switch USB debug Pico to Wi-Fi/controller mode",
                "return to normal bridge/controller operation",
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
            println!();
            println!("Debug action did not complete:");
            println!("  {e:#}");
            println!();
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

    println!();
    println!("Current Pico state");
    if setup.is_empty() && wifi.is_empty() && bootsel.is_empty() {
        println!("  No Pico found in USB debug, Wi-Fi, or BOOTSEL mode.");
        print_recovery_steps();
        return Ok(());
    }

    if setup.is_empty() {
        println!("  USB debug mode: none");
    } else {
        println!("  USB debug mode:");
        for probe in &setup {
            println!("    {}", probe.status_line());
        }
    }

    if wifi.is_empty() {
        println!("  Wi-Fi/controller mode: none");
    } else {
        println!("  Wi-Fi/controller mode:");
        for pico in &wifi {
            println!("    {}", pico.detail_label());
            println!("      manual IP: {}", pico.peer.ip());
        }
    }

    if bootsel.is_empty() {
        println!("  BOOTSEL firmware mode: none");
    } else {
        println!("  BOOTSEL firmware mode:");
        for (mount, board) in &bootsel {
            println!("    {}  {}", mount.display(), board.label());
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
        println!(
            "Asking {} to reboot into USB debug mode...",
            target.short_label()
        );
        pico_mode::request_reboot_to_setup(target).await?;
    }
    println!("Waiting for setup-mode USB...");
    let ports = wait_for_setup_ports_after(&before, MODE_SWITCH_TIMEOUT).await?;
    println!("USB debug mode is available:");
    for probe in probe_setup_ports(ports).await {
        println!("  {}", probe.status_line());
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
        println!("Asking {port} to reboot into Wi-Fi/controller mode...");
        let hello = reboot_setup_port_to_wifi(port.clone()).await?;
        println!(
            "  {port} fw v{} {} -> Wi-Fi mode",
            hello.firmware_version(),
            board_type_label(hello.board_type)
        );
    }
    println!("Waiting up to 60 s for a Wi-Fi reply...");
    match wait_for_wifi_picos_after(&before_wifi, MODE_SWITCH_TIMEOUT).await? {
        WifiWaitResult::Found(picos) => {
            println!("Wi-Fi/controller mode is available:");
            for pico in picos {
                println!("  {}", pico.detail_label());
                println!("  manual IP: {}", pico.peer.ip());
            }
            Ok(())
        }
        WifiWaitResult::TimedOut => {
            println!("No Pico replied on Wi-Fi after switching modes.");
            if let Ok(ports) = cdc::find_setup_ports() {
                if !ports.is_empty() {
                    println!("Setup-mode USB is still visible; reading recent debug log.");
                    print_setup_logs(ports).await?;
                }
            }
            bail!("Wi-Fi mode did not answer. Check Wi-Fi credentials, router isolation, or use USB debug mode to reconfigure Wi-Fi.")
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
    if ports.is_empty() {
        bail!("no setup-mode USB Pico selected");
    }
    let before = bootsel_mount_set();
    for port in &ports {
        println!("Asking {port} to reboot into BOOTSEL firmware mode...");
        reboot_setup_port_to_bootsel(port.clone()).await?;
    }
    println!("Waiting for BOOTSEL drive...");
    let mounts = wait_for_bootsel_mounts_after(&before, MODE_SWITCH_TIMEOUT).await?;
    println!("BOOTSEL firmware mode is available:");
    for (mount, board) in mounts {
        println!("  {}  {}", mount.display(), board.label());
    }
    Ok(())
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
        println!();
        println!("USB debug log from {port}");
        match read_setup_log(port.clone()).await {
            Ok((text, lost)) if !text.trim().is_empty() => {
                if lost > 0 {
                    println!("  {lost} older byte(s) were dropped from the firmware ring buffer.");
                }
                let lines: Vec<&str> = text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                let start = lines.len().saturating_sub(80);
                for line in &lines[start..] {
                    println!("  {line}");
                }
            }
            Ok((_text, _lost)) => println!("  Firmware log is empty."),
            Err(e) => println!("  Could not read log: {e:#}"),
        }
    }
    Ok(())
}

async fn choose_wifi_target() -> Result<Option<cmd_run::PicoTarget>> {
    loop {
        println!("Looking for running Pico boards on Wi-Fi...");
        let picos = cmd_run::discover_picos(DISCOVER_TIMEOUT).await?;
        if picos.is_empty() {
            support::print_no_pico_wifi_help(DISCOVER_TIMEOUT.as_secs());
            let choices = vec![
                menu_item("Try Wi-Fi discovery again", "repeat broadcast discovery"),
                menu_item("Enter Pico IP manually", "use the IP shown by your router"),
                menu_item("Back", "return to Pico debug"),
            ];
            match select("Wi-Fi Pico", &choices, 0).await? {
                0 => continue,
                1 => return prompt_manual_pico_ip().await,
                _ => return Ok(None),
            }
        }

        let mut items: Vec<String> = picos
            .iter()
            .map(|p| menu_item(&p.detail_label(), "switch this Pico to USB debug"))
            .collect();
        items.push(menu_item(
            "Enter Pico IP manually",
            "use the IP shown by your router",
        ));
        items.push(menu_item("Back", "return to Pico debug"));
        let idx = select("Wi-Fi Pico", &items, 0).await?;
        if idx < picos.len() {
            return Ok(Some(picos[idx].clone()));
        }
        if idx == picos.len() {
            return prompt_manual_pico_ip().await;
        }
        return Ok(None);
    }
}

fn resolve_setup_ports(all: bool, requested: Vec<String>) -> Result<Vec<String>> {
    let visible = cdc::find_setup_ports()?;
    if !requested.is_empty() {
        let visible_set: BTreeSet<String> = visible.iter().cloned().collect();
        let mut selected = Vec::new();
        for port in requested {
            if !visible_set.contains(&port) {
                bail!("{port} is not a visible USB debug Pico port. Visible ports: {visible:?}");
            }
            selected.push(port);
        }
        return Ok(selected);
    }
    if visible.is_empty() {
        bail!(
            "no Pico in USB debug mode found (VID 0x{:04X}, PID 0x{:04X})",
            cdc::SETUP_VID,
            cdc::SETUP_PID,
        );
    }
    if all || visible.len() == 1 {
        return Ok(if all {
            visible
        } else {
            vec![visible[0].clone()]
        });
    }
    bail!(
        "{} USB debug Pico ports are visible. Run `couchlink debug` for the picker or add --all/--port.",
        visible.len()
    )
}

async fn resolve_wifi_targets(all: bool, ips: Vec<String>) -> Result<Vec<cmd_run::PicoTarget>> {
    if !ips.is_empty() {
        let mut targets = Vec::new();
        for text in ips {
            let ip = parse_ip_arg(&text)?;
            targets.push(cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await?);
        }
        return Ok(targets);
    }

    let picos = cmd_run::discover_picos(DISCOVER_TIMEOUT).await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(DISCOVER_TIMEOUT.as_secs()));
    }
    if all || picos.len() == 1 {
        return Ok(if all { picos } else { vec![picos[0].clone()] });
    }
    bail!(
        "{} Wi-Fi Pico boards replied. Run `couchlink debug` for the picker or add --all/--ip.",
        picos.len()
    )
}

async fn choose_setup_port(prompt: &str) -> Result<Option<String>> {
    let ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        println!(
            "No Pico is in USB debug mode (VID 0x{:04X}, PID 0x{:04X}).",
            cdc::SETUP_VID,
            cdc::SETUP_PID,
        );
        println!("If the Pico is on Wi-Fi, switch it to USB debug mode first.");
        println!("If it is not visible anywhere, use BOOTSEL firmware update mode.");
        return Ok(None);
    }
    if ports.len() == 1 {
        return Ok(Some(ports[0].clone()));
    }
    let idx = select(prompt, &ports, 0).await?;
    Ok(Some(ports[idx].clone()))
}

async fn prompt_manual_pico_ip() -> Result<Option<cmd_run::PicoTarget>> {
    let text = input_text("Pico IP address (blank to cancel)").await?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let ip = match parse_ip_arg(&text) {
        Ok(ip) => ip,
        Err(e) => {
            println!("Invalid IP address: {e:#}");
            return Ok(None);
        }
    };
    println!("Probing {ip}:{} directly...", protocol::PORT);
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await {
        Ok(pico) => Ok(Some(pico)),
        Err(e) => {
            println!("No Pico replied at {ip}: {e:#}");
            Ok(None)
        }
    }
}

fn parse_ip_arg(text: &str) -> Result<IpAddr> {
    cmd_run::parse_ip_selector(text)
        .ok_or_else(|| anyhow!("`{}` is not a valid IP address", text.trim()))
}

async fn probe_setup_ports(ports: Vec<String>) -> Vec<SetupProbe> {
    let mut probes = Vec::new();
    for port in ports {
        let port_for_task = port.clone();
        let probe =
            tokio::task::spawn_blocking(move || -> Result<(cdc::HelloAck, cdc::SelfTestAck)> {
                let mut pico = cdc::PicoSetup::open_named(&port_for_task)?;
                let hello = pico.hello()?;
                let self_test = pico.self_test()?;
                Ok((hello, self_test))
            })
            .await;
        match probe {
            Ok(Ok((hello, self_test))) => probes.push(SetupProbe {
                port,
                hello: Some(hello),
                self_test: Some(self_test),
                error: None,
            }),
            Ok(Err(e)) => probes.push(SetupProbe {
                port,
                hello: None,
                self_test: None,
                error: Some(format!("{e:#}")),
            }),
            Err(e) => probes.push(SetupProbe {
                port,
                hello: None,
                self_test: None,
                error: Some(format!("probe task failed: {e}")),
            }),
        }
    }
    probes
}

async fn reboot_setup_port_to_wifi(port: String) -> Result<cdc::HelloAck> {
    tokio::task::spawn_blocking(move || -> Result<cdc::HelloAck> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        if hello.proto_version != cdc::PROTO_VERSION {
            bail!(
                "Pico speaks CDC protocol v{}, bridge speaks v{}. Update whichever side is older.",
                hello.proto_version,
                cdc::PROTO_VERSION,
            );
        }
        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }
        if !hello.creds_present() {
            bail!("Pico has no saved Wi-Fi credentials. Choose `Set up or change Wi-Fi` first.");
        }
        pico.reboot_to_run()?;
        Ok(hello)
    })
    .await?
}

async fn reboot_setup_port_to_bootsel(port: String) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        let hello = pico.hello()?;
        let self_test = pico.self_test()?;
        if !self_test.passed {
            bail!("SELF_TEST failed: {}", self_test.message);
        }
        println!(
            "  {port} fw v{} {} -> BOOTSEL",
            hello.firmware_version(),
            board_type_label(hello.board_type)
        );
        pico.reboot_to_bootsel()?;
        Ok(())
    })
    .await?
}

async fn read_setup_log(port: String) -> Result<(String, u32)> {
    tokio::task::spawn_blocking(move || -> Result<(String, u32)> {
        let mut pico = cdc::PicoSetup::open_named(&port)?;
        pico.get_log_buffer()
    })
    .await?
}

fn setup_port_set() -> Result<BTreeSet<String>> {
    Ok(cdc::find_setup_ports()?.into_iter().collect())
}

fn bootsel_mount_set() -> BTreeSet<String> {
    cmd_flash::visible_bootsel_mounts()
        .into_iter()
        .map(|(path, _board)| path.display().to_string())
        .collect()
}

async fn wait_for_setup_ports_after(
    before: &BTreeSet<String>,
    timeout: Duration,
) -> Result<Vec<String>> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    loop {
        let ports = cdc::find_setup_ports()?;
        let current: BTreeSet<String> = ports.iter().cloned().collect();
        if !ports.is_empty() && &current != before {
            return Ok(ports);
        }
        if before.is_empty() && !ports.is_empty() {
            return Ok(ports);
        }
        let now = Instant::now();
        if now >= deadline {
            bail!(
                "the Pico did not reappear in USB debug mode within {} s",
                timeout.as_secs()
            );
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            println!(
                "  ... still waiting for USB debug mode ({elapsed}/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn wait_for_bootsel_mounts_after(
    before: &BTreeSet<String>,
    timeout: Duration,
) -> Result<Vec<(PathBuf, cmd_flash::BootselBoard)>> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    loop {
        let mounts = cmd_flash::visible_bootsel_mounts();
        let current: BTreeSet<String> = mounts
            .iter()
            .map(|(path, _board)| path.display().to_string())
            .collect();
        if !mounts.is_empty() && &current != before {
            return Ok(mounts);
        }
        if before.is_empty() && !mounts.is_empty() {
            return Ok(mounts);
        }
        let now = Instant::now();
        if now >= deadline {
            bail!(
                "the Pico did not reappear as a BOOTSEL drive within {} s",
                timeout.as_secs()
            );
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            println!(
                "  ... still waiting for BOOTSEL ({elapsed}/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

enum WifiWaitResult {
    Found(Vec<cmd_run::PicoTarget>),
    TimedOut,
}

async fn wifi_uid_set() -> Result<BTreeSet<u32>> {
    Ok(cmd_run::discover_picos(Duration::from_secs(2))
        .await?
        .into_iter()
        .map(|pico| pico.info.unique_id_short)
        .collect())
}

async fn wait_for_wifi_picos_after(
    before: &BTreeSet<u32>,
    timeout: Duration,
) -> Result<WifiWaitResult> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    tokio::time::sleep(Duration::from_secs(3)).await;
    loop {
        let picos = cmd_run::discover_picos(Duration::from_secs(2)).await?;
        let current: BTreeSet<u32> = picos.iter().map(|pico| pico.info.unique_id_short).collect();
        if !picos.is_empty() && &current != before {
            return Ok(WifiWaitResult::Found(picos));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(WifiWaitResult::TimedOut);
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            println!(
                "  ... still waiting for Wi-Fi reply ({elapsed}/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
    }
}

fn print_recovery_steps() {
    println!("Recovery paths:");
    println!("  If the Pico is on Wi-Fi, choose `Switch Wi-Fi Pico to USB debug mode`.");
    println!(
        "  If USB debug mode is visible, choose `Switch USB debug Pico to Wi-Fi/controller mode`."
    );
    println!("  If neither mode is visible, use `Update Pico firmware` and BOOTSEL firmware mode.");
    println!(
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

struct SetupProbe {
    port: String,
    hello: Option<cdc::HelloAck>,
    self_test: Option<cdc::SelfTestAck>,
    error: Option<String>,
}

impl SetupProbe {
    fn status_line(&self) -> String {
        match (self.hello, &self.self_test, &self.error) {
            (Some(hello), Some(self_test), _) => format!(
                "{}  fw v{}  {}  {}  SELF_TEST {} {}",
                self.port,
                hello.firmware_version(),
                board_type_label(hello.board_type),
                creds_label(hello),
                if self_test.passed { "PASS" } else { "FAIL" },
                self_test.message
            ),
            (_, _, Some(error)) => format!("{}  visible, but HELLO failed: {error}", self.port),
            _ => format!("{}  visible, but probe did not finish", self.port),
        }
    }
}

fn menu_item(label: &str, help: &str) -> String {
    format!("{label:<48} {help}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_options_counts_actions() {
        assert_eq!(DebugOptions::default().action_count(), 0);
        let one = DebugOptions {
            status: true,
            ..DebugOptions::default()
        };
        assert_eq!(one.action_count(), 1);
        let two = DebugOptions {
            status: true,
            to_wifi: true,
            ..DebugOptions::default()
        };
        assert_eq!(two.action_count(), 2);
    }

    #[test]
    fn board_type_labels_are_user_facing() {
        assert_eq!(board_type_label(protocol::BOARD_PICO_2_W), "Pico 2 W");
        assert_eq!(
            board_type_label(protocol::BOARD_PICO_W_RP2040),
            "Pico W / WH"
        );
        assert_eq!(board_type_label(0xFF), "Pico");
    }
}
