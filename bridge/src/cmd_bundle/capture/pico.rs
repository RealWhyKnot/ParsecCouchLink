use super::log::CaptureLog;
use super::{
    PicoBundleCapture, PicoCaptureSeed, bundle_usb_packets_for_target, query_bluetooth_cdc_status,
    sanitize_path_component, short_bundle_error,
};
use crate::cmd_bundle::adapter_survey::AdapterSurveyReport;
use crate::cmd_bundle::bluetooth_report::{
    BluetoothReport, BluetoothReportInput, bluetooth_usb_packets_stub, build_bluetooth_report,
    format_bluetooth_report_json, format_bluetooth_report_text,
};
use crate::cmd_bundle::manifest::ManifestPicoCapture;
use crate::cmd_bundle::pico_diag;
use crate::cmd_bundle::usb_packets::{
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, usb_packets_text_from_diag,
};
use crate::{cdc, cmd_run, cmd_usb_diag, pico_cache, pico_state, protocol};
use std::fmt::Write as _;
use std::time::{Duration, Instant};

pub(super) struct PicoReportOutputs {
    usb_packets_text: String,
    adapter_survey_text: String,
    adapter_survey_json: String,
    adapter_survey_report: Option<AdapterSurveyReport>,
    bluetooth_report_text: String,
    bluetooth_report_json: String,
    bluetooth_report: Option<BluetoothReport>,
}

impl PicoReportOutputs {
    fn packets_only(usb_packets_text: String) -> Self {
        Self {
            usb_packets_text,
            adapter_survey_text: String::new(),
            adapter_survey_json: String::new(),
            adapter_survey_report: None,
            bluetooth_report_text: String::new(),
            bluetooth_report_json: String::new(),
            bluetooth_report: None,
        }
    }
}

pub(super) struct PicoCaptureParts {
    state_captured: bool,
    state_json: String,
    pico_diag_status: String,
    usb_diag_status: String,
    pico_state_status: String,
    pico_diag_text: String,
    usb_diag_text: String,
    initial_usb_capture_text: String,
    outputs: PicoReportOutputs,
}

pub(super) struct UsbDiagProbe {
    status: &'static str,
    text: String,
    diag: Option<protocol::UsbDiag>,
}

pub(super) struct BtStatusProbe {
    status: &'static str,
    data: Option<cdc::BtStatus>,
    error: Option<String>,
}

pub(super) async fn probe_pico_state(
    uid: &str,
    target: &cmd_run::PicoTarget,
    capture_log: &mut CaptureLog,
) -> Option<protocol::PicoStateDiag> {
    let started = Instant::now();
    match pico_state::query_pico_state(target, Duration::from_millis(900)).await {
        Ok(state) => {
            capture_log.record(
                format!("per_pico.{uid}.pico_state"),
                started,
                "captured",
                crate::protocol::PICO_STATE_WIRE_SIZE,
                "",
            );
            Some(state)
        }
        Err(e) => {
            capture_log.record(
                format!("per_pico.{uid}.pico_state"),
                started,
                "timeout_or_unsupported",
                0,
                format!("{e:#}"),
            );
            None
        }
    }
}

pub(super) async fn probe_usb_diag(
    uid: &str,
    target: &cmd_run::PicoTarget,
    capture_log: &mut CaptureLog,
) -> UsbDiagProbe {
    let started = Instant::now();
    match cmd_usb_diag::query_usb_diag(target, Duration::from_secs(3)).await {
        Ok(diag) => {
            let text = cmd_usb_diag::format_usb_diag(&diag, target.persona);
            capture_log.record(
                format!("per_pico.{uid}.usb_diag"),
                started,
                "captured",
                text.len(),
                "",
            );
            UsbDiagProbe {
                status: "captured",
                text,
                diag: Some(diag),
            }
        }
        Err(e) => {
            let text = format!(
                "Structured Pico USB diagnostics were not captured for {}.\n\nerror={e:#}\n",
                target.detail_label()
            );
            capture_log.record(
                format!("per_pico.{uid}.usb_diag"),
                started,
                "not_captured",
                text.len(),
                format!("{e:#}"),
            );
            UsbDiagProbe {
                status: "not_captured",
                text,
                diag: None,
            }
        }
    }
}

pub(super) async fn probe_bt_status(
    uid: &str,
    target: &cmd_run::PicoTarget,
    capture_log: &mut CaptureLog,
) -> BtStatusProbe {
    let started = Instant::now();
    if !target.persona.is_bluetooth() {
        return BtStatusProbe {
            status: "not_applicable",
            data: None,
            error: None,
        };
    }
    match query_bluetooth_cdc_status(target).await {
        Ok(status) => {
            let bytes = cdc::BT_STATUS_FIXED_LEN.saturating_add(status.local_name.len());
            capture_log.record(
                format!("per_pico.{uid}.bt_status_cdc"),
                started,
                "captured",
                bytes,
                "",
            );
            BtStatusProbe {
                status: "captured",
                data: Some(status),
                error: None,
            }
        }
        Err(e) => {
            let short = short_bundle_error(&e);
            capture_log.record(
                format!("per_pico.{uid}.bt_status_cdc"),
                started,
                "not_captured",
                0,
                short.clone(),
            );
            BtStatusProbe {
                status: "not_captured",
                data: None,
                error: Some(short),
            }
        }
    }
}

pub(super) fn bluetooth_outputs(
    uid: &str,
    path: &str,
    target: &cmd_run::PicoTarget,
    input: BluetoothReportInput<'_>,
    capture_log: &mut CaptureLog,
) -> PicoReportOutputs {
    let report = build_bluetooth_report(uid, path, target, input);
    let text = format_bluetooth_report_text(&report);
    let json = format_bluetooth_report_json(&report);
    capture_log.record_duration(
        format!("per_pico.{uid}.bluetooth_report"),
        0,
        report.status,
        text.len().saturating_add(json.len()),
        "txt_and_json",
    );
    PicoReportOutputs {
        bluetooth_report_text: text,
        bluetooth_report_json: json,
        bluetooth_report: Some(report),
        ..PicoReportOutputs::packets_only(bluetooth_usb_packets_stub(uid, target))
    }
}

pub(super) async fn adapter_outputs(
    uid: &str,
    target: &cmd_run::PicoTarget,
    pico_diag_text: &str,
    usb_diag: Option<&protocol::UsbDiag>,
    capture_log: &mut CaptureLog,
) -> PicoReportOutputs {
    let packet_capture =
        bundle_usb_packets_for_target(uid, target, pico_diag_text, usb_diag, capture_log).await;
    if let Some(capture_target) = packet_capture.capture_target.as_ref() {
        pico_cache::record(
            &pico_cache::PicoStateSnapshot::from_target("bundle-usb-capture", capture_target)
                .with_outcome("bundle: persona USB capture"),
        );
    }
    PicoReportOutputs {
        adapter_survey_text: packet_capture.adapter_survey_text,
        adapter_survey_json: packet_capture.adapter_survey_json,
        adapter_survey_report: packet_capture.adapter_survey_report,
        ..PicoReportOutputs::packets_only(packet_capture.text)
    }
}

pub(super) async fn capture_live_pico(
    uid: &str,
    path: &str,
    target: &cmd_run::PicoTarget,
    capture_log: &mut CaptureLog,
) -> PicoCaptureParts {
    let mut snapshot = pico_cache::PicoStateSnapshot::from_target("bundle", target);
    let pico_state = probe_pico_state(uid, target, capture_log).await;
    let pico_state_status = if let Some(state) = pico_state.as_ref() {
        snapshot = snapshot.with_pico_state(state);
        "captured"
    } else {
        "timeout_or_unsupported"
    };

    let started = Instant::now();
    let diag = pico_diag::capture_run_udp_for_target(target).await;
    let pico_diag_status = diag.discriminant_str().to_string();
    let pico_diag_text = diag.stub_text();
    capture_log.record(
        format!("per_pico.{uid}.pico_diag"),
        started,
        &pico_diag_status,
        pico_diag_text.len(),
        diag.source_str().unwrap_or(""),
    );

    let usb = probe_usb_diag(uid, target, capture_log).await;
    if let Some(diag) = usb.diag.as_ref() {
        snapshot = snapshot.with_usb_diag(diag, target.persona);
    }
    let bt = probe_bt_status(uid, target, capture_log).await;

    snapshot = snapshot.with_outcome(format!(
        "bundle: pico_state={pico_state_status}; pico_diag={pico_diag_status}; usb_diag={}; bt_status_cdc={}",
        usb.status, bt.status
    ));
    pico_cache::record(&snapshot);
    let initial_usb_capture_text = usb_packets_text_from_diag(uid, &pico_diag_text);
    let outputs = if target.persona.is_bluetooth() {
        let input = BluetoothReportInput {
            pico_state: pico_state.as_ref(),
            bt_status: bt.data.as_ref(),
            bt_status_error: bt.error,
            usb_diag: usb.diag.as_ref(),
            pico_diag_text: &pico_diag_text,
        };
        bluetooth_outputs(uid, path, target, input, capture_log)
    } else {
        adapter_outputs(uid, target, &pico_diag_text, usb.diag.as_ref(), capture_log).await
    };
    PicoCaptureParts {
        state_captured: true,
        state_json: state_json_from_snapshot(&snapshot),
        pico_diag_status,
        usb_diag_status: usb.status.to_string(),
        pico_state_status: pico_state_status.to_string(),
        pico_diag_text,
        usb_diag_text: usb.text,
        initial_usb_capture_text,
        outputs,
    }
}

pub(super) fn unattempted_parts(
    seed: &PicoCaptureSeed,
    state_captured: bool,
    diag_status: &str,
    state_status: &str,
    state_json: String,
) -> PicoCaptureParts {
    PicoCaptureParts {
        state_captured,
        state_json,
        pico_diag_status: diag_status.to_string(),
        usb_diag_status: diag_status.to_string(),
        pico_state_status: state_status.to_string(),
        pico_diag_text: offline_pico_text(seed, "firmware diag log"),
        usb_diag_text: offline_pico_text(seed, "USB counters"),
        initial_usb_capture_text: offline_pico_text(seed, "initial USB packet dump"),
        outputs: PicoReportOutputs::packets_only(offline_pico_text(seed, "USB packet dump")),
    }
}

pub(super) fn log_usb_packet_status(
    seed: &PicoCaptureSeed,
    usb_packets_text: &str,
    capture_log: &mut CaptureLog,
) -> (&'static str, usize) {
    let usb_packet_count = count_usb_packet_lines(usb_packets_text);
    let usb_packet_stats_count = count_usb_packet_stats_lines(usb_packets_text);
    let usb_packet_event_count = count_usb_packet_event_lines(usb_packets_text);
    let usb_packet_harvest_count = count_usb_packet_harvest_lines(usb_packets_text);
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
    (usb_packet_status, usb_packet_count)
}

pub(super) fn bluetooth_report_status_label(
    seed: &PicoCaptureSeed,
    report: Option<&BluetoothReport>,
) -> String {
    if let Some(report) = report {
        return report.status.to_string();
    }
    let bluetooth_target = seed
        .target
        .as_ref()
        .is_some_and(|target| target.persona.is_bluetooth());
    if bluetooth_target {
        "not_captured".to_string()
    } else {
        "not_applicable".to_string()
    }
}

pub(super) async fn capture_one_pico(
    seed: PicoCaptureSeed,
    capture_log: &mut CaptureLog,
) -> PicoBundleCapture {
    let path = format!("picos/{}", sanitize_path_component(&seed.uid));
    let parts = if let Some(target) = seed.target.as_ref() {
        capture_live_pico(&seed.uid, &path, target, capture_log).await
    } else if let Some(saved) = seed.saved.as_ref() {
        let snapshot = pico_cache::PicoStateSnapshot::offline_from_config("bundle-offline", saved);
        pico_cache::record(&snapshot);
        unattempted_parts(
            &seed,
            true,
            "offline_not_attempted",
            "offline_not_attempted",
            state_json_from_snapshot(&snapshot),
        )
    } else if let Some(cached) = seed.cached_state_json.as_ref() {
        unattempted_parts(
            &seed,
            true,
            "cache_only_not_attempted",
            "cache_only",
            cached.clone(),
        )
    } else {
        unattempted_parts(&seed, false, "no_state", "no_state", "{}\n".to_string())
    };
    let outputs = parts.outputs;
    let (usb_packet_status, usb_packet_count) =
        log_usb_packet_status(&seed, &outputs.usb_packets_text, capture_log);
    let bluetooth_report_status =
        bluetooth_report_status_label(&seed, outputs.bluetooth_report.as_ref());

    PicoBundleCapture {
        manifest: ManifestPicoCapture {
            peer: seed.target.as_ref().map(|target| target.peer.to_string()),
            live: seed.target.is_some(),
            cached_state_included: seed.cached_state_json.is_some(),
            uid: seed.uid,
            path,
            source: seed.source,
            state_captured: parts.state_captured,
            pico_diag_status: parts.pico_diag_status,
            usb_diag_status: parts.usb_diag_status,
            pico_state_status: parts.pico_state_status,
            usb_packet_dump_status: usb_packet_status.to_string(),
            usb_packet_dump_count: usb_packet_count,
            bluetooth_report_status,
        },
        state_json: parts.state_json,
        pico_diag_text: parts.pico_diag_text,
        usb_diag_text: parts.usb_diag_text,
        initial_usb_capture_text: parts.initial_usb_capture_text,
        usb_packets_text: outputs.usb_packets_text,
        adapter_survey_text: outputs.adapter_survey_text,
        adapter_survey_json: outputs.adapter_survey_json,
        adapter_survey_report: outputs.adapter_survey_report,
        bluetooth_report_text: outputs.bluetooth_report_text,
        bluetooth_report_json: outputs.bluetooth_report_json,
        bluetooth_report: outputs.bluetooth_report,
    }
}

pub(super) fn state_json_from_snapshot(snapshot: &pico_cache::PicoStateSnapshot) -> String {
    serde_json::to_string_pretty(snapshot).unwrap_or_else(|e| {
        format!("{{\"capture_outcome\":\"state_serialization_failed\",\"error\":\"{e}\"}}\n")
    })
}

pub(super) fn offline_pico_text(seed: &PicoCaptureSeed, artifact: &str) -> String {
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
