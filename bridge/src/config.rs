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
}

impl Config {
    pub fn remember_pico(&mut self, pico: PicoIdentity) {
        if let Some(existing) = self
            .picos
            .iter_mut()
            .find(|p| p.unique_id_short == pico.unique_id_short)
        {
            *existing = pico.clone();
        } else {
            self.picos.push(pico.clone());
        }
        self.last_pico = Some(pico);
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
}

impl PicoIdentity {
    pub fn uid_hex(&self) -> String {
        format!("{:08X}", self.unique_id_short)
    }

    pub fn board_label(&self) -> &'static str {
        match self.board_type {
            protocol::BOARD_PICO_2_W => "Pico 2 W",
            protocol::BOARD_PICO_W_RP2040 => "Pico W",
            _ => "Pico",
        }
    }

    pub fn firmware_version(&self) -> String {
        format!("{}.{}.{}", self.fw_major, self.fw_minor, self.fw_patch)
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
