//! Best-effort local diagnostic cache for last-seen Pico state.
//!
//! This is intentionally separate from `config.toml`: config is user
//! preference and saved routing state, while this cache is support evidence
//! that `couchlink bundle` can include even when the Pico is offline later.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::cmd_run::PicoTarget;
use crate::{config, protocol};

const SCHEMA_VERSION: u8 = 1;
const CURRENT_FILE: &str = "pico-state-current.json";
const HISTORY_FILE: &str = "pico-state-history.jsonl";
const HISTORY_MAX_BYTES: u64 = 512 * 1024;
const HISTORY_KEEP_BYTES: usize = 256 * 1024;

static WARNED: OnceLock<()> = OnceLock::new();

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CachedUsbDiag {
    pub verdict: String,
    pub mounted: bool,
    pub suspended: bool,
    pub mount_count: u32,
    pub umount_count: u32,
    pub suspend_count: u32,
    pub resume_count: u32,
    pub device_desc_count: u32,
    pub config_desc_count: u32,
    pub queued_reports: u32,
    pub host_accepted_reports: u32,
    pub host_out_reports: u32,
    pub in_blocked_not_mounted: u32,
    pub in_blocked_not_ready: u32,
    pub in_blocked_short_write: u32,
    pub in_idle_suppressed: u32,
    pub last_mount_ms: u32,
    pub last_in_sent_ms: u32,
    pub last_in_blocked_ms: u32,
    pub last_in_blocked_reason: String,
    pub last_in_blocked_want: u16,
    pub last_in_blocked_got: u16,
    pub last_out_ms: u32,
    pub last_bridge_packet_ms: u32,
    pub bridge_peer: bool,
    pub parsec_connected: bool,
}

impl CachedUsbDiag {
    pub fn from_diag(diag: &protocol::UsbDiag, persona: protocol::Persona) -> Self {
        Self {
            verdict: usb_verdict_label(diag, persona).to_string(),
            mounted: diag.mounted(),
            suspended: diag.suspended(),
            mount_count: diag.mount_count,
            umount_count: diag.umount_count,
            suspend_count: diag.suspend_count,
            resume_count: diag.resume_count,
            device_desc_count: diag.device_desc_count,
            config_desc_count: diag.config_desc_count,
            queued_reports: diag.xinput_in_queued_count,
            host_accepted_reports: diag.xinput_in_sent_count,
            host_out_reports: diag.xinput_out_count,
            in_blocked_not_mounted: diag.xinput_in_blocked_not_mounted_count,
            in_blocked_not_ready: diag.xinput_in_blocked_not_ready_count,
            in_blocked_short_write: diag.xinput_in_blocked_short_write_count,
            in_idle_suppressed: diag.xinput_in_idle_suppressed_count,
            last_mount_ms: diag.last_mount_ms,
            last_in_sent_ms: diag.last_in_sent_ms,
            last_in_blocked_ms: diag.last_in_blocked_ms,
            last_in_blocked_reason: protocol::usb_in_blocked_reason_label(
                diag.last_in_blocked_reason,
            )
            .to_string(),
            last_in_blocked_want: diag.last_in_blocked_want,
            last_in_blocked_got: diag.last_in_blocked_got,
            last_out_ms: diag.last_out_ms,
            last_bridge_packet_ms: diag.last_bridge_packet_ms,
            bridge_peer: diag.bridge_peer_latched(),
            parsec_connected: diag.parsec_connected(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteSnapshot {
    pub source_slot: Option<u32>,
    pub source_label: Option<String>,
    pub peer_health: Option<String>,
    pub sent_total: Option<u64>,
    pub inbound_total: Option<u64>,
    pub sent_delta: Option<u64>,
    pub last_inbound_ms_ago: Option<u64>,
    pub source_connected: Option<bool>,
    pub last_send_type: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PicoStateSnapshot {
    pub schema_version: u8,
    pub captured_at: String,
    pub source: String,
    pub uid: Option<String>,
    pub unique_id_short: Option<u32>,
    pub board_type: Option<u8>,
    pub board_label: Option<String>,
    pub firmware: Option<String>,
    pub protocol: Option<u8>,
    pub ip: Option<String>,
    pub peer: Option<String>,
    pub persona: Option<String>,
    pub ack_flags: Option<String>,
    pub uptime_seconds: Option<u32>,
    pub route: Option<RouteSnapshot>,
    pub usb_diag: Option<CachedUsbDiag>,
    pub pico_state: Option<BTreeMap<String, serde_json::Value>>,
    pub capture_outcome: Option<String>,
    pub notes: Vec<String>,
}

impl PicoStateSnapshot {
    pub fn from_target(source: impl Into<String>, pico: &PicoTarget) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            captured_at: Local::now().to_rfc3339(),
            source: source.into(),
            uid: Some(pico.uid_hex()),
            unique_id_short: Some(pico.info.unique_id_short),
            board_type: Some(pico.info.board_type),
            board_label: Some(pico.board_label().to_string()),
            firmware: Some(pico.info.firmware_version().to_string()),
            protocol: Some(pico.info.proto_version),
            ip: Some(pico.peer.ip().to_string()),
            peer: Some(pico.peer.to_string()),
            persona: Some(pico.persona.label().to_string()),
            ack_flags: Some(format!("0x{:02X}", pico.ack_flags)),
            uptime_seconds: Some(pico.info.uptime_seconds),
            route: None,
            usb_diag: None,
            pico_state: None,
            capture_outcome: None,
            notes: Vec::new(),
        }
    }

    pub fn offline_from_config(source: impl Into<String>, pico: &config::PicoIdentity) -> Self {
        let peer = pico
            .last_ip
            .as_deref()
            .map(|ip| format!("{}:{}", ip, protocol::PORT));
        Self {
            schema_version: SCHEMA_VERSION,
            captured_at: Local::now().to_rfc3339(),
            source: source.into(),
            uid: Some(pico.uid_hex()),
            unique_id_short: Some(pico.unique_id_short),
            board_type: Some(pico.board_type),
            board_label: Some(pico.board_label().to_string()),
            firmware: Some(pico.firmware_version()),
            protocol: None,
            ip: pico.last_ip.clone(),
            peer,
            persona: None,
            ack_flags: None,
            uptime_seconds: None,
            route: None,
            usb_diag: None,
            pico_state: None,
            capture_outcome: Some("cached_offline_identity".to_string()),
            notes: vec![
                "Pico was not reachable in this capture; values came from saved identity.".into(),
            ],
        }
    }

    pub fn with_route(mut self, route: RouteSnapshot) -> Self {
        self.route = Some(route);
        self
    }

    pub fn with_usb_diag(mut self, diag: &protocol::UsbDiag, persona: protocol::Persona) -> Self {
        self.usb_diag = Some(CachedUsbDiag::from_diag(diag, persona));
        self
    }

    pub fn with_pico_state(mut self, state: &protocol::PicoStateDiag) -> Self {
        self.pico_state = Some(state.to_json_map());
        self
    }

    pub fn with_outcome(mut self, outcome: impl Into<String>) -> Self {
        self.capture_outcome = Some(outcome.into());
        self
    }
}

pub fn record_target(source: &str, pico: &PicoTarget) {
    record(PicoStateSnapshot::from_target(source, pico));
}

pub fn record(snapshot: PicoStateSnapshot) {
    if let Err(e) = record_inner(snapshot) {
        warn_once(format!(
            "pico-cache: could not write diagnostic cache: {e:#}"
        ));
    }
}

pub fn current_path() -> Option<PathBuf> {
    config::diag_cache_dir().ok().map(|d| d.join(CURRENT_FILE))
}

pub fn history_path() -> Option<PathBuf> {
    config::diag_cache_dir().ok().map(|d| d.join(HISTORY_FILE))
}

pub fn read_current() -> Option<String> {
    fs::read_to_string(current_path()?).ok()
}

pub fn read_history() -> Option<String> {
    fs::read_to_string(history_path()?).ok()
}

fn record_inner(snapshot: PicoStateSnapshot) -> anyhow::Result<()> {
    config::ensure_dirs()?;
    trim_history_if_needed();
    let json = serde_json::to_string_pretty(&snapshot)?;
    if let Some(path) = current_path() {
        fs::write(path, json)?;
    }
    if let Some(path) = history_path() {
        let line = serde_json::to_string(&snapshot)?;
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
    }
    Ok(())
}

fn trim_history_if_needed() {
    let Some(path) = history_path() else {
        return;
    };
    let Ok(meta) = fs::metadata(&path) else {
        return;
    };
    if meta.len() <= HISTORY_MAX_BYTES {
        return;
    }
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    let start = bytes.len().saturating_sub(HISTORY_KEEP_BYTES);
    let start = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| start + i + 1)
        .unwrap_or(start);
    let _ = fs::write(path, &bytes[start..]);
}

pub fn duration_ms(d: Duration) -> u64 {
    d.as_millis().min(u128::from(u64::MAX)) as u64
}

fn warn_once(message: String) {
    if WARNED.set(()).is_ok() {
        tracing::warn!("{message}");
    }
}

fn usb_verdict_label(diag: &protocol::UsbDiag, persona: protocol::Persona) -> &'static str {
    match diag.configuration_state() {
        protocol::UsbConfigurationState::NoHostTraffic => return "no_usb_host_traffic",
        protocol::UsbConfigurationState::EnumerationStarted => {
            return "enumeration_started_not_configured"
        }
        protocol::UsbConfigurationState::ConfiguredThenUnmounted => {
            return "configured_then_unmounted"
        }
        protocol::UsbConfigurationState::ConfiguredThenUnmountedWithoutCallback => {
            return "configured_then_unmounted_without_callback"
        }
        protocol::UsbConfigurationState::Suspended => return "configured_suspended",
        protocol::UsbConfigurationState::Configured => {}
    }

    if !diag.xinput_report_sent() {
        match diag.last_in_blocked_reason {
            protocol::USB_DIAG_IN_BLOCKED_NOT_MOUNTED => "configured_report_blocked_not_mounted",
            protocol::USB_DIAG_IN_BLOCKED_NOT_READY => "configured_report_blocked_not_ready",
            protocol::USB_DIAG_IN_BLOCKED_SHORT_WRITE => "configured_report_blocked_short_write",
            _ => "configured_no_report_accepted",
        }
    } else if diag.xinput_out_seen() {
        match persona {
            protocol::Persona::Keyboard => "polling_with_keyboard_out",
            protocol::Persona::Debug => "polling_with_debug_out",
            protocol::Persona::N64 => "polling_with_n64_diag_out",
            protocol::Persona::N64UsbC => "polling_with_n64_usbc_diag_out",
            _ => "polling_with_out",
        }
    } else {
        "polling_no_out"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_snapshot_keeps_identity_without_secrets() {
        let pico = config::PicoIdentity {
            unique_id_short: 0x02E22DA9,
            board_type: protocol::BOARD_PICO_2_W,
            fw_major: 26,
            fw_minor: 6,
            fw_patch: 15,
            last_ip: Some("10.0.0.16".to_string()),
            device_name: Some("Pico 2 W".to_string()),
        };

        let snap = PicoStateSnapshot::offline_from_config("test", &pico);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("02E22DA9"));
        assert!(json.contains("cached_offline_identity"));
        assert!(!json.to_ascii_lowercase().contains("password"));
        assert!(!json.to_ascii_lowercase().contains("ssid"));
    }

    fn usb_diag(mounted: bool, desc: bool) -> protocol::UsbDiag {
        protocol::UsbDiag {
            seq: 1,
            flags: 0,
            version: protocol::USB_DIAG_VERSION,
            usb_flags: if mounted {
                protocol::USB_DIAG_FLAG_MOUNTED
            } else {
                0
            },
            activity_flags: 0,
            last_out_len: 0,
            now_ms: 10_000,
            last_bridge_packet_ms: 0,
            mount_count: if mounted { 1 } else { 0 },
            umount_count: 0,
            suspend_count: 0,
            resume_count: 0,
            device_desc_count: if desc { 1 } else { 0 },
            config_desc_count: if desc { 1 } else { 0 },
            xinput_in_queued_count: 0,
            xinput_in_sent_count: 0,
            xinput_out_count: 0,
            xinput_in_blocked_not_mounted_count: 0,
            xinput_in_blocked_not_ready_count: 0,
            xinput_in_blocked_short_write_count: 0,
            xinput_in_idle_suppressed_count: 0,
            last_mount_ms: 9000,
            last_umount_ms: 0,
            last_in_queued_ms: 0,
            last_in_sent_ms: 0,
            last_out_ms: 0,
            last_in_blocked_ms: 0,
            last_in_blocked_reason: 0,
            last_in_blocked_want: 0,
            last_in_blocked_got: 0,
            last_out_byte0: 0,
            last_out_byte1: 0,
        }
    }

    #[test]
    fn usb_verdict_label_keeps_dropped_after_mount_distinct() {
        let mut diag = usb_diag(false, true);
        diag.mount_count = 1;

        assert_eq!(
            usb_verdict_label(&diag, protocol::Persona::Ps3),
            "configured_then_unmounted_without_callback"
        );

        diag.umount_count = 1;
        assert_eq!(
            usb_verdict_label(&diag, protocol::Persona::Ps3),
            "configured_then_unmounted"
        );
    }
}
