use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::Local;

use super::adapter_survey::{
    adapter_survey_candidates, adapter_survey_report_json, adapter_survey_text,
    build_adapter_survey_report, diag_has_usb_host_traffic, survey_attempt_from_diag,
    survey_diag_accepted, AdapterSurveyRawCapture, AdapterSurveyReport,
};
use super::bluetooth_report::{
    bluetooth_usb_packets_stub, build_bluetooth_report, format_bluetooth_report_json,
    format_bluetooth_report_text, BluetoothReport, BluetoothReportInput,
};
use super::manifest::ManifestPicoCapture;
use super::pico_diag;
use super::usb_packets::{
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, duration_ms_u64, usb_packets_text_from_debug_snapshot,
    usb_packets_text_from_diag,
};
use crate::{
    cdc, cmd_auto, cmd_persona, cmd_run, cmd_usb_diag, config, debug_packets, pico_cache,
    pico_mode, pico_state, protocol,
};
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

#[derive(Default)]
pub(super) struct CaptureLog {
    pub(super) lines: Vec<String>,
}

impl CaptureLog {
    pub(super) fn record_duration(
        &mut self,
        step: impl AsRef<str>,
        duration_ms: u64,
        status: impl AsRef<str>,
        bytes: usize,
        reason: impl AsRef<str>,
    ) {
        let step = sanitize_log_field(step.as_ref());
        let status = sanitize_log_field(status.as_ref());
        let reason = sanitize_log_field(reason.as_ref());
        tracing::debug!(
            "bundle-capture: step={} duration_ms={} status={} bytes={} reason={}",
            step,
            duration_ms,
            status,
            bytes,
            reason
        );
        self.lines.push(format!(
            "{}\t{step}\t{duration_ms}\t{status}\t{bytes}\t{reason}",
            Local::now().to_rfc3339()
        ));
    }

    pub(super) fn record(
        &mut self,
        step: impl AsRef<str>,
        started: Instant,
        status: impl AsRef<str>,
        bytes: usize,
        reason: impl AsRef<str>,
    ) {
        let duration_ms = pico_cache::duration_ms(started.elapsed());
        self.record_duration(step, duration_ms, status, bytes, reason);
    }

    pub(super) fn text(&self) -> String {
        let mut out = String::from("captured_at\tstep\tduration_ms\tstatus\tbytes\treason\n");
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

fn sanitize_log_field(value: &str) -> String {
    value.replace(['\r', '\n', '\t'], " ").trim().to_string()
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
            .map(|name| name.to_string())
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

    let started = Instant::now();
    match cmd_run::discover_picos(Duration::from_secs(2)).await {
        Ok(picos) => {
            capture_log.record(
                "per_pico.broadcast_discovery",
                started,
                "ok",
                picos.len(),
                format!("targets={}", picos.len()),
            );
            for target in picos {
                let uid = target.uid_hex();
                seeds
                    .entry(uid.clone())
                    .and_modify(|seed| {
                        seed.target = Some(target.clone());
                        seed.source = "broadcast discovery".to_string();
                    })
                    .or_insert(PicoCaptureSeed {
                        uid,
                        target: Some(target),
                        saved: None,
                        source: "broadcast discovery".to_string(),
                        cached_state_json: None,
                    });
            }
        }
        Err(e) => {
            capture_log.record(
                "per_pico.broadcast_discovery",
                started,
                "error",
                0,
                format!("{e:#}"),
            );
        }
    }

    let cfg = config::load().unwrap_or_default();
    for saved in saved_picos_from_config(&cfg) {
        let uid = saved.uid_hex();
        seeds
            .entry(uid.clone())
            .and_modify(|seed| {
                seed.saved = Some(saved.clone());
            })
            .or_insert(PicoCaptureSeed {
                uid: uid.clone(),
                target: None,
                saved: Some(saved.clone()),
                source: "saved config".to_string(),
                cached_state_json: None,
            });

        let already_live = seeds
            .get(&uid)
            .and_then(|seed| seed.target.as_ref())
            .is_some();
        if already_live {
            continue;
        }
        let Some(last_ip) = saved.last_ip.as_deref() else {
            continue;
        };
        let Some(ip) = cmd_run::parse_ip_selector(last_ip) else {
            capture_log.record(
                format!("per_pico.{uid}.last_known_ip_probe"),
                Instant::now(),
                "invalid_saved_ip",
                0,
                last_ip,
            );
            continue;
        };

        let started = Instant::now();
        match cmd_run::probe_pico_ip(ip, Duration::from_secs(2)).await {
            Ok(target) => {
                capture_log.record(
                    format!("per_pico.{uid}.last_known_ip_probe"),
                    started,
                    "ok",
                    1,
                    target.peer.to_string(),
                );
                seeds
                    .entry(uid.clone())
                    .and_modify(|seed| {
                        seed.target = Some(target.clone());
                        seed.source = "last-known IP probe".to_string();
                    })
                    .or_insert(PicoCaptureSeed {
                        uid,
                        target: Some(target),
                        saved: Some(saved),
                        source: "last-known IP probe".to_string(),
                        cached_state_json: None,
                    });
            }
            Err(e) => {
                capture_log.record(
                    format!("per_pico.{uid}.last_known_ip_probe"),
                    started,
                    "not_reachable",
                    0,
                    format!("{e:#}"),
                );
            }
        }
    }

    if let Some(cache) = pico_cache::read_current() {
        if let Some(uid) = uid_from_cache_json(&cache) {
            seeds
                .entry(uid.clone())
                .and_modify(|seed| {
                    seed.cached_state_json = Some(cache.clone());
                })
                .or_insert(PicoCaptureSeed {
                    uid,
                    target: None,
                    saved: None,
                    source: "diagnostic cache".to_string(),
                    cached_state_json: Some(cache),
                });
        }
    }

    let mut captures = Vec::new();
    for seed in seeds.into_values() {
        captures.push(capture_one_pico(seed, capture_log).await);
    }
    captures
}

async fn capture_one_pico(
    seed: PicoCaptureSeed,
    capture_log: &mut CaptureLog,
) -> PicoBundleCapture {
    let path = format!("picos/{}", sanitize_path_component(&seed.uid));
    let state_captured: bool;
    let pico_diag_status: String;
    let usb_diag_status: String;
    let pico_state_status: String;
    let pico_diag_text: String;
    let usb_diag_text: String;
    let initial_usb_capture_text: String;
    let usb_packets_text: String;
    let adapter_survey_text: String;
    let adapter_survey_json: String;
    let adapter_survey_report: Option<AdapterSurveyReport>;
    let bluetooth_report_text: String;
    let bluetooth_report_json: String;
    let bluetooth_report: Option<BluetoothReport>;

    let state_json = if let Some(target) = seed.target.as_ref() {
        state_captured = true;
        let mut snapshot = pico_cache::PicoStateSnapshot::from_target("bundle", target);
        let target_pico_state_status;
        let target_usb_diag_status;
        let target_bt_status_status;
        let mut target_pico_state_data: Option<protocol::PicoStateDiag> = None;
        let mut target_usb_diag_data: Option<protocol::UsbDiag> = None;
        let mut target_bt_status_data: Option<cdc::BtStatus> = None;
        let mut target_bt_status_error: Option<String> = None;

        let started = Instant::now();
        match pico_state::query_pico_state(target, Duration::from_millis(900)).await {
            Ok(state) => {
                target_pico_state_status = "captured".to_string();
                snapshot = snapshot.with_pico_state(&state);
                target_pico_state_data = Some(state);
                capture_log.record(
                    format!("per_pico.{}.pico_state", seed.uid),
                    started,
                    "captured",
                    crate::protocol::PICO_STATE_WIRE_SIZE,
                    "",
                );
            }
            Err(e) => {
                target_pico_state_status = "timeout_or_unsupported".to_string();
                capture_log.record(
                    format!("per_pico.{}.pico_state", seed.uid),
                    started,
                    "timeout_or_unsupported",
                    0,
                    format!("{e:#}"),
                );
            }
        }

        let started = Instant::now();
        let diag = pico_diag::capture_run_udp_for_target(target).await;
        let target_pico_diag_status = diag.discriminant_str().to_string();
        let target_pico_diag_text = diag.stub_text();
        capture_log.record(
            format!("per_pico.{}.pico_diag", seed.uid),
            started,
            &target_pico_diag_status,
            target_pico_diag_text.len(),
            diag.source_str().unwrap_or(""),
        );

        let started = Instant::now();
        let target_usb_diag_text =
            match cmd_usb_diag::query_usb_diag(target, Duration::from_secs(3)).await {
                Ok(diag) => {
                    target_usb_diag_status = "captured".to_string();
                    let text = cmd_usb_diag::format_usb_diag(&diag, target.persona);
                    snapshot = snapshot.with_usb_diag(&diag, target.persona);
                    target_usb_diag_data = Some(diag);
                    capture_log.record(
                        format!("per_pico.{}.usb_diag", seed.uid),
                        started,
                        "captured",
                        text.len(),
                        "",
                    );
                    text
                }
                Err(e) => {
                    target_usb_diag_status = "not_captured".to_string();
                    let text = format!(
                    "Structured Pico USB diagnostics were not captured for {}.\n\nerror={e:#}\n",
                    target.detail_label()
                );
                    capture_log.record(
                        format!("per_pico.{}.usb_diag", seed.uid),
                        started,
                        "not_captured",
                        text.len(),
                        format!("{e:#}"),
                    );
                    text
                }
            };

        let started = Instant::now();
        if target.persona.is_bluetooth() {
            match query_bluetooth_cdc_status(target).await {
                Ok(status) => {
                    target_bt_status_status = "captured".to_string();
                    let bytes = cdc::BT_STATUS_FIXED_LEN.saturating_add(status.local_name.len());
                    target_bt_status_data = Some(status);
                    capture_log.record(
                        format!("per_pico.{}.bt_status_cdc", seed.uid),
                        started,
                        "captured",
                        bytes,
                        "",
                    );
                }
                Err(e) => {
                    let short = short_bundle_error(&e);
                    target_bt_status_status = "not_captured".to_string();
                    target_bt_status_error = Some(short.clone());
                    capture_log.record(
                        format!("per_pico.{}.bt_status_cdc", seed.uid),
                        started,
                        "not_captured",
                        0,
                        short,
                    );
                }
            }
        } else {
            target_bt_status_status = "not_applicable".to_string();
        }

        snapshot = snapshot.with_outcome(format!(
            "bundle: pico_state={target_pico_state_status}; pico_diag={target_pico_diag_status}; usb_diag={target_usb_diag_status}; bt_status_cdc={target_bt_status_status}"
        ));
        pico_cache::record(snapshot.clone());
        pico_state_status = target_pico_state_status;
        pico_diag_status = target_pico_diag_status;
        pico_diag_text = target_pico_diag_text;
        initial_usb_capture_text = usb_packets_text_from_diag(&seed.uid, &pico_diag_text);
        if target.persona.is_bluetooth() {
            let report = build_bluetooth_report(
                &seed.uid,
                &path,
                target,
                BluetoothReportInput {
                    pico_state: target_pico_state_data.as_ref(),
                    bt_status: target_bt_status_data.as_ref(),
                    bt_status_error: target_bt_status_error,
                    usb_diag: target_usb_diag_data.as_ref(),
                    pico_diag_text: &pico_diag_text,
                },
            );
            let text = format_bluetooth_report_text(&report);
            let json = format_bluetooth_report_json(&report);
            capture_log.record_duration(
                format!("per_pico.{}.bluetooth_report", seed.uid),
                0,
                report.status,
                text.len().saturating_add(json.len()),
                "txt_and_json",
            );
            usb_packets_text = bluetooth_usb_packets_stub(&seed.uid, target);
            adapter_survey_text = String::new();
            adapter_survey_json = String::new();
            adapter_survey_report = None;
            bluetooth_report_text = text;
            bluetooth_report_json = json;
            bluetooth_report = Some(report);
        } else {
            let packet_capture = bundle_usb_packets_for_target(
                &seed.uid,
                target,
                &pico_diag_text,
                target_usb_diag_data.as_ref(),
                capture_log,
            )
            .await;
            if let Some(capture_target) = packet_capture.capture_target.as_ref() {
                pico_cache::record(
                    pico_cache::PicoStateSnapshot::from_target(
                        "bundle-usb-capture",
                        capture_target,
                    )
                    .with_outcome("bundle: persona USB capture"),
                );
            }
            usb_packets_text = packet_capture.text;
            adapter_survey_text = packet_capture.adapter_survey_text;
            adapter_survey_json = packet_capture.adapter_survey_json;
            adapter_survey_report = packet_capture.adapter_survey_report;
            bluetooth_report_text = String::new();
            bluetooth_report_json = String::new();
            bluetooth_report = None;
        }
        usb_diag_status = target_usb_diag_status;
        usb_diag_text = target_usb_diag_text;
        state_json_from_snapshot(&snapshot)
    } else if let Some(saved) = seed.saved.as_ref() {
        state_captured = true;
        let snapshot = pico_cache::PicoStateSnapshot::offline_from_config("bundle-offline", saved);
        pico_cache::record(snapshot.clone());
        pico_diag_status = "offline_not_attempted".to_string();
        usb_diag_status = "offline_not_attempted".to_string();
        pico_state_status = "offline_not_attempted".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        initial_usb_capture_text = offline_pico_text(&seed, "initial USB packet dump");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        adapter_survey_text = String::new();
        adapter_survey_json = String::new();
        adapter_survey_report = None;
        bluetooth_report_text = String::new();
        bluetooth_report_json = String::new();
        bluetooth_report = None;
        state_json_from_snapshot(&snapshot)
    } else if let Some(cached) = seed.cached_state_json.as_ref() {
        state_captured = true;
        pico_diag_status = "cache_only_not_attempted".to_string();
        usb_diag_status = "cache_only_not_attempted".to_string();
        pico_state_status = "cache_only".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        initial_usb_capture_text = offline_pico_text(&seed, "initial USB packet dump");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        adapter_survey_text = String::new();
        adapter_survey_json = String::new();
        adapter_survey_report = None;
        bluetooth_report_text = String::new();
        bluetooth_report_json = String::new();
        bluetooth_report = None;
        cached.clone()
    } else {
        state_captured = false;
        pico_diag_status = "no_state".to_string();
        usb_diag_status = "no_state".to_string();
        pico_state_status = "no_state".to_string();
        pico_diag_text = offline_pico_text(&seed, "firmware diag log");
        usb_diag_text = offline_pico_text(&seed, "USB counters");
        initial_usb_capture_text = offline_pico_text(&seed, "initial USB packet dump");
        usb_packets_text = offline_pico_text(&seed, "USB packet dump");
        adapter_survey_text = String::new();
        adapter_survey_json = String::new();
        adapter_survey_report = None;
        bluetooth_report_text = String::new();
        bluetooth_report_json = String::new();
        bluetooth_report = None;
        "{}\n".to_string()
    };
    let usb_packet_count = count_usb_packet_lines(&usb_packets_text);
    let usb_packet_stats_count = count_usb_packet_stats_lines(&usb_packets_text);
    let usb_packet_event_count = count_usb_packet_event_lines(&usb_packets_text);
    let usb_packet_harvest_count = count_usb_packet_harvest_lines(&usb_packets_text);
    let usb_packet_status = if usb_packet_count > 0 {
        "captured"
    } else if usb_packet_stats_count > 0 {
        "stats_only"
    } else if usb_packet_event_count > 0 {
        "lifecycle_only"
    } else if usb_packet_harvest_count > 0 {
        "harvest_only"
    } else if seed.target.is_some() {
        "no_packets"
    } else {
        "not_attempted"
    };
    capture_log.record_duration(
        format!("per_pico.{}.usb_packets", seed.uid),
        0,
        usb_packet_status,
        usb_packets_text.len(),
        format!(
            "count={usb_packet_count}; stats={usb_packet_stats_count}; events={usb_packet_event_count}; harvest={usb_packet_harvest_count}"
        ),
    );

    PicoBundleCapture {
        manifest: ManifestPicoCapture {
            uid: seed.uid,
            path,
            peer: seed.target.as_ref().map(|target| target.peer.to_string()),
            live: seed.target.is_some(),
            source: seed.source,
            state_captured,
            pico_diag_status,
            usb_diag_status,
            pico_state_status,
            usb_packet_dump_status: usb_packet_status.to_string(),
            usb_packet_dump_count: usb_packet_count,
            bluetooth_report_status: bluetooth_report
                .as_ref()
                .map(|report| report.status.to_string())
                .unwrap_or_else(|| {
                    if seed
                        .target
                        .as_ref()
                        .map(|target| target.persona.is_bluetooth())
                        .unwrap_or(false)
                    {
                        "not_captured".to_string()
                    } else {
                        "not_applicable".to_string()
                    }
                }),
            cached_state_included: seed.cached_state_json.is_some(),
        },
        state_json,
        pico_diag_text,
        usb_diag_text,
        initial_usb_capture_text,
        usb_packets_text,
        adapter_survey_text,
        adapter_survey_json,
        adapter_survey_report,
        bluetooth_report_text,
        bluetooth_report_json,
        bluetooth_report,
    }
}

async fn bundle_usb_packets_for_target(
    uid: &str,
    target: &cmd_run::PicoTarget,
    fallback_diag_text: &str,
    current_diag: Option<&protocol::UsbDiag>,
    capture_log: &mut CaptureLog,
) -> BundleUsbPacketCapture {
    let original_persona = target.persona;
    let mut current = target.clone();
    let mut attempts = Vec::new();
    let mut capture_sections = Vec::new();
    let mut capture_target = None;
    let mut current_raw_capture = AdapterSurveyRawCapture::not_attempted("not_needed");
    let current_needs_capture = current_diag
        .map(|diag| diag.device_desc_count > 0 && !survey_diag_accepted(target.persona, diag))
        .unwrap_or(false);
    if current_needs_capture {
        let captured = if target.persona == protocol::Persona::Debug {
            let text =
                harvest_usb_packets_for_target(uid, target, fallback_diag_text, capture_log).await;
            PersonaPacketCapture {
                raw_capture: AdapterSurveyRawCapture {
                    attempted: true,
                    status: "captured".to_string(),
                    raw_packet_lines: count_usb_packet_lines(&text),
                    packet_stats_lines: count_usb_packet_stats_lines(&text),
                    usb_event_lines: count_usb_packet_event_lines(&text),
                    harvest_lines: count_usb_packet_harvest_lines(&text),
                },
                text,
                capture_target: Some(target.clone()),
            }
        } else {
            capture_persona_usb_packets(
                uid,
                target,
                target.persona,
                fallback_diag_text,
                capture_log,
            )
            .await
        };
        current_raw_capture = captured.raw_capture;
        if let Some(target) = captured.capture_target {
            current = target.clone();
            capture_target = Some(target);
        }
        if !captured.text.is_empty() {
            capture_sections.push(captured.text);
        }
    }

    let current_attempt = survey_attempt_from_diag(
        target.persona,
        true,
        false,
        current_diag.cloned(),
        current_raw_capture,
    );
    let current_has_no_usb_host = current_diag
        .map(|diag| !diag_has_usb_host_traffic(diag))
        .unwrap_or(false);
    let current_accepted = current_attempt.accepted;
    attempts.push(current_attempt);

    if current_has_no_usb_host {
        capture_log.record_duration(
            format!("per_pico.{uid}.adapter_survey.current"),
            0,
            "no_usb_host_traffic",
            0,
            "current USB diagnostic had no descriptor, mount, suspend, report, or OUT traffic",
        );
    }

    let candidates = adapter_survey_candidates(target.persona, current_accepted);
    for candidate in candidates {
        let switched = current.persona != candidate;
        let Some(active) =
            switch_to_survey_persona(uid, current.clone(), candidate, capture_log).await
        else {
            attempts.push(survey_attempt_from_diag(
                candidate,
                false,
                switched,
                None,
                AdapterSurveyRawCapture::not_attempted("switch_failed"),
            ));
            continue;
        };
        current = active.clone();

        capture_log.record_duration(
            format!(
                "per_pico.{uid}.adapter_survey.{}.usb_settle",
                candidate.label()
            ),
            cmd_auto::USB_SETTLE.as_millis() as u64,
            "sleep",
            0,
            "allow adapter USB host detection",
        );
        tokio::time::sleep(cmd_auto::USB_SETTLE).await;

        let diag = query_survey_usb_diag(uid, &active, candidate, capture_log).await;
        let needs_capture = diag
            .as_ref()
            .map(|diag| diag.device_desc_count > 0 && !survey_diag_accepted(candidate, diag))
            .unwrap_or(false);
        let mut raw_capture = AdapterSurveyRawCapture::not_attempted("not_needed");
        if needs_capture {
            let captured = capture_persona_usb_packets(
                uid,
                &active,
                candidate,
                fallback_diag_text,
                capture_log,
            )
            .await;
            raw_capture = captured.raw_capture;
            if let Some(target) = captured.capture_target {
                current = target.clone();
                capture_target = Some(target);
            }
            if !captured.text.is_empty() {
                capture_sections.push(captured.text);
            }
        }

        let attempt = survey_attempt_from_diag(candidate, false, switched, diag, raw_capture);
        let accepted = attempt.accepted;
        attempts.push(attempt);
        if accepted {
            capture_log.record_duration(
                format!("per_pico.{uid}.adapter_survey.stop"),
                0,
                "accepted",
                0,
                format!("persona={}", candidate.label()),
            );
            break;
        }
    }

    let restore_status =
        restore_persona_after_bundle(uid, &current, original_persona, capture_log).await;
    let restored_persona = if restore_status == "confirmed" || restore_status == "already_current" {
        Some(original_persona.label().to_string())
    } else {
        None
    };
    let report = build_adapter_survey_report(
        uid.to_string(),
        original_persona.label().to_string(),
        restore_status,
        restored_persona,
        attempts,
        vec![
            "PS3 is tested first for USB-to-Maple adapters, followed by a generic HID gamepad fallback.",
            "Debug mode uses the XInput USB shape and is not selected as adapter proof.",
            "Polling or configured means the adapter accepted that persona.",
            "device_desc_count=0 means the adapter did not enumerate that persona.",
            "Descriptor traffic without configuration points to descriptor or report rejection.",
        ],
    );

    let adapter_survey_text = adapter_survey_text(&report);
    let adapter_survey_json = adapter_survey_report_json(&report);
    let mut text = usb_packets_text_from_diag(uid, fallback_diag_text);
    for section in capture_sections {
        text.push('\n');
        text.push_str(&section);
    }
    BundleUsbPacketCapture {
        text,
        capture_target,
        adapter_survey_text,
        adapter_survey_json,
        adapter_survey_report: Some(report),
    }
}

async fn switch_to_survey_persona(
    uid: &str,
    current: cmd_run::PicoTarget,
    candidate: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> Option<cmd_run::PicoTarget> {
    if current.persona == candidate {
        return Some(current);
    }

    let started = Instant::now();
    match pico_mode::request_set_persona(&current, candidate).await {
        Ok(()) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.switch_request",
                candidate.label()
            ),
            started,
            "sent",
            1,
            format!("from={} to={}", current.persona.label(), candidate.label()),
        ),
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.switch_request",
                    candidate.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return None;
        }
    }

    let started = Instant::now();
    let matched = match cmd_persona::wait_for_persona(
        &[current.info.unique_id_short],
        candidate,
        BUNDLE_PERSONA_WAIT,
    )
    .await
    {
        Ok(matched) => matched,
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.switch_wait",
                    candidate.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return None;
        }
    };
    let found = matched
        .iter()
        .find(|pico| pico.info.unique_id_short == current.info.unique_id_short)
        .cloned();
    capture_log.record(
        format!(
            "per_pico.{uid}.adapter_survey.{}.switch_wait",
            candidate.label()
        ),
        started,
        if found
            .as_ref()
            .map(|pico| pico.persona == candidate)
            .unwrap_or(false)
        {
            "confirmed"
        } else {
            "not_confirmed"
        },
        matched.len(),
        format!("observed={}", format_observed_personas(&matched)),
    );
    match found {
        Some(pico) if pico.persona == candidate => Some(pico),
        _ => None,
    }
}

async fn query_survey_usb_diag(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> Option<protocol::UsbDiag> {
    let started = Instant::now();
    match cmd_usb_diag::query_usb_diag(target, cmd_auto::USB_PROBE).await {
        Ok(diag) => {
            capture_log.record(
                format!("per_pico.{uid}.adapter_survey.{}.usb_diag", persona.label()),
                started,
                "captured",
                protocol::USB_DIAG_WIRE_SIZE,
                format!(
                    "score={}; device_desc_count={}; config_desc_count={}",
                    cmd_auto::score_label(cmd_auto::score_usb_diag(&diag)),
                    diag.device_desc_count,
                    diag.config_desc_count
                ),
            );
            Some(diag)
        }
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.adapter_survey.{}.usb_diag", persona.label()),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            None
        }
    }
}

struct PersonaPacketCapture {
    pub(super) text: String,
    raw_capture: AdapterSurveyRawCapture,
    capture_target: Option<cmd_run::PicoTarget>,
}

async fn capture_persona_usb_packets(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    fallback_diag_text: &str,
    capture_log: &mut CaptureLog,
) -> PersonaPacketCapture {
    let started = Instant::now();
    match pico_mode::request_set_usb_capture_persona(target, persona).await {
        Ok(()) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.capture_request",
                persona.label()
            ),
            started,
            "sent",
            1,
            "usb_capture=enabled",
        ),
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.capture_request",
                    persona.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return PersonaPacketCapture {
                text: String::new(),
                raw_capture: AdapterSurveyRawCapture::not_attempted("request_failed"),
                capture_target: None,
            };
        }
    }

    let started = Instant::now();
    let matched = match cmd_persona::wait_for_persona(
        &[target.info.unique_id_short],
        persona,
        BUNDLE_PERSONA_WAIT,
    )
    .await
    {
        Ok(matched) => matched,
        Err(e) => {
            capture_log.record(
                format!(
                    "per_pico.{uid}.adapter_survey.{}.capture_wait",
                    persona.label()
                ),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return PersonaPacketCapture {
                text: String::new(),
                raw_capture: AdapterSurveyRawCapture::not_attempted("wait_failed"),
                capture_target: None,
            };
        }
    };
    let found = matched
        .iter()
        .find(|pico| pico.info.unique_id_short == target.info.unique_id_short)
        .cloned();
    capture_log.record(
        format!(
            "per_pico.{uid}.adapter_survey.{}.capture_wait",
            persona.label()
        ),
        started,
        if found
            .as_ref()
            .map(|pico| pico.persona == persona)
            .unwrap_or(false)
        {
            "confirmed"
        } else {
            "not_confirmed"
        },
        matched.len(),
        format!("observed={}", format_observed_personas(&matched)),
    );
    let Some(capture_target) = found.filter(|pico| pico.persona == persona) else {
        return PersonaPacketCapture {
            text: String::new(),
            raw_capture: AdapterSurveyRawCapture::not_attempted("not_confirmed"),
            capture_target: None,
        };
    };

    capture_log.record_duration(
        format!(
            "per_pico.{uid}.adapter_survey.{}.capture_settle",
            persona.label()
        ),
        cmd_auto::USB_SETTLE.as_millis() as u64,
        "sleep",
        0,
        "allow capture-enabled persona to enumerate",
    );
    tokio::time::sleep(cmd_auto::USB_SETTLE).await;

    let mut text =
        harvest_usb_packets_for_target(uid, &capture_target, fallback_diag_text, capture_log).await;
    let _ = writeln!(
        text,
        "# adapter-survey-capture persona={} status=attempted",
        persona.label()
    );
    let raw_capture = AdapterSurveyRawCapture {
        attempted: true,
        status: "captured".to_string(),
        raw_packet_lines: count_usb_packet_lines(&text),
        packet_stats_lines: count_usb_packet_stats_lines(&text),
        usb_event_lines: count_usb_packet_event_lines(&text),
        harvest_lines: count_usb_packet_harvest_lines(&text),
    };

    let started = Instant::now();
    match pico_mode::request_clear_usb_capture(&capture_target).await {
        Ok(()) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.capture_clear",
                persona.label()
            ),
            started,
            "sent",
            1,
            "usb_capture=disabled",
        ),
        Err(e) => capture_log.record(
            format!(
                "per_pico.{uid}.adapter_survey.{}.capture_clear",
                persona.label()
            ),
            started,
            "error",
            0,
            format!("{e:#}"),
        ),
    }

    PersonaPacketCapture {
        text,
        raw_capture,
        capture_target: Some(capture_target),
    }
}

async fn harvest_usb_packets_for_target(
    uid: &str,
    target: &cmd_run::PicoTarget,
    fallback_diag_text: &str,
    capture_log: &mut CaptureLog,
) -> String {
    let started = Instant::now();
    match debug_packets::capture_run_diag_log(target.peer, BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT)
        .await
    {
        Ok(snapshot) => {
            let duration_ms = duration_ms_u64(started.elapsed());
            let text = usb_packets_text_from_debug_snapshot(uid, &snapshot, duration_ms);
            capture_log.record(
                format!("per_pico.{uid}.usb_packet_harvest"),
                started,
                "captured",
                text.len(),
                format!(
                    "chunks={}; missing_chunks={}; lost_bytes={}; diag_bytes={}",
                    snapshot.chunk_count,
                    snapshot.missing_chunks.len(),
                    snapshot.lost_bytes,
                    snapshot.byte_count
                ),
            );
            text
        }
        Err(e) => {
            let duration_ms = duration_ms_u64(started.elapsed());
            let mut text = usb_packets_text_from_diag(uid, fallback_diag_text);
            text.push_str(&debug_packets::harvest_error_line(
                duration_ms,
                &format!("{e:#}"),
            ));
            text.push('\n');
            capture_log.record(
                format!("per_pico.{uid}.usb_packet_harvest"),
                started,
                "error",
                text.len(),
                format!("{e:#}"),
            );
            text
        }
    }
}

async fn restore_persona_after_bundle(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> String {
    if target.persona == persona {
        capture_log.record_duration(
            format!("per_pico.{uid}.restore_persona_request"),
            0,
            "already_current",
            0,
            format!("persona={}", persona.label()),
        );
        return "already_current".to_string();
    }

    let started = Instant::now();
    match pico_mode::request_set_persona(target, persona).await {
        Ok(()) => capture_log.record(
            format!("per_pico.{uid}.restore_persona_request"),
            started,
            "sent",
            1,
            format!("persona={}", persona.label()),
        ),
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.restore_persona_request"),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            return "request_failed".to_string();
        }
    }

    let started = Instant::now();
    match cmd_persona::wait_for_persona(
        &[target.info.unique_id_short],
        persona,
        BUNDLE_RESTORE_PERSONA_WAIT,
    )
    .await
    {
        Ok(matched) => {
            let restored = matched.iter().find(|pico| pico.persona == persona);
            let status = if restored.is_some() {
                "confirmed"
            } else {
                "not_confirmed"
            };
            capture_log.record(
                format!("per_pico.{uid}.restore_persona_wait"),
                started,
                status,
                matched.len(),
                format!("observed={}", format_observed_personas(&matched)),
            );
            if let Some(restored) = restored {
                pico_cache::record(
                    pico_cache::PicoStateSnapshot::from_target("bundle-restore", restored)
                        .with_outcome(format!("restored_{}", persona.label())),
                );
            }
            status.to_string()
        }
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.restore_persona_wait"),
                started,
                "error",
                0,
                format!("{e:#}"),
            );
            "wait_failed".to_string()
        }
    }
}

fn format_observed_personas(targets: &[cmd_run::PicoTarget]) -> String {
    if targets.is_empty() {
        return "none".to_string();
    }
    targets
        .iter()
        .map(|target| format!("{}:{}", target.uid_hex(), target.persona.label()))
        .collect::<Vec<_>>()
        .join(",")
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
    let text = error.to_string();
    const MAX_LEN: usize = 180;
    if text.len() <= MAX_LEN {
        text
    } else {
        let prefix: String = text.chars().take(MAX_LEN).collect();
        format!("{prefix}...")
    }
}

fn saved_picos_from_config(cfg: &config::Config) -> Vec<config::PicoIdentity> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for pico in &cfg.picos {
        if seen.insert(pico.unique_id_short) {
            out.push(pico.clone());
        }
    }
    if let Some(pico) = cfg.last_pico.as_ref() {
        if seen.insert(pico.unique_id_short) {
            out.push(pico.clone());
        }
    }
    out
}

fn uid_from_cache_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(uid) = value.get("uid").and_then(|v| v.as_str()) {
        return Some(sanitize_path_component(uid));
    }
    let uid = value.get("unique_id_short").and_then(|v| v.as_u64())?;
    Some(format!("{:08X}", uid as u32))
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

fn state_json_from_snapshot(snapshot: &pico_cache::PicoStateSnapshot) -> String {
    serde_json::to_string_pretty(snapshot).unwrap_or_else(|e| {
        format!(
            "{{\"capture_outcome\":\"state_serialization_failed\",\"error\":\"{}\"}}\n",
            e
        )
    })
}

fn offline_pico_text(seed: &PicoCaptureSeed, artifact: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{artifact} was not captured because this Pico was not reachable during bundle capture."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "uid={}", seed.uid);
    let _ = writeln!(out, "source={}", seed.source);
    if let Some(saved) = seed.saved.as_ref() {
        let _ = writeln!(out, "saved_board={}", saved.board_label());
        let _ = writeln!(out, "saved_firmware={}", saved.firmware_version());
        if let Some(ip) = saved.last_ip.as_deref() {
            let _ = writeln!(out, "last_known_ip={ip}");
        }
    }
    if seed.cached_state_json.is_some() {
        let _ = writeln!(out, "cached_state_available=true");
    }
    out
}
