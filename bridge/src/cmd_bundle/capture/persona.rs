use super::log::CaptureLog;
use super::{
    BUNDLE_DEBUG_PACKET_HARVEST_TIMEOUT, BUNDLE_PERSONA_WAIT, BUNDLE_RESTORE_PERSONA_WAIT,
};
use crate::cmd_bundle::adapter_survey::AdapterSurveyRawCapture;
use crate::cmd_bundle::usb_packets::{
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, duration_ms_u64, usb_packets_text_from_debug_snapshot,
    usb_packets_text_from_diag,
};
use crate::{cmd_auto, cmd_persona, cmd_run, debug_packets, pico_cache, pico_mode, protocol};
use anyhow::Result;
use std::fmt::Write as _;
use std::time::Instant;

pub(super) struct PersonaPacketCapture {
    pub(in crate::cmd_bundle) text: String,
    pub(super) raw_capture: AdapterSurveyRawCapture,
    pub(super) capture_target: Option<cmd_run::PicoTarget>,
}

pub(super) async fn enable_capture_persona(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    capture_log: &mut CaptureLog,
) -> Result<cmd_run::PicoTarget, &'static str> {
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
            return Err("request_failed");
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
            return Err("wait_failed");
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
        if found.as_ref().is_some_and(|pico| pico.persona == persona) {
            "confirmed"
        } else {
            "not_confirmed"
        },
        matched.len(),
        format!("observed={}", format_observed_personas(&matched)),
    );
    found
        .filter(|pico| pico.persona == persona)
        .ok_or("not_confirmed")
}

pub(super) async fn capture_persona_usb_packets(
    uid: &str,
    target: &cmd_run::PicoTarget,
    persona: protocol::Persona,
    fallback_diag_text: &str,
    capture_log: &mut CaptureLog,
) -> PersonaPacketCapture {
    let capture_target = match enable_capture_persona(uid, target, persona, capture_log).await {
        Ok(capture_target) => capture_target,
        Err(status) => {
            return PersonaPacketCapture {
                text: String::new(),
                raw_capture: AdapterSurveyRawCapture::not_attempted(status),
                capture_target: None,
            };
        }
    };

    capture_log.record_duration(
        format!(
            "per_pico.{uid}.adapter_survey.{}.capture_settle",
            persona.label()
        ),
        u64::try_from(cmd_auto::USB_SETTLE.as_millis()).unwrap_or(u64::MAX),
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

pub(super) async fn harvest_usb_packets_for_target(
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

pub(super) async fn restore_persona_after_bundle(
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
                    &pico_cache::PicoStateSnapshot::from_target("bundle-restore", restored)
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

pub(super) fn format_observed_personas(targets: &[cmd_run::PicoTarget]) -> String {
    if targets.is_empty() {
        return "none".to_string();
    }
    targets
        .iter()
        .map(|target| format!("{}:{}", target.uid_hex(), target.persona.label()))
        .collect::<Vec<_>>()
        .join(",")
}
