mod log;
mod persona;
mod pico;
mod seeds;
mod survey;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

use super::adapter_survey::AdapterSurveyReport;
use super::bluetooth_report::BluetoothReport;
use super::manifest::ManifestPicoCapture;
use crate::{cdc, cmd_run, cmd_usb_diag, config, debug_packets};
pub(super) use log::CaptureLog;
use persona::{
    PersonaPacketCapture, capture_persona_usb_packets, format_observed_personas,
    harvest_usb_packets_for_target, restore_persona_after_bundle,
};
use pico::capture_one_pico;
use seeds::{seed_from_broadcast_discovery, seed_from_diagnostic_cache, seed_from_saved_config};
use survey::bundle_usb_packets_for_target;
const BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT: Duration = Duration::from_secs(2);
const BUNDLE_PERSONA_WAIT: Duration = Duration::from_secs(60);
const BUNDLE_RESTORE_PERSONA_WAIT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug)]
pub(super) struct UsbDiagBundle {
    pub(super) text: String,
    pub(super) captured: bool,
    pub(super) target_count: usize,
}

#[derive(Clone, Debug)]
pub(super) struct PicoBundleCapture {
    pub(super) manifest: ManifestPicoCapture,
    pub(super) state_json: String,
    pub(super) pico_diag_text: String,
    pub(super) usb_diag_text: String,
    pub(super) initial_usb_capture_text: String,
    pub(super) usb_packets_text: String,
    pub(super) adapter_survey_text: String,
    pub(super) adapter_survey_json: String,
    pub(super) adapter_survey_report: Option<AdapterSurveyReport>,
    pub(super) bluetooth_report_text: String,
    pub(super) bluetooth_report_json: String,
    pub(super) bluetooth_report: Option<BluetoothReport>,
}

#[derive(Clone, Debug)]
struct PicoCaptureSeed {
    uid: String,
    target: Option<cmd_run::PicoTarget>,
    saved: Option<config::PicoIdentity>,
    source: String,
    cached_state_json: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct RetainedDebugPacketLog {
    pub(super) name: String,
    pub(super) text: String,
}

#[derive(Clone, Debug)]
struct BundleUsbPacketCapture {
    pub(super) text: String,
    capture_target: Option<cmd_run::PicoTarget>,
    pub(super) adapter_survey_text: String,
    pub(super) adapter_survey_json: String,
    pub(super) adapter_survey_report: Option<AdapterSurveyReport>,
}

pub(super) fn collect_retained_debug_packet_logs(
    capture_log: &mut CaptureLog,
) -> Vec<RetainedDebugPacketLog> {
    let mut out = Vec::new();
    for path in debug_packets::recent_packet_files(debug_packets::DEBUG_PACKET_FILE_RETENTION) {
        let started = Instant::now();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(std::string::ToString::to_string)
        else {
            continue;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                capture_log.record(
                    format!("retained_debug_packet_log.{name}"),
                    started,
                    "included",
                    text.len(),
                    "",
                );
                out.push(RetainedDebugPacketLog { name, text });
            }
            Err(e) => {
                capture_log.record(
                    format!("retained_debug_packet_log.{name}"),
                    started,
                    "not_included",
                    0,
                    format!("{e:#}"),
                );
            }
        }
    }
    out
}

pub(super) async fn capture_usb_diag_text() -> UsbDiagBundle {
    let (targets, source) = match resolve_usb_diag_targets().await {
        Ok(found) => found,
        Err(e) => {
            return UsbDiagBundle {
                text: format!(
                    "Structured Pico USB diagnostics were not captured.\n\n\
                     Suggested next step:\n\
                     - Make sure the Pico is powered, has joined Wi-Fi, and is still plugged into the console adapter.\n\
                     - Run `couchlink.exe bundle` again immediately after the failure.\n\
                     - If the Pico is on Wi-Fi but broadcast discovery is blocked, choose `Enter Pico IP manually` from the guided menu once, then run bundle again.\n\n\
                     Diagnostic details:\n\
                     error={e:#}\n"
                ),
                captured: false,
                target_count: 0,
            };
        }
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Structured Pico USB diagnostics\n# target source: {source}\n"
    );

    let mut captured = false;
    for pico in &targets {
        let _ = writeln!(out, "{}", pico.detail_label());
        match cmd_usb_diag::query_usb_diag(pico, Duration::from_secs(3)).await {
            Ok(diag) => {
                captured = true;
                out.push_str(&cmd_usb_diag::format_usb_diag(&diag, pico.persona));
            }
            Err(e) => {
                let _ = writeln!(
                    out,
                    "  FAIL  USB diagnostic did not reply: {e:#}\n  Update Pico firmware, then run this bundle again."
                );
            }
        }
        out.push('\n');
    }

    UsbDiagBundle {
        text: out,
        captured,
        target_count: targets.len(),
    }
}

async fn resolve_usb_diag_targets() -> Result<(Vec<cmd_run::PicoTarget>, String)> {
    match cmd_run::discover_picos(Duration::from_secs(2)).await {
        Ok(picos) if !picos.is_empty() => {
            return Ok((picos, "broadcast discovery".to_string()));
        }
        Ok(_) => {}
        Err(e) => {
            tracing::debug!("bundle: USB diag broadcast discovery failed: {e:#}");
        }
    }

    let cfg = config::load().unwrap_or_default();
    let last_ip = cfg
        .last_pico
        .as_ref()
        .and_then(|p| p.last_ip.as_deref())
        .ok_or_else(|| {
            anyhow!("no running Pico answered discovery and no last-known Pico IP is saved")
        })?;
    let ip = cmd_run::parse_ip_selector(last_ip)
        .ok_or_else(|| anyhow!("last-known Pico IP `{last_ip}` is not a valid IP address"))?;
    let pico = cmd_run::probe_pico_ip(ip, Duration::from_secs(3))
        .await
        .with_context(|| format!("probing last-known Pico IP {ip}"))?;
    Ok((vec![pico], format!("last-known IP {ip}")))
}

pub(super) async fn capture_per_pico(capture_log: &mut CaptureLog) -> Vec<PicoBundleCapture> {
    let mut seeds: BTreeMap<String, PicoCaptureSeed> = BTreeMap::new();
    seed_from_broadcast_discovery(&mut seeds, capture_log).await;
    seed_from_saved_config(&mut seeds, capture_log).await;
    seed_from_diagnostic_cache(&mut seeds);

    let mut captures = Vec::new();
    for seed in seeds.into_values() {
        captures.push(capture_one_pico(seed, capture_log).await);
    }
    captures
}

async fn query_bluetooth_cdc_status(target: &cmd_run::PicoTarget) -> Result<cdc::BtStatus> {
    if !target.persona.is_bluetooth() {
        anyhow::bail!("target is not in Bluetooth mode");
    }
    let uid = target.info.unique_id_short;
    tokio::task::spawn_blocking(move || query_bluetooth_cdc_status_blocking(uid))
        .await
        .context("joining Bluetooth CDC status query")?
}

fn query_bluetooth_cdc_status_blocking(uid: u32) -> Result<cdc::BtStatus> {
    let ports = cdc::find_setup_ports().context(
        "Bluetooth mode status requires the Pico USB diagnostic port; could not enumerate local CouchLink USB diagnostic ports",
    )?;
    let mut probe_errors = Vec::new();
    for port in ports {
        match cdc::PicoSetup::open_named(&port).and_then(|mut pico| {
            let found_uid = pico.unique_id_short()?;
            if found_uid == uid {
                let status = pico.bt_status()?;
                Ok(Some(status))
            } else {
                Ok(None)
            }
        }) {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(e) => probe_errors.push(format!("{port}: {e:#}")),
        }
    }

    let mut msg = format!(
        "no matching CouchLink USB diagnostic port answered for Pico {uid:08X}; expected USB identity VID 0x2E8A PID 0xCAF0"
    );
    if !probe_errors.is_empty() {
        msg.push_str("; probe errors: ");
        msg.push_str(&probe_errors.join(" | "));
    }
    anyhow::bail!("{msg}")
}

fn short_bundle_error(error: &anyhow::Error) -> String {
    const MAX_LEN: usize = 180;
    let text = error.to_string();
    if text.len() <= MAX_LEN {
        text
    } else {
        let prefix: String = text.chars().take(MAX_LEN).collect();
        format!("{prefix}...")
    }
}

pub(super) fn sanitize_path_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}
