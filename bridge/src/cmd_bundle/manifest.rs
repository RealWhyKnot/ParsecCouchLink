use anyhow::Result;
use chrono::Local;
use serde::Serialize;

use crate::{config, logfile};

use super::collect::BUNDLE_LOG_FILES_PER_PREFIX;
use super::flags::{Capture, Inclusion};

pub(super) const BUNDLE_SCHEMA_VERSION: u8 = 23;

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
    pub bluetooth_report_status: String,
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
    pico_diag_captured: Capture,
    pico_diag_lost_bytes: u32,
    pico_diag_outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pico_diag_source: Option<String>,
    usb_devices_captured: Capture,
    usb_capture_method: String,
    usb_events_captured: Capture,
    pico_usb_enumerated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pico_usb_mode: Option<String>,
    pico_usb_diag_captured: Capture,
    pico_usb_diag_target_count: usize,
    adapter_connection_included: Inclusion,
    adapter_connection_path: &'static str,
    adapter_connection_json_included: Inclusion,
    adapter_connection_json_path: &'static str,
    initial_usb_capture_included: Inclusion,
    initial_usb_capture_path: &'static str,
    adapter_survey_included: Inclusion,
    adapter_survey_path: &'static str,
    adapter_survey_json_included: Inclusion,
    adapter_survey_json_path: &'static str,
    bluetooth_report_included: Inclusion,
    bluetooth_report_path: &'static str,
    bluetooth_report_json_included: Inclusion,
    bluetooth_report_json_path: &'static str,
    app_log_retention_count: usize,
    bundled_log_files_per_prefix: usize,
    debug_packet_log_retention_count: usize,
    retained_debug_packet_count: usize,
    usb_packet_summary_included: Inclusion,
    usb_packet_summary_path: &'static str,
    usb_packet_records_included: Inclusion,
    usb_packet_records_path: &'static str,
    usb_control_transfers_included: Inclusion,
    usb_control_transfers_path: &'static str,
    usb_hid_reports_included: Inclusion,
    usb_hid_reports_path: &'static str,
    usb_packet_timeline_included: Inclusion,
    usb_packet_timeline_path: &'static str,
    usb_enumeration_analysis_included: Inclusion,
    usb_enumeration_analysis_path: &'static str,
    debug_capture_verdict_included: Inclusion,
    debug_capture_verdict_path: &'static str,
    debug_capture_evidence_included: Inclusion,
    debug_capture_evidence_path: &'static str,
    diagnostic_cache_included: Inclusion,
    retained_debug_packet_logs: Vec<String>,
    per_pico_capture_outcomes: Vec<ManifestPicoCapture>,
    host_snapshots: Vec<ManifestHostSnapshot>,
    capture_policy_notes: Vec<&'static str>,
    redaction_policy: Vec<&'static str>,
    crash_files: Vec<String>,
    setup_transcripts: Vec<String>,
    notes: Vec<&'static str>,
}

pub(super) struct ManifestInput<'a> {
    pub(super) pico_diag_captured: Capture,
    pub(super) pico_diag_lost_bytes: u32,
    pub(super) pico_diag_outcome: &'a str,
    pub(super) pico_diag_source: Option<&'a str>,
    pub(super) usb_devices_captured: Capture,
    pub(super) usb_capture_method: &'a str,
    pub(super) usb_events_captured: Capture,
    pub(super) pico_usb_enumerated: bool,
    pub(super) pico_usb_mode: Option<&'a str>,
    pub(super) pico_usb_diag_captured: Capture,
    pub(super) pico_usb_diag_target_count: usize,
    pub(super) retained_debug_packet_logs: &'a [String],
    pub(super) retained_debug_packet_count: usize,
    pub(super) diagnostic_cache_included: Inclusion,
    pub(super) per_pico_capture_outcomes: &'a [ManifestPicoCapture],
    pub(super) host_snapshots: &'a [ManifestHostSnapshot],
    pub(super) crash_files: &'a [String],
    pub(super) setup_transcripts: &'a [String],
}

pub(super) async fn build_manifest(input: &ManifestInput<'_>) -> Result<Manifest> {
    let cfg = config::load().unwrap_or_default();
    Ok(Manifest {
        bundle_schema_version: BUNDLE_SCHEMA_VERSION,
        bridge_version: crate::VERSION,
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
        pico_diag_captured: input.pico_diag_captured,
        pico_diag_lost_bytes: input.pico_diag_lost_bytes,
        pico_diag_outcome: input.pico_diag_outcome.to_string(),
        pico_diag_source: input.pico_diag_source.map(std::string::ToString::to_string),
        usb_devices_captured: input.usb_devices_captured,
        usb_capture_method: input.usb_capture_method.to_string(),
        usb_events_captured: input.usb_events_captured,
        pico_usb_enumerated: input.pico_usb_enumerated,
        pico_usb_mode: input.pico_usb_mode.map(std::string::ToString::to_string),
        pico_usb_diag_captured: input.pico_usb_diag_captured,
        pico_usb_diag_target_count: input.pico_usb_diag_target_count,
        adapter_connection_included: Inclusion::Included,
        adapter_connection_path: "adapter-connection.txt",
        adapter_connection_json_included: Inclusion::Included,
        adapter_connection_json_path: "adapter-connection.json",
        initial_usb_capture_included: Inclusion::Included,
        initial_usb_capture_path: "initial-usb-capture.txt",
        adapter_survey_included: Inclusion::Included,
        adapter_survey_path: "adapter-survey.txt",
        adapter_survey_json_included: Inclusion::Included,
        adapter_survey_json_path: "adapter-survey.json",
        bluetooth_report_included: Inclusion::Included,
        bluetooth_report_path: "bluetooth-report.txt",
        bluetooth_report_json_included: Inclusion::Included,
        bluetooth_report_json_path: "bluetooth-report.json",
        app_log_retention_count: logfile::LOG_FILE_RETENTION,
        bundled_log_files_per_prefix: BUNDLE_LOG_FILES_PER_PREFIX,
        debug_packet_log_retention_count: crate::debug_packets::DEBUG_PACKET_FILE_RETENTION,
        retained_debug_packet_count: input.retained_debug_packet_count,
        usb_packet_summary_included: Inclusion::Included,
        usb_packet_summary_path: "usb-packets-summary.json",
        usb_packet_records_included: Inclusion::Included,
        usb_packet_records_path: "usb-packets.jsonl",
        usb_control_transfers_included: Inclusion::Included,
        usb_control_transfers_path: "usb-control-transfers.txt",
        usb_hid_reports_included: Inclusion::Included,
        usb_hid_reports_path: "usb-hid-reports.txt",
        usb_packet_timeline_included: Inclusion::Included,
        usb_packet_timeline_path: "usb-packet-timeline.txt",
        usb_enumeration_analysis_included: Inclusion::Included,
        usb_enumeration_analysis_path: "usb-enumeration-analysis.txt",
        debug_capture_verdict_included: Inclusion::Included,
        debug_capture_verdict_path: "debug-capture-verdict.txt",
        debug_capture_evidence_included: Inclusion::Included,
        debug_capture_evidence_path: "debug-capture-evidence.json",
        diagnostic_cache_included: input.diagnostic_cache_included,
        retained_debug_packet_logs: input.retained_debug_packet_logs.to_vec(),
        per_pico_capture_outcomes: input.per_pico_capture_outcomes.to_vec(),
        host_snapshots: input.host_snapshots.to_vec(),
        capture_policy_notes: capture_policy_notes(),
        redaction_policy: redaction_policy(),
        crash_files: input.crash_files.to_vec(),
        setup_transcripts: input.setup_transcripts.to_vec(),
        notes: vec![
            "Wi-Fi credentials are NOT included.",
            "SSID is NOT included.",
            "Logs are filtered to the last 5 rotated files per prefix.",
        ],
    })
}

fn capture_policy_notes() -> Vec<&'static str> {
    vec![
        "Bundle capture is best-effort and non-interactive.",
        "Run-mode Pico boards are queried over LAN from the bridge PC.",
        "Setup-mode Pico boards are queried over USB CDC and WinUSB vendor diagnostics when available.",
        "BOOTSEL drives are inventoried when present.",
        "Offline Pico boards are represented from the local diagnostic cache and saved config when available.",
        "adapter-connection.txt warns when live Pico USB counters show no console/adapter USB host enumeration traffic.",
        "initial-usb-capture.txt preserves USB packet lines harvested before bundle switches personas.",
        "adapter-survey.txt and adapter-survey.json record live persona checks for PS3, generic HID, PS4, keyboard, XInput, Xbox One, and Maple shapes without prompting, including expected, attempted, missing, failed, coverage_status, and stop_reason fields.",
        "Bundle restores the original persona after the adapter survey pass.",
        "Bluetooth mode is captured separately in bluetooth-report.txt and bluetooth-report.json because the Pico stays plugged into the PC and receives live controller input over USB CDC.",
        "Bluetooth mode does not use the USB adapter survey; its live controller packet path is PC USB CDC to Pico, then Bluetooth HID to the paired receiver.",
        "Debug input mode uses the XInput USB shape; debug evidence is not treated as proof that a HID persona works with an adapter.",
        "When a surveyed persona gets descriptor traffic but does not configure, bundle requests a one-shot USB packet capture boot for that same persona.",
        "If persona switching, USB diagnostic queries, packet capture, or persona restore cannot complete, bundle-capture.txt records the failed step, duration, and reason.",
        "While debug input mode is streaming, the bridge periodically drains the Pico diag ring into retained host packet logs so later bundles can include them.",
        "When bundle finds a live Pico already in debug input mode, it performs a bundle-time GET_LOG harvest and records the harvest health in that Pico's usb-packets.txt.",
        "Retained debug packet logs include per-harvest health records for GET_LOG duration, chunks, lost bytes, packet counts, and failures.",
        "usb-packets-summary.json summarizes packet directions, sources, reasons, lifecycle events, sequence gaps, truncation, firmware packet-stat checkpoints, debug harvest chunk health, and decoded setup/control behavior maps.",
        "usb-packets.jsonl normalizes each packet/event/stat line for reverse-engineering tools, including decoded USB setup and HID report metadata where present.",
        "usb-control-transfers.txt extracts setup and control-IN traffic into a compact transcript with decoded setup request names.",
        "usb-hid-reports.txt extracts HID report ids and report types from HID OUT/FEATURE payloads and HID GET_REPORT/SET_REPORT setup requests.",
        "usb-packet-timeline.txt extracts packet, lifecycle/first-traffic event, stats, and harvest records in timestamp order with per-source timing deltas.",
        "usb-enumeration-analysis.txt turns packet and lifecycle captures into a phase checklist for descriptor fetches, USB identity, mount state, first accepted IN/OUT traffic, configuration, vendor probes, HID reports, and endpoint traffic.",
        "debug-capture-verdict.txt explains whether the bundle contains enough debug input packet evidence for adapter reverse engineering.",
        "debug-capture-evidence.json exposes the same debug capture gate, lossiness, and per-source evidence counts in machine-readable form.",
    ]
}

fn redaction_policy() -> Vec<&'static str> {
    vec![
        "Wi-Fi passwords are not included.",
        "SSID values are redacted from bundle text.",
        "Key/value fields named password, pass, ssid, token, secret, authorization, api_key, apikey, hostname, computername, username, user_name, or userprofile are redacted.",
        "Private LAN IP addresses, MAC addresses, Windows user profile paths, hostnames, and usernames are redacted from bundle text.",
        "Lengths, ports, failure codes, timings, device names, driver names, and firmware IDs may be included for diagnosis.",
        "Raw per-key keyboard traces require trace logging and are not enabled by default.",
        "Raw USB packet dumps are captured only in debug input mode or a bundle-requested one-shot USB capture boot.",
        "Retained debug packet logs are redacted by the same bundle filter before they are written into the ZIP.",
    ]
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
            bluetooth_report_status: "not_applicable".to_string(),
            cached_state_included: true,
        };
        let host = ManifestHostSnapshot {
            name: "network_routes".to_string(),
            path: "host/network-routes.txt".to_string(),
            captured: true,
            bytes: 123,
            status: "captured in 1 ms".to_string(),
        };

        let manifest = build_manifest(&ManifestInput {
            pico_diag_captured: Capture::Captured,
            pico_diag_lost_bytes: 0,
            pico_diag_outcome: "captured",
            pico_diag_source: Some("run-udp"),
            usb_devices_captured: Capture::Captured,
            usb_capture_method: "pnputil",
            usb_events_captured: Capture::Captured,
            pico_usb_enumerated: true,
            pico_usb_mode: Some("run"),
            pico_usb_diag_captured: Capture::Captured,
            pico_usb_diag_target_count: 1,
            retained_debug_packet_logs: &["usb-packets-20260615-214000-02E22DA9.log".to_string()],
            retained_debug_packet_count: 7,
            diagnostic_cache_included: Inclusion::Included,
            per_pico_capture_outcomes: &[pico],
            host_snapshots: &[host],
            crash_files: &[],
            setup_transcripts: &[],
        })
        .await
        .unwrap();
        let json = serde_json::to_value(&manifest).unwrap();

        assert_report_paths(&json);
        assert_packet_outputs_and_policy(&json);
    }

    fn assert_report_paths(json: &serde_json::Value) {
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
        assert_eq!(json["adapter_connection_included"], true);
        assert_eq!(json["adapter_connection_path"], "adapter-connection.txt");
        assert_eq!(json["adapter_connection_json_included"], true);
        assert_eq!(
            json["adapter_connection_json_path"],
            "adapter-connection.json"
        );
        assert_eq!(json["initial_usb_capture_included"], true);
        assert_eq!(json["initial_usb_capture_path"], "initial-usb-capture.txt");
        assert_eq!(json["adapter_survey_included"], true);
        assert_eq!(json["adapter_survey_path"], "adapter-survey.txt");
        assert_eq!(json["adapter_survey_json_included"], true);
        assert_eq!(json["adapter_survey_json_path"], "adapter-survey.json");
        assert_eq!(json["bluetooth_report_included"], true);
        assert_eq!(json["bluetooth_report_path"], "bluetooth-report.txt");
        assert_eq!(json["bluetooth_report_json_included"], true);
        assert_eq!(json["bluetooth_report_json_path"], "bluetooth-report.json");
    }

    fn assert_packet_outputs_and_policy(json: &serde_json::Value) {
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
        assert_eq!(json["usb_enumeration_analysis_included"], true);
        assert_eq!(
            json["usb_enumeration_analysis_path"],
            "usb-enumeration-analysis.txt"
        );
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
        assert!(
            json["redaction_policy"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry.as_str().unwrap().contains("SSID values are redacted"))
        );
    }
}
