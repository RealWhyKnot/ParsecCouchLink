//! manifest.json assembly and the Windows version probe.

use anyhow::Result;
use chrono::Local;
use serde::Serialize;

use crate::config;

#[derive(Serialize)]
pub(super) struct Manifest {
    bridge_version: &'static str,
    protocol_version: u8,
    cdc_protocol_version: u8,
    generated_at: String,
    os: String,
    windows_version: Option<String>,
    last_pico: Option<config::PicoIdentity>,
    setup_complete: bool,
    config_path: String,
    log_dir: String,
    report_url: String,
    pico_diag_captured: bool,
    pico_diag_lost_bytes: u32,
    pico_diag_outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pico_diag_source: Option<String>,
    usb_devices_captured: bool,
    usb_capture_method: String,
    usb_events_captured: bool,
    pico_usb_enumerated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pico_usb_mode: Option<String>,
    crash_files: Vec<String>,
    setup_transcripts: Vec<String>,
    notes: Vec<&'static str>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn build_manifest(
    pico_diag_captured: bool,
    pico_diag_lost_bytes: u32,
    pico_diag_outcome: &str,
    pico_diag_source: Option<&str>,
    usb_devices_captured: bool,
    usb_capture_method: &str,
    usb_events_captured: bool,
    pico_usb_enumerated: bool,
    pico_usb_mode: Option<&str>,
    crash_files: &[String],
    setup_transcripts: &[String],
) -> Result<Manifest> {
    let cfg = config::load().unwrap_or_default();
    Ok(Manifest {
        bridge_version: env!("CARGO_PKG_VERSION"),
        protocol_version: crate::protocol::PROTO_VERSION,
        cdc_protocol_version: crate::cdc::PROTO_VERSION,
        generated_at: Local::now().to_rfc3339(),
        os: std::env::consts::OS.to_string(),
        windows_version: windows_version().await,
        last_pico: cfg.last_pico,
        setup_complete: cfg.setup_complete,
        config_path: config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        log_dir: config::log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        report_url: crate::support::issue_url(),
        pico_diag_captured,
        pico_diag_lost_bytes,
        pico_diag_outcome: pico_diag_outcome.to_string(),
        pico_diag_source: pico_diag_source.map(|s| s.to_string()),
        usb_devices_captured,
        usb_capture_method: usb_capture_method.to_string(),
        usb_events_captured,
        pico_usb_enumerated,
        pico_usb_mode: pico_usb_mode.map(|s| s.to_string()),
        crash_files: crash_files.to_vec(),
        setup_transcripts: setup_transcripts.to_vec(),
        notes: vec![
            "Wi-Fi credentials are NOT included.",
            "SSID is NOT included.",
            "Logs are filtered to the last 3 rotated files per prefix.",
        ],
    })
}

#[cfg(windows)]
pub(super) async fn windows_version() -> Option<String> {
    let out = tokio::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(not(windows))]
pub(super) async fn windows_version() -> Option<String> {
    None
}
