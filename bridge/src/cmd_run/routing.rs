use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};

use anyhow::{anyhow, bail, Context, Result};

use crate::{config, protocol, xinput};

use super::{PicoTarget, RunOptions, StreamRoute};
pub fn identity_from_target(pico: &PicoTarget) -> config::PicoIdentity {
    config::PicoIdentity {
        unique_id_short: pico.info.unique_id_short,
        board_type: pico.info.board_type,
        fw_major: pico.info.fw_major,
        fw_minor: pico.info.fw_minor,
        fw_patch: pico.info.fw_patch,
        last_ip: Some(pico.peer.ip().to_string()),
        device_name: Some(pico.board_label().to_string()),
        nickname: None,
    }
}

pub fn auto_routes(
    picos: Vec<PicoTarget>,
    preferred_slots: Option<Vec<u32>>,
) -> Result<Vec<StreamRoute>> {
    if picos.is_empty() {
        bail!("no Picos selected");
    }
    let connected: Vec<u32> = xinput::connected_slots()
        .into_iter()
        .map(|s| s.slot)
        .collect();
    let mut slots = preferred_slots.unwrap_or_else(|| {
        if connected.is_empty() {
            (0..picos.len().min(4)).map(|i| i as u32).collect()
        } else {
            connected
        }
    });
    slots.sort_unstable();
    slots.dedup();
    if slots.is_empty() {
        bail!("no XInput source slots are available");
    }
    if slots.len() < picos.len() {
        bail!(
            "{} Pico(s) selected but only {} source controller slot(s) available. Use --route to map explicit slots.",
            picos.len(),
            slots.len()
        );
    }
    Ok(picos
        .into_iter()
        .zip(slots)
        .map(|(pico, source_slot)| StreamRoute { source_slot, pico })
        .collect())
}

pub fn parse_route_specs(specs: &[String], picos: &[PicoTarget]) -> Result<Vec<StreamRoute>> {
    let mut routes = Vec::new();
    for spec in specs {
        let (source, target) = spec
            .split_once('=')
            .or_else(|| spec.split_once(':'))
            .ok_or_else(|| anyhow!("route must look like 1=07D37EB6 or 2=192.168.50.4"))?;
        let source_slot = parse_user_slot(source)?;
        let pico = match_pico_selector(target, picos)?;
        routes.push(StreamRoute { source_slot, pico });
    }
    if routes.is_empty() {
        bail!("no routes provided");
    }
    Ok(routes)
}

pub fn parse_user_slot(input: &str) -> Result<u32> {
    let s = input
        .trim()
        .trim_start_matches(['p', 'P'])
        .trim_start_matches(['x', 'X']);
    let user_slot: u32 = s
        .parse()
        .with_context(|| format!("invalid controller slot `{input}`"))?;
    if !(1..=4).contains(&user_slot) {
        bail!("controller slot must be 1, 2, 3, or 4");
    }
    Ok(user_slot - 1)
}

pub fn match_pico_selector(selector: &str, picos: &[PicoTarget]) -> Result<PicoTarget> {
    let selector = selector.trim();
    if selector.is_empty() {
        bail!("empty Pico selector");
    }

    if let Some(ip) = parse_ip_selector(selector) {
        let matches: Vec<_> = picos.iter().filter(|p| p.peer.ip() == ip).collect();
        return single_match(selector, matches);
    }

    let uid_text = selector
        .strip_prefix("0x")
        .or_else(|| selector.strip_prefix("0X"))
        .unwrap_or(selector);
    if uid_text.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(uid) = u32::from_str_radix(uid_text, 16) {
            let matches: Vec<_> = picos
                .iter()
                .filter(|p| p.info.unique_id_short == uid)
                .collect();
            return single_match(selector, matches);
        }
    }

    let wanted_board = match selector.to_ascii_lowercase().as_str() {
        "rp2350" | "pico2" | "pico2w" | "pico-2-w" => Some(protocol::BOARD_PICO_2_W),
        "rp2040" | "picow" | "pico-w" | "pico-wh" => Some(protocol::BOARD_PICO_W_RP2040),
        _ => None,
    };
    if let Some(board) = wanted_board {
        let matches: Vec<_> = picos
            .iter()
            .filter(|p| p.info.board_type == board)
            .collect();
        return single_match(selector, matches);
    }

    bail!(
        "Pico `{}` was not found. Use a UID like 07D37EB6, an IP address, or rp2350/rp2040.",
        selector
    );
}

pub fn parse_ip_selector(input: &str) -> Option<IpAddr> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Some(ip);
    }
    trimmed.parse::<SocketAddr>().ok().map(|addr| addr.ip())
}

pub(super) fn manual_ips_from_options(options: &RunOptions) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for spec in &options.picos {
        push_ip_if_new(&mut ips, spec);
    }
    for spec in &options.routes {
        if let Some(target) = route_target(spec) {
            push_ip_if_new(&mut ips, target);
        }
    }
    ips
}

fn push_ip_if_new(ips: &mut Vec<IpAddr>, input: &str) {
    let Some(ip) = parse_ip_selector(input) else {
        return;
    };
    if !ips.contains(&ip) {
        ips.push(ip);
    }
}

fn route_target(spec: &str) -> Option<&str> {
    spec.split_once('=')
        .or_else(|| spec.split_once(':'))
        .map(|(_, target)| target.trim())
}

fn single_match(selector: &str, matches: Vec<&PicoTarget>) -> Result<PicoTarget> {
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("Pico `{selector}` was not found in the current discovery results"),
        _ => bail!("Pico selector `{selector}` matched more than one board; use the UID instead"),
    }
}

pub(super) fn select_picos_by_specs(
    specs: &[String],
    picos: &[PicoTarget],
) -> Result<Vec<PicoTarget>> {
    let mut selected = Vec::new();
    for spec in specs {
        let pico = match_pico_selector(spec, picos)?;
        if !selected
            .iter()
            .any(|p: &PicoTarget| p.info.unique_id_short == pico.info.unique_id_short)
        {
            selected.push(pico);
        }
    }
    Ok(selected)
}

pub(super) fn routes_from_saved(
    saved: &[config::RouteConfig],
    picos: &[PicoTarget],
) -> Result<Vec<StreamRoute>> {
    if saved.is_empty() {
        bail!("no saved routing layout found; run `couchlink` to create one");
    }
    let mut routes = Vec::new();
    for saved_route in saved {
        let selector = format!("{:08X}", saved_route.pico_uid);
        let pico = match_pico_selector(&selector, picos)?;
        routes.push(StreamRoute {
            source_slot: saved_route.source_slot,
            pico,
        });
    }
    Ok(routes)
}

pub(super) fn validate_routes(routes: &[StreamRoute]) -> Result<()> {
    let mut pico_uids = HashSet::new();
    for route in routes {
        if route.source_slot >= 4 {
            bail!("controller slot must be 1, 2, 3, or 4");
        }
        if !pico_uids.insert(route.pico.info.unique_id_short) {
            bail!(
                "the same Pico ({}) is routed more than once. Pick one source controller per Pico.",
                route.pico.uid_hex()
            );
        }
    }
    Ok(())
}

pub(super) fn save_routes(routes: &[StreamRoute]) -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    cfg.routes = routes
        .iter()
        .map(|route| config::RouteConfig {
            source_slot: route.source_slot,
            pico_uid: route.pico.info.unique_id_short,
            label: Some(route.pico.board_label().to_string()),
        })
        .collect();
    for route in routes {
        cfg.remember_pico(identity_from_target(&route.pico));
    }
    config::save(&cfg)
}
