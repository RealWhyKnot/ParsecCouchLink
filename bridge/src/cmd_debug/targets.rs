use super::{DISCOVER_TIMEOUT, menu_item};
use crate::tui::{input_text, select};
use crate::{cdc, cmd_run, protocol, support};
use anyhow::{Result, anyhow, bail};
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::time::Duration;

pub(super) async fn choose_wifi_target() -> Result<Option<cmd_run::PicoTarget>> {
    loop {
        outln!("Looking for running Pico boards on Wi-Fi...");
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

pub(super) fn resolve_setup_ports(all: bool, requested: Vec<String>) -> Result<Vec<String>> {
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

pub(super) async fn resolve_wifi_targets(
    all: bool,
    ips: Vec<String>,
) -> Result<Vec<cmd_run::PicoTarget>> {
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

pub(super) async fn choose_setup_port(prompt: &str) -> Result<Option<String>> {
    let ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        outln!(
            "No Pico is in USB debug mode (VID 0x{:04X}, PID 0x{:04X}).",
            cdc::SETUP_VID,
            cdc::SETUP_PID,
        );
        outln!("If the Pico is on Wi-Fi, switch it to USB debug mode first.");
        outln!("If it is not visible anywhere, use BOOTSEL firmware update mode.");
        return Ok(None);
    }
    if ports.len() == 1 {
        return Ok(Some(ports[0].clone()));
    }
    let idx = select(prompt, &ports, 0).await?;
    Ok(Some(ports[idx].clone()))
}

pub(super) async fn prompt_manual_pico_ip() -> Result<Option<cmd_run::PicoTarget>> {
    let text = input_text("Pico IP address (blank to cancel)").await?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let ip = match parse_ip_arg(&text) {
        Ok(ip) => ip,
        Err(e) => {
            outln!("Invalid IP address: {e:#}");
            return Ok(None);
        }
    };
    outln!("Probing {ip}:{} directly...", protocol::PORT);
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await {
        Ok(pico) => Ok(Some(pico)),
        Err(e) => {
            outln!("No Pico replied at {ip}: {e:#}");
            Ok(None)
        }
    }
}

pub(super) fn parse_ip_arg(text: &str) -> Result<IpAddr> {
    cmd_run::parse_ip_selector(text)
        .ok_or_else(|| anyhow!("`{}` is not a valid IP address", text.trim()))
}
