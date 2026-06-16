//! manifest.json assembly and the Windows version probe.

use anyhow::Result;
use chrono::Local;
use serde::Serialize;

use crate::{config, logfile};

use super::collect::BUNDLE_LOG_FILES_PER_PREFIX;

pub(super) const BUNDLE_SCHEMA_VERSION: u8 = 13;

#[derive(Clone, Debug, Serialize)]
pub(super) struct ManifestPicoCapture {
    pub uid: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    pub live: bool,
    pub source: String,
    pub state_captured: bool,
    pub pico_diag_status: String,
    pub usb_diag_status: String,
    pub pico_state_status: String,
    pub usb_packet_dump_status: String,
    pub usb_packet_dump_count: usize,
    pub cached_state_included: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ManifestHostSnapshot {
    pub name: String,
    pub path: String,
    pub captured: bool,
    pub bytes: usize,
    pub status: String,
}

#[derive(Serialize)]
pub(super) struct Manifest {
    bundle_schema_version: u8,
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
    pico_usb_diag_captured: bool,
    pico_usb_diag_target_count: usize,
    app_log_retention_count: usize,
    bundled_log_files_per_prefix: usize,
    debug_packet_log_retention_count: usize,
    retained_debug_packet_count: usize,
    usb_packet_summary_included: bool,
    usb_packet_summary_path: &'static str,
    usb_packet_records_included: bool,
    usb_packet_records_path: &'static str,
    usb_control_transfers_included: bool,
    usb_control_transfers_path: &'static str,
    usb_hid_reports_included: bool,
    usb_hid_reports_path: &'static str,
    usb_packet_timeline_included: bool,
    usb_packet_timeline_path: &'static str,
    debug_capture_verdict_included: bool,
    debug_capture_verdict_path: &'static str,
    debug_capture_evidence_included: bool,
    debug_capture_evidence_path: &'static str,
    diagnostic_cache_included: bool,
    retained_debug_packet_logs: Vec<String>,
    per_pico_capture_outcomes: Vec<ManifestPicoCapture>,
    host_snapshots: Vec<ManifestHostSnapshot>,
    capture_policy_notes: Vec<&'static str>,
    redaction_policy: Vec<&'static str>,
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
    pico_usb_diag_captured: bool,
    pico_usb_diag_target_count: usize,
    retained_debug_packet_logs: &[String],
    retained_debug_packet_count: usize,
    diagnostic_cache_included: bool,
    per_pico_capture_outcomes: &[ManifestPicoCapture],
    host_snapshots: &[ManifestHostSnapshot],
    crash_files: &[String],
    setup_transcripts: &[String],
) -> Result<Manifest> {
    let cfg = config::load().unwrap_or_default();
    Ok(Manifest {
        bundle_schema_version: BUNDLE_SCHEMA_VERSION,
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
        pico_usb_diag_captured,
        pico_usb_diag_target_count,
        app_log_retention_count: logfile::LOG_FILE_RETENTION,
        bundled_log_files_per_prefix: BUNDLE_LOG_FILES_PER_PREFIX,
        debug_packet_log_retention_count: crate::debug_packets::DEBUG_PACKET_FILE_RETENTION,
        retained_debug_packet_count,
        usb_packet_summary_included: true,
        usb_packet_summary_path: "usb-packets-summary.json",
        usb_packet_records_included: true,
        usb_packet_records_path: "usb-packets.jsonl",
        usb_control_transfers_included: true,
        usb_control_transfers_path: "usb-control-transfers.txt",
        usb_hid_reports_included: true,
        usb_hid_reports_path: "usb-hid-reports.txt",
        usb_packet_timeline_included: true,
        usb_packet_timeline_path: "usb-packet-timeline.txt",
        debug_capture_verdict_included: true,
        debug_capture_verdict_path: "debug-capture-verdict.txt",
        debug_capture_evidence_included: true,
        debug_capture_evidence_path: "debug-capture-evidence.json",
        diagnostic_cache_included,
        retained_debug_packet_logs: retained_debug_packet_logs.to_vec(),
        per_pico_capture_outcomes: per_pico_capture_outcomes.to_vec(),
        host_snapshots: host_snapshots.to_vec(),
        capture_policy_notes: vec![
            "Bundle capture is best-effort and non-interactive.",
            "Run-mode Pico boards are queried over LAN from the bridge PC.",
            "Setup-mode Pico boards are queried over USB CDC and WinUSB vendor diagnostics when available.",
            "BOOTSEL drives are inventoried when present.",
            "Offline Pico boards are represented from the local diagnostic cache and saved config when available.",
            "Debug input mode uses the XInput USB shape and logs raw USB IN/OUT packet samples for adapter reverse engineering.",
            "While debug input mode is streaming, the bridge periodically drains the Pico diag ring into retained host packet logs so later bundles can include them.",
            "When bundle finds a live Pico already in debug input mode, it performs a bundle-time GET_LOG harvest and records the harvest health in that Pico's usb-packets.txt.",
            "Retained debug packet logs include per-harvest health records for GET_LOG duration, chunks, lost bytes, packet counts, and failures.",
            "usb-packets-summary.json summarizes packet directions, sources, reasons, sequence gaps, truncation, firmware packet-stat checkpoints, and debug harvest chunk health.",
            "usb-packets.jsonl normalizes each packet/stat line for reverse-engineering tools, including decoded USB setup and HID report metadata where present.",
            "usb-control-transfers.txt extracts setup and control-IN traffic into a compact transcript with decoded setup request names.",
            "usb-hid-reports.txt extracts HID report ids and report types from HID OUT/FEATURE payloads and HID GET_REPORT/SET_REPORT setup requests.",
            "usb-packet-timeline.txt extracts packet, stats, and harvest records in timestamp order with per-source timing deltas.",
            "debug-capture-verdict.txt explains whether the bundle contains enough debug input packet evidence for adapter reverse engineering.",
            "debug-capture-evidence.json exposes the same debug capture gate and per-source evidence counts in machine-readable form.",
        ],
        redaction_policy: vec![
            "Wi-Fi passwords are not included.",
            "SSID values are redacted from bundle text.",
            "Key/value fields named password, pass, ssid, token, secret, authorization, api_key, or apikey are redacted.",
            "Lengths, failure codes, timings, local IPs, device names, driver names, firmware IDs, and filesystem paths may be included for diagnosis.",
            "Raw per-key keyboard traces require trace logging and are not enabled by default.",
            "Raw USB packet dumps are only captured when the Pico is deliberately switched into debug input mode.",
            "Retained debug packet logs are redacted by the same bundle filter before they are written into the ZIP.",
        ],
        crash_files: crash_files.to_vec(),
        setup_transcripts: setup_transcripts.to_vec(),
        notes: vec![
            "Wi-Fi credentials are NOT included.",
            "SSID is NOT included.",
            "Logs are filtered to the last 5 rotated files per prefix.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manifest_includes_diagnostics_schema_fields() {
        let pico = ManifestPicoCapture {
            uid: "02E22DA9".to_string(),
            path: "picos/02E22DA9".to_string(),
            peer: Some("10.0.0.24:4242".to_string()),
            live: true,
            source: "broadcast discovery".to_string(),
            state_captured: true,
            pico_diag_status: "captured".to_string(),
            usb_diag_status: "captured".to_string(),
            pico_state_status: "captured".to_string(),
            usb_packet_dump_status: "captured".to_string(),
            usb_packet_dump_count: 2,
            cached_state_included: true,
        };
        let host = ManifestHostSnapshot {
            name: "network_routes".to_string(),
            path: "host/network-routes.txt".to_string(),
            captured: true,
            bytes: 123,
            status: "captured in 1 ms".to_string(),
        };

        let manifest = build_manifest(
            true,
            0,
            "captured",
            Some("run-udp"),
            true,
            "pnputil",
            true,
            true,
            Some("run"),
            true,
            1,
            &["usb-packets-20260615-214000-02E22DA9.log".to_string()],
            7,
            true,
            &[pico],
            &[host],
            &[],
            &[],
        )
        .await
        .unwrap();
        let json = serde_json::to_value(&manifest).unwrap();

        assert_eq!(json["bundle_schema_version"], BUNDLE_SCHEMA_VERSION);
        assert_eq!(
            json["bundled_log_files_per_prefix"],
            BUNDLE_LOG_FILES_PER_PREFIX
        );
        assert_eq!(json["app_log_retention_count"], logfile::LOG_FILE_RETENTION);
        assert_eq!(
            json["debug_packet_log_retention_count"],
            crate::debug_packets::DEBUG_PACKET_FILE_RETENTION
        );
        assert_eq!(json["diagnostic_cache_included"], true);
        assert_eq!(
            json["retained_debug_packet_logs"][0],
            "usb-packets-20260615-214000-02E22DA9.log"
        );
        assert_eq!(json["retained_debug_packet_count"], 7);
        assert_eq!(json["usb_packet_summary_included"], true);
        assert_eq!(json["usb_packet_summary_path"], "usb-packets-summary.json");
        assert_eq!(json["usb_packet_records_included"], true);
        assert_eq!(json["usb_packet_records_path"], "usb-packets.jsonl");
        assert_eq!(json["usb_control_transfers_included"], true);
        assert_eq!(
            json["usb_control_transfers_path"],
            "usb-control-transfers.txt"
        );
        assert_eq!(json["usb_hid_reports_included"], true);
        assert_eq!(json["usb_hid_reports_path"], "usb-hid-reports.txt");
        assert_eq!(json["usb_packet_timeline_included"], true);
        assert_eq!(json["usb_packet_timeline_path"], "usb-packet-timeline.txt");
        assert_eq!(json["debug_capture_verdict_included"], true);
        assert_eq!(
            json["debug_capture_verdict_path"],
            "debug-capture-verdict.txt"
        );
        assert_eq!(json["debug_capture_evidence_included"], true);
        assert_eq!(
            json["debug_capture_evidence_path"],
            "debug-capture-evidence.json"
        );
        assert_eq!(json["per_pico_capture_outcomes"][0]["uid"], "02E22DA9");
        assert_eq!(
            json["per_pico_capture_outcomes"][0]["usb_packet_dump_count"],
            2
        );
        assert_eq!(json["host_snapshots"][0]["path"], "host/network-routes.txt");
        assert!(json["redaction_policy"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry.as_str().unwrap().contains("SSID values are redacted")));
    }
}
