use super::setup_probe::SetupLabProbe;
use crate::{cmd_flash, cmd_run, protocol};
use anyhow::{Context, Result, bail};
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PicoSelector {
    Uid(u32),
    Ip(IpAddr),
    Board(u8),
}

pub(super) fn parse_selectors(values: &[String]) -> Result<Vec<PicoSelector>> {
    values.iter().map(|s| parse_selector(s)).collect()
}

pub(super) fn parse_selector(value: &str) -> Result<PicoSelector> {
    let trimmed = value.trim();
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Ok(PicoSelector::Ip(ip));
    }

    let normalized = trimmed
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .replace(['-', '_', ' '], "")
        .to_ascii_lowercase();
    match normalized.as_str() {
        "pico2w" | "pico2" | "rp2350" => return Ok(PicoSelector::Board(protocol::BOARD_PICO_2_W)),
        "picow" | "picowh" | "pico" | "rp2040" => {
            return Ok(PicoSelector::Board(protocol::BOARD_PICO_W_RP2040));
        }
        _ => {}
    }

    if normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        let uid = u32::from_str_radix(&normalized, 16)
            .with_context(|| format!("invalid Pico UID selector `{value}`"))?;
        return Ok(PicoSelector::Uid(uid));
    }

    bail!("invalid Pico selector `{value}`; use UID, IP, pico2w, picow, rp2350, or rp2040")
}

pub(super) fn setup_probe_selected(selectors: &[PicoSelector], probe: &SetupLabProbe) -> bool {
    selectors.is_empty()
        || selectors.iter().any(|selector| match selector {
            PicoSelector::Uid(uid) => *uid == probe.uid,
            PicoSelector::Board(board) => *board == probe.hello.board_type,
            PicoSelector::Ip(_) => false,
        })
}

pub(super) fn wifi_target_selected(
    selectors: &[PicoSelector],
    target: &cmd_run::PicoTarget,
) -> bool {
    selectors.is_empty()
        || selectors.iter().any(|selector| match selector {
            PicoSelector::Uid(uid) => *uid == target.info.unique_id_short,
            PicoSelector::Board(board) => *board == target.info.board_type,
            PicoSelector::Ip(ip) => *ip == target.peer.ip(),
        })
}

pub(super) fn selected_bootsel_mounts(
    selectors: &[PicoSelector],
    mounts: Vec<(PathBuf, cmd_flash::BootselBoard)>,
) -> Vec<(PathBuf, cmd_flash::BootselBoard)> {
    mounts
        .into_iter()
        .filter(|(_, board)| {
            selectors.is_empty()
                || selectors.iter().any(|selector| match selector {
                    PicoSelector::Board(board_type) => {
                        board_type_matches_bootsel(*board_type, *board)
                    }
                    PicoSelector::Uid(_) | PicoSelector::Ip(_) => false,
                })
        })
        .collect()
}

pub(super) fn board_type_matches_bootsel(board_type: u8, bootsel: cmd_flash::BootselBoard) -> bool {
    matches!(
        (board_type, bootsel),
        (protocol::BOARD_PICO_2_W, cmd_flash::BootselBoard::Rp2350)
            | (
                protocol::BOARD_PICO_W_RP2040,
                cmd_flash::BootselBoard::Rp2040
            )
    )
}

pub(super) fn board_type_label(board_type: u8) -> &'static str {
    match board_type {
        protocol::BOARD_PICO_2_W => "Pico 2 W",
        protocol::BOARD_PICO_W_RP2040 => "Pico W",
        _ => "Pico",
    }
}
