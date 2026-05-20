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
    /// Remote-debug tunnel session, if the user has opted in. Holds an
    /// upload (write) token used on every WS reconnect and a view token
    /// used to generate the shareable URL. Tokens rotate on `tunnel reset`.
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Base URL of the tunnel server, e.g. https://couchlink.whyknot.dev .
    pub server: String,
    /// 32-char write token issued by the tunnel server.
    #[serde(default)]
    pub write_token: String,
    /// 32-char view token, used to construct shareable URLs.
    #[serde(default)]
    pub view_token: String,
}

impl TelemetryConfig {
    pub fn ws_url(&self) -> String {
        let base = self.server.trim_end_matches('/');
        let ws = if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            base.to_string()
        };
        format!("{ws}/ws")
    }

    pub fn view_url(&self) -> String {
        let base = self.server.trim_end_matches('/');
        format!("{base}/v/{}", self.view_token)
    }

    #[allow(dead_code)] // available for callers that mint without a TelemetryConfig
    pub fn mint_url(&self) -> String {
        let base = self.server.trim_end_matches('/');
        format!("{base}/api/sessions")
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
