//! Per-user config and data directories, via `directories::ProjectDirs`.
//! Wi-Fi credentials are never persisted here -- they live only in the
//! Pico's flash. This file tracks the last-seen Pico identity and
//! housekeeping state.

use std::fs;
use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::protocol;

const QUALIFIER: &str = "";
const ORGANIZATION: &str = "";
const APPLICATION: &str = "ParsecCouchLink";

pub fn dirs() -> Result<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .context("could not resolve per-user application directories")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(dirs()?.config_dir().join("config.toml"))
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs()?.config_dir().to_path_buf())
}

pub fn log_dir() -> Result<PathBuf> {
    Ok(dirs()?.data_local_dir().join("logs"))
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(dirs()?.data_local_dir().to_path_buf())
}

pub fn diag_cache_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("diagnostics"))
}

/// Directory for panic crash files. Sibling of `log_dir()`.
pub fn crash_dir() -> Result<PathBuf> {
    Ok(log_dir()?
        .parent()
        .ok_or_else(|| anyhow::anyhow!("log_dir has no parent"))?
        .join("crashes"))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Last known Pico identity. Used to warn when a different Pico
    /// appears and to remember the last LAN address as a fast-path
    /// hint (still re-verified via discovery on every start).
    pub last_pico: Option<PicoIdentity>,
    /// Path to the .uf2 used during the most recent flash, if known.
    pub last_uf2: Option<PathBuf>,
    /// Set after setup finishes successfully. Bridge run mode warns if
    /// this is false to nudge the user toward `couchlink setup`.
    pub setup_complete: bool,
    /// Saved Pico inventory shown by the guided home screen. This is
    /// intentionally just device identity and last-known network info;
    /// Wi-Fi credentials remain only on the Pico.
    #[serde(default)]
    pub picos: Vec<PicoIdentity>,
    /// Saved controller-to-Pico layout from the guided runner. The
    /// Startup shortcut uses `couchlink run`, so this lets a multi-Pico
    /// layout start without asking questions every logon.
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    /// Optional external power controls for `couchlink lab`. Each command is
    /// an argv vector: first item is the executable, remaining items are args.
    #[serde(default)]
    pub lab_power: Option<LabPowerConfig>,
}

impl Config {
    pub fn remember_pico(&mut self, mut pico: PicoIdentity) {
        if let Some(existing) = self
            .picos
            .iter_mut()
            .find(|p| p.unique_id_short == pico.unique_id_short)
        {
            // Fresh observations (discovery acks, USB hello) never carry a
            // nickname; keep the user's saved one instead of clobbering it.
            if pico.nickname.is_none() {
                pico.nickname = existing.nickname.clone();
            }
            *existing = pico.clone();
        } else {
            self.picos.push(pico.clone());
        }
        self.last_pico = Some(pico);
    }

    pub fn nickname_for(&self, unique_id_short: u32) -> Option<&str> {
        self.picos
            .iter()
            .find(|p| p.unique_id_short == unique_id_short)?
            .nickname
            .as_deref()
    }

    /// Set or clear a saved Pico's nickname. Returns false when no saved
    /// Pico has that UID.
    pub fn set_nickname(&mut self, unique_id_short: u32, nickname: Option<String>) -> bool {
        let Some(pico) = self
            .picos
            .iter_mut()
            .find(|p| p.unique_id_short == unique_id_short)
        else {
            return false;
        };
        pico.nickname = nickname.clone();
        if let Some(last) = self
            .last_pico
            .as_mut()
            .filter(|p| p.unique_id_short == unique_id_short)
        {
            last.nickname = nickname;
        }
        true
    }

    pub fn forget_pico(&mut self, unique_id_short: u32) {
        self.picos.retain(|p| p.unique_id_short != unique_id_short);
        self.routes.retain(|r| r.pico_uid != unique_id_short);
        if self
            .last_pico
            .as_ref()
            .map(|p| p.unique_id_short == unique_id_short)
            .unwrap_or(false)
        {
            self.last_pico = self.picos.first().cloned();
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PicoIdentity {
    pub unique_id_short: u32,
    pub board_type: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub last_ip: Option<String>,
    pub device_name: Option<String>,
    /// User-assigned name, usually the console this Pico is dedicated to
    /// ("Dreamcast", "N64 player 2"). Shown ahead of the board/UID in the
    /// home screen and pickers.
    #[serde(default)]
    pub nickname: Option<String>,
}

impl PicoIdentity {
    pub fn uid_hex(&self) -> String {
        format!("{:08X}", self.unique_id_short)
    }

    /// "<nickname> - <board> <uid>" when named, else "<board> <uid>".
    pub fn display_title(&self) -> String {
        match self.nickname.as_deref() {
            Some(name) => format!("{name} - {} {}", self.board_label(), self.uid_hex()),
            None => format!("{} {}", self.board_label(), self.uid_hex()),
        }
    }

    pub fn board_label(&self) -> &'static str {
        match self.board_type {
            protocol::BOARD_PICO_2_W => "Pico 2 W",
            protocol::BOARD_PICO_W_RP2040 => "Pico W",
            _ => "Pico",
        }
    }

    /// Last-known firmware for a saved Pico, only shown while it is offline.
    /// Stored as the date-triplet (the discovery ack never carried the build
    /// number), so render it through the shared formatter -- "2026.5.31.x",
    /// build unknown -- rather than a raw "26.5.31" that reads like a
    /// different, lower version.
    pub fn firmware_version(&self) -> String {
        crate::firmware_version::FirmwareVersion::from_triplet(
            self.fw_major,
            self.fw_minor,
            self.fw_patch,
        )
        .to_string()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteConfig {
    /// XInput slot, zero-based internally. The UI displays this as
    /// controller 1 through 4.
    pub source_slot: u32,
    pub pico_uid: u32,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabPowerConfig {
    pub off: Vec<String>,
    pub on: Vec<String>,
    #[serde(default)]
    pub probe: Option<Vec<String>>,
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    match fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).context("parsing config.toml"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn save(cfg: &Config) -> Result<()> {
    ensure_dirs()?;
    let path = config_path()?;
    let s = toml::to_string_pretty(cfg).context("serializing config")?;
    fs::write(&path, s).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn ensure_dirs() -> Result<()> {
    let d = dirs()?;
    fs::create_dir_all(d.config_dir())?;
    fs::create_dir_all(d.data_local_dir())?;
    fs::create_dir_all(log_dir()?)?;
    fs::create_dir_all(diag_cache_dir()?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pico(uid: u32, ip: Option<&str>) -> PicoIdentity {
        PicoIdentity {
            unique_id_short: uid,
            board_type: protocol::BOARD_PICO_2_W,
            fw_major: 26,
            fw_minor: 5,
            fw_patch: 30,
            last_ip: ip.map(|s| s.to_string()),
            device_name: None,
            nickname: None,
        }
    }

    #[test]
    fn remember_pico_upserts_by_uid() {
        let mut cfg = Config::default();
        cfg.remember_pico(pico(0x07D37EB6, Some("192.168.50.226")));
        cfg.remember_pico(pico(0x07D37EB6, Some("192.168.50.227")));

        assert_eq!(cfg.picos.len(), 1);
        assert_eq!(cfg.picos[0].last_ip.as_deref(), Some("192.168.50.227"));
        assert_eq!(cfg.last_pico.as_ref().unwrap().unique_id_short, 0x07D37EB6);
    }

    #[test]
    fn remember_pico_keeps_nickname_when_fresh_observation_has_none() {
        let mut cfg = Config::default();
        cfg.remember_pico(pico(0x07D37EB6, Some("192.168.50.226")));
        assert!(cfg.set_nickname(0x07D37EB6, Some("Dreamcast".to_string())));

        // A rescan rebuilds the identity from the ack, without a nickname.
        cfg.remember_pico(pico(0x07D37EB6, Some("192.168.50.227")));

        assert_eq!(cfg.nickname_for(0x07D37EB6), Some("Dreamcast"));
        assert_eq!(
            cfg.last_pico.as_ref().unwrap().nickname.as_deref(),
            Some("Dreamcast")
        );
        assert_eq!(cfg.picos[0].last_ip.as_deref(), Some("192.168.50.227"));
    }

    #[test]
    fn set_nickname_clears_and_reports_unknown_uid() {
        let mut cfg = Config::default();
        cfg.remember_pico(pico(0x07D37EB6, None));
        assert!(cfg.set_nickname(0x07D37EB6, Some("N64".to_string())));
        assert!(cfg.set_nickname(0x07D37EB6, None));
        assert_eq!(cfg.nickname_for(0x07D37EB6), None);
        assert!(!cfg.set_nickname(0xDEADBEEF, Some("nope".to_string())));
    }

    #[test]
    fn config_survives_toml_round_trip() {
        let mut cfg = Config {
            last_uf2: Some(PathBuf::from("couchlink-pico2w.uf2")),
            setup_complete: true,
            ..Config::default()
        };
        cfg.remember_pico(pico(0x07D37EB6, Some("192.168.50.226")));
        cfg.remember_pico(pico(0x523861E6, None));
        cfg.routes.push(RouteConfig {
            source_slot: 2,
            pico_uid: 0x523861E6,
            label: Some("Pico W".to_string()),
        });
        cfg.lab_power = Some(LabPowerConfig {
            off: vec!["hubctl.exe".to_string(), "off".to_string()],
            on: vec!["hubctl.exe".to_string(), "on".to_string()],
            probe: Some(vec!["hubctl.exe".to_string(), "status".to_string()]),
        });

        let serialized = toml::to_string_pretty(&cfg).unwrap();
        let restored: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(restored.picos, cfg.picos);
        assert_eq!(restored.routes, cfg.routes);
        assert_eq!(restored.last_pico, cfg.last_pico);
        assert_eq!(restored.setup_complete, cfg.setup_complete);
        assert_eq!(restored.last_uf2, cfg.last_uf2);
        assert_eq!(restored.lab_power, cfg.lab_power);
    }

    #[test]
    fn forget_pico_removes_routes_and_last_pico() {
        let mut cfg = Config::default();
        cfg.remember_pico(pico(0x07D37EB6, None));
        cfg.routes.push(RouteConfig {
            source_slot: 0,
            pico_uid: 0x07D37EB6,
            label: None,
        });

        cfg.forget_pico(0x07D37EB6);

        assert!(cfg.picos.is_empty());
        assert!(cfg.routes.is_empty());
        assert!(cfg.last_pico.is_none());
    }
}
