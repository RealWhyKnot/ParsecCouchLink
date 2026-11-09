use super::log::CaptureLog;
use super::{
    BUNDLE_PERSONA_WAIT, BundleUsbPacketCapture, PersonaPacketCapture, capture_persona_usb_packets,
    format_observed_personas, harvest_usb_packets_for_target, restore_persona_after_bundle,
};
use crate::cmd_bundle::adapter_survey::{
    AdapterSurveyRawCapture, SurveyStep, adapter_survey_candidates, adapter_survey_report_json,
    adapter_survey_text, build_adapter_survey_report, diag_has_usb_host_traffic,
    survey_attempt_from_diag, survey_diag_accepted,
};
use crate::cmd_bundle::usb_packets::{
    count_usb_packet_event_lines, count_usb_packet_harvest_lines, count_usb_packet_lines,
    count_usb_packet_stats_lines, usb_packets_text_from_diag,
};
use crate::{cmd_auto, cmd_persona, cmd_run, cmd_usb_diag, pico_mode, protocol};
use std::time::Instant;

pub(super) struct AdapterSurveyRun<'a> {
    uid: &'a str,
    fallback_diag_text: &'a str,
    current: cmd_run::PicoTarget,
    attempts: Vec<crate::cmd_bundle::adapter_survey::AdapterSurveyAttempt>,
    capture_sections: Vec<String>,
    capture_target: Option<cmd_run::PicoTarget>,
}

impl AdapterSurveyRun<'_> {
    fn absorb(&mut self, captured: PersonaPacketCapture) -> AdapterSurveyRawCapture {
        if let Some(target) = captured.capture_target {
            self.current = target.clone();
            self.capture_target = Some(target);
        }
        if !captured.text.is_empty() {
            self.capture_sections.push(captured.text);
        }
        captured.raw_capture
    }

    async fn capture_current(
        &mut self,
        target: &cmd_run::PicoTarget,
        capture_log: &mut CaptureLog,
    ) -> AdapterSurveyRawCapture {
        let captured = if target.persona == protocol::Persona::Debug {
            let text = harvest_usb_packets_for_target(
                self.uid,
                target,
                self.fallback_diag_text,
                capture_log,
            )
            .await;
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
                self.uid,
                target,
                target.persona,
                self.fallback_diag_text,
                capture_log,
            )
            .await
        };
        self.absorb(captured)
    }

    async fn survey_current(
        &mut self,
        target: &cmd_run::PicoTarget,
        current_diag: Option<&protocol::UsbDiag>,
        capture_log: &mut CaptureLog,
    ) -> bool {
        let current_needs_capture = current_diag.is_some_and(|diag| {
            diag.device_desc_count > 0 && !survey_diag_accepted(target.persona, diag)
        });
        let current_raw_capture = if current_needs_capture {
            self.capture_current(target, capture_log).await
        } else {
            AdapterSurveyRawCapture::not_attempted("not_needed")
        };

        let current_attempt = survey_attempt_from_diag(
            target.persona,
            SurveyStep::CurrentPersona,
            current_diag,
            current_raw_capture,
        );
        let current_has_no_usb_host =
            current_diag.is_some_and(|diag| !diag_has_usb_host_traffic(diag));
        let current_accepted = current_attempt.accepted;
        self.attempts.push(current_attempt);

        if current_has_no_usb_host {
            capture_log.record_duration(
                format!("per_pico.{}.adapter_survey.current", self.uid),
                0,
                "no_usb_host_traffic",
                0,
                "current USB diagnostic had no descriptor, mount, suspend, report, or OUT traffic",
            );
        }
        current_accepted
    }

    async fn survey_candidate(
        &mut self,
        candidate: protocol::Persona,
        capture_log: &mut CaptureLog,
    ) -> bool {
        let uid = self.uid;
        let switched = self.current.persona != candidate;
        let Some(active) =
            switch_to_survey_persona(uid, self.current.clone(), candidate, capture_log).await
        else {
            self.attempts.push(survey_attempt_from_diag(
                candidate,
                SurveyStep::Candidate { switched },
                None,
                AdapterSurveyRawCapture::not_attempted("switch_failed"),
            ));
            return false;
        };
        self.current = active.clone();

        capture_log.record_duration(
            format!(
                "per_pico.{uid}.adapter_survey.{}.usb_settle",
                candidate.label()
            ),
            u64::try_from(cmd_auto::USB_SETTLE.as_millis()).unwrap_or(u64::MAX),
            "sleep",
            0,
            "allow adapter USB host detection",
        );
        tokio::time::sleep(cmd_auto::USB_SETTLE).await;

        let diag = query_survey_usb_diag(uid, &active, candidate, capture_log).await;
        let needs_capture = diag.as_ref().is_some_and(|diag| {
            diag.device_desc_count > 0 && !survey_diag_accepted(candidate, diag)
        });
        let raw_capture = if needs_capture {
            let captured = capture_persona_usb_packets(
                uid,
                &active,
                candidate,
                self.fallback_diag_text,
                capture_log,
            )
            .await;
            self.absorb(captured)
        } else {
            AdapterSurveyRawCapture::not_attempted("not_needed")
        };

        let attempt = survey_attempt_from_diag(
            candidate,
            SurveyStep::Candidate { switched },
            diag.as_ref(),
            raw_capture,
        );
        let accepted = attempt.accepted;
        self.attempts.push(attempt);
        if accepted {
            capture_log.record_duration(
                format!("per_pico.{uid}.adapter_survey.stop"),
                0,
                "accepted",
                0,
                format!("persona={}", candidate.label()),
            );
        }
        accepted
    }
}

pub(super) async fn bundle_usb_packets_for_target(
    uid: &str,
    target: &cmd_run::PicoTarget,
    fallback_diag_text: &str,
    current_diag: Option<&protocol::UsbDiag>,
    capture_log: &mut CaptureLog,
) -> BundleUsbPacketCapture {
    let original_persona = target.persona;
    let mut run = AdapterSurveyRun {
        uid,
        fallback_diag_text,
        current: target.clone(),
        attempts: Vec::new(),
        capture_sections: Vec::new(),
        capture_target: None,
    };
    let current_accepted = run.survey_current(target, current_diag, capture_log).await;

    for candidate in adapter_survey_candidates(target.persona, current_accepted) {
        if run.survey_candidate(candidate, capture_log).await {
            break;
        }
    }

    let restore_status =
        restore_persona_after_bundle(uid, &run.current, original_persona, capture_log).await;
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
        run.attempts,
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
    for section in run.capture_sections {
        text.push('\n');
        text.push_str(&section);
    }
    BundleUsbPacketCapture {
        text,
        capture_target: run.capture_target,
        adapter_survey_text,
        adapter_survey_json,
        adapter_survey_report: Some(report),
    }
}

pub(super) async fn switch_to_survey_persona(
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
        if found.as_ref().is_some_and(|pico| pico.persona == candidate) {
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

pub(super) async fn query_survey_usb_diag(
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
