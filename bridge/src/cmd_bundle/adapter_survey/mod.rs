mod bundle;
mod connection;

use std::fmt::Write as _;

use serde::Serialize;

use crate::{cmd_auto, protocol};

use super::flags::Capture;
pub(super) use bundle::{adapter_survey_bundle_json, aggregate_adapter_survey_text};
pub(super) use connection::{
    adapter_connection_json, adapter_connection_report, adapter_connection_text,
};

const ADAPTER_SURVEY_PERSONAS: &[protocol::Persona] = &[
    protocol::Persona::Ps3,
    protocol::Persona::GenericHid,
    protocol::Persona::Ps4,
    protocol::Persona::Keyboard,
    protocol::Persona::Xinput,
    protocol::Persona::XboxOne,
    protocol::Persona::Maple,
];

#[derive(Clone, Debug, Serialize)]
pub(super) struct AdapterSurveyReport {
    pub(super) artifact_schema_version: u8,
    pub(super) uid: String,
    pub(super) original_persona: String,
    pub(super) restore_status: String,
    pub(super) restored_persona: Option<String>,
    pub(super) expected_adapter_personas: Vec<String>,
    pub(super) attempted_personas: Vec<String>,
    pub(super) missing_adapter_personas: Vec<String>,
    pub(super) failed_usb_diag_personas: Vec<String>,
    pub(super) current_no_usb_host_traffic: bool,
    pub(super) coverage_status: &'static str,
    pub(super) stop_reason: &'static str,
    pub(super) best_candidate: Option<AdapterSurveyBest>,
    pub(super) attempts: Vec<AdapterSurveyAttempt>,
    pub(super) notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AdapterSurveyBest {
    pub(super) persona: String,
    pub(super) score_rank: u8,
    pub(super) score: String,
    pub(super) accepted: bool,
    pub(super) verdict: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AdapterSurveyAttempt {
    pub(super) persona: String,
    pub(super) current_at_start: bool,
    pub(super) switched: bool,
    pub(super) usb_diag_captured: Capture,
    pub(super) score_rank: u8,
    pub(super) score: String,
    pub(super) accepted: bool,
    pub(super) verdict: String,
    pub(super) device_desc_count: u32,
    pub(super) config_desc_count: u32,
    pub(super) mount_count: u32,
    pub(super) umount_count: u32,
    pub(super) suspend_count: u32,
    pub(super) resume_count: u32,
    pub(super) input_report_sent_count: u32,
    pub(super) host_out_count: u32,
    pub(super) raw_capture: AdapterSurveyRawCapture,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AdapterSurveyRawCapture {
    pub(super) attempted: bool,
    pub(super) status: String,
    pub(super) raw_packet_lines: usize,
    pub(super) packet_stats_lines: usize,
    pub(super) usb_event_lines: usize,
    pub(super) harvest_lines: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AdapterConnectionReport {
    pub(super) artifact_schema_version: u8,
    pub(super) status: &'static str,
    pub(super) warning: bool,
    pub(super) surveyed_live_pico_count: usize,
    pub(super) live_pico_count: usize,
    pub(super) no_usb_host_pico_count: usize,
    pub(super) host_traffic_pico_count: usize,
    pub(super) accepted_pico_count: usize,
    pub(super) descriptor_or_report_rejected_pico_count: usize,
    pub(super) per_pico: Vec<AdapterConnectionPico>,
    pub(super) next_steps: Vec<&'static str>,
    pub(super) notes: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct AdapterHostResponse {
    pub(super) accepted: bool,
    pub(super) host_traffic_seen: bool,
    pub(super) descriptor_or_report_rejected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SurveyStep {
    CurrentPersona,
    Candidate { switched: bool },
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AdapterConnectionPico {
    pub(super) uid: String,
    pub(super) path: String,
    pub(super) live: bool,
    pub(super) status: &'static str,
    pub(super) warning: bool,
    pub(super) attempts: usize,
    pub(super) coverage_status: String,
    pub(super) stop_reason: String,
    pub(super) restore_status: String,
    pub(super) attempted_personas: Vec<String>,
    pub(super) missing_adapter_personas: Vec<String>,
    pub(super) failed_usb_diag_personas: Vec<String>,
    #[serde(flatten)]
    pub(super) host: AdapterHostResponse,
    pub(super) usb_diag_missing: bool,
    pub(super) device_desc_total: u64,
    pub(super) config_desc_total: u64,
    pub(super) mount_total: u64,
    pub(super) raw_packet_lines: usize,
}

impl AdapterSurveyRawCapture {
    pub(super) fn not_attempted(status: &str) -> Self {
        Self {
            attempted: false,
            status: status.to_string(),
            raw_packet_lines: 0,
            packet_stats_lines: 0,
            usb_event_lines: 0,
            harvest_lines: 0,
        }
    }
}

pub(super) fn survey_attempt_from_diag(
    persona: protocol::Persona,
    step: SurveyStep,
    diag: Option<&protocol::UsbDiag>,
    raw_capture: AdapterSurveyRawCapture,
) -> AdapterSurveyAttempt {
    let score = diag.map_or(cmd_auto::AutoScore::NoUsbTraffic, cmd_auto::score_usb_diag);
    let accepted = diag.is_some_and(|diag| survey_diag_accepted(persona, diag));
    AdapterSurveyAttempt {
        persona: persona.label().to_string(),
        current_at_start: step == SurveyStep::CurrentPersona,
        switched: matches!(step, SurveyStep::Candidate { switched: true }),
        usb_diag_captured: diag.is_some().into(),
        score_rank: score.rank(),
        score: cmd_auto::score_label(score).to_string(),
        accepted,
        verdict: adapter_survey_verdict(persona, diag, score, accepted).to_string(),
        device_desc_count: diag.map_or(0, |diag| diag.device_desc_count),
        config_desc_count: diag.map_or(0, |diag| diag.config_desc_count),
        mount_count: diag.map_or(0, |diag| diag.mount_count),
        umount_count: diag.map_or(0, |diag| diag.umount_count),
        suspend_count: diag.map_or(0, |diag| diag.suspend_count),
        resume_count: diag.map_or(0, |diag| diag.resume_count),
        input_report_sent_count: diag.map_or(0, |diag| diag.xinput_in_sent_count),
        host_out_count: diag.map_or(0, |diag| diag.xinput_out_count),
        raw_capture,
    }
}

pub(super) fn survey_diag_accepted(persona: protocol::Persona, diag: &protocol::UsbDiag) -> bool {
    persona != protocol::Persona::Debug
        && cmd_auto::adapter_accepts_score(cmd_auto::score_usb_diag(diag))
}

pub(super) fn diag_has_usb_host_traffic(diag: &protocol::UsbDiag) -> bool {
    diag.device_desc_count > 0
        || diag.config_desc_count > 0
        || diag.mount_count > 0
        || diag.umount_count > 0
        || diag.suspend_count > 0
        || diag.resume_count > 0
        || diag.xinput_in_sent_count > 0
        || diag.xinput_out_count > 0
}

pub(super) fn adapter_survey_candidates(
    current: protocol::Persona,
    current_accepted: bool,
) -> Vec<protocol::Persona> {
    if current_accepted && current != protocol::Persona::Debug {
        return Vec::new();
    }

    ADAPTER_SURVEY_PERSONAS
        .iter()
        .copied()
        .filter(|persona| *persona != current)
        .collect()
}

pub(super) fn attempt_has_usb_host_traffic(attempt: &AdapterSurveyAttempt) -> bool {
    attempt.device_desc_count > 0
        || attempt.config_desc_count > 0
        || attempt.mount_count > 0
        || attempt.umount_count > 0
        || attempt.suspend_count > 0
        || attempt.resume_count > 0
        || attempt.input_report_sent_count > 0
        || attempt.host_out_count > 0
}

pub(super) fn adapter_survey_verdict(
    persona: protocol::Persona,
    diag: Option<&protocol::UsbDiag>,
    score: cmd_auto::AutoScore,
    accepted: bool,
) -> &'static str {
    if accepted {
        return "accepted_by_adapter";
    }
    let Some(diag) = diag else {
        return "usb_diag_not_captured";
    };
    if persona == protocol::Persona::Debug && cmd_auto::adapter_accepts_score(score) {
        return "debug_xinput_evidence_only";
    }
    if diag.device_desc_count == 0 {
        return "adapter_did_not_enumerate";
    }
    "descriptor_or_report_rejected"
}

pub(super) fn best_adapter_survey_candidate(
    attempts: &[AdapterSurveyAttempt],
) -> Option<AdapterSurveyBest> {
    let mut best: Option<&AdapterSurveyAttempt> = None;
    for attempt in attempts {
        if attempt.persona == protocol::Persona::Debug.label()
            || !attempt.usb_diag_captured.is_captured()
        {
            continue;
        }
        let replace = best.is_none_or(|existing| {
            (attempt.accepted && !existing.accepted)
                || (attempt.accepted == existing.accepted
                    && attempt.score_rank > existing.score_rank)
        });
        if replace {
            best = Some(attempt);
        }
    }
    best.map(|attempt| AdapterSurveyBest {
        persona: attempt.persona.clone(),
        score_rank: attempt.score_rank,
        score: attempt.score.clone(),
        accepted: attempt.accepted,
        verdict: attempt.verdict.clone(),
    })
}

pub(super) fn build_adapter_survey_report(
    uid: String,
    original_persona: String,
    restore_status: String,
    restored_persona: Option<String>,
    attempts: Vec<AdapterSurveyAttempt>,
    notes: Vec<&'static str>,
) -> AdapterSurveyReport {
    let best_candidate = best_adapter_survey_candidate(&attempts);
    let expected_adapter_personas = adapter_survey_expected_personas();
    let attempted_personas = attempts
        .iter()
        .map(|attempt| attempt.persona.clone())
        .collect::<Vec<_>>();
    let missing_adapter_personas = expected_adapter_personas
        .iter()
        .filter(|persona| !attempted_personas.contains(persona))
        .cloned()
        .collect::<Vec<_>>();
    let failed_usb_diag_personas = attempts
        .iter()
        .filter(|attempt| !attempt.usb_diag_captured.is_captured())
        .map(|attempt| attempt.persona.clone())
        .collect::<Vec<_>>();
    let current_no_usb_host_traffic = attempts
        .iter()
        .find(|attempt| attempt.current_at_start)
        .is_some_and(|attempt| {
            attempt.usb_diag_captured.is_captured() && !attempt_has_usb_host_traffic(attempt)
        });
    let current_accepted = attempts
        .iter()
        .any(|attempt| attempt.current_at_start && attempt.accepted);
    let accepted_candidate = attempts
        .iter()
        .any(|attempt| !attempt.current_at_start && attempt.accepted);
    let coverage_status = if current_accepted || accepted_candidate {
        "stopped_after_acceptance"
    } else if missing_adapter_personas.is_empty() && failed_usb_diag_personas.is_empty() {
        "all_adapter_personas_attempted"
    } else {
        "incomplete"
    };
    let stop_reason = if current_accepted {
        "accepted_current_persona"
    } else if accepted_candidate {
        "accepted_candidate"
    } else if !missing_adapter_personas.is_empty() {
        "not_all_personas_attempted"
    } else if !failed_usb_diag_personas.is_empty() {
        "usb_diag_or_switch_failed"
    } else {
        "exhausted_candidates"
    };

    AdapterSurveyReport {
        artifact_schema_version: 1,
        uid,
        original_persona,
        restore_status,
        restored_persona,
        expected_adapter_personas,
        attempted_personas,
        missing_adapter_personas,
        failed_usb_diag_personas,
        current_no_usb_host_traffic,
        coverage_status,
        stop_reason,
        best_candidate,
        attempts,
        notes,
    }
}

pub(super) fn adapter_survey_expected_personas() -> Vec<String> {
    ADAPTER_SURVEY_PERSONAS
        .iter()
        .map(|persona| persona.label().to_string())
        .collect()
}

pub(super) fn format_string_list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(",")
    }
}

pub(super) fn adapter_survey_text(report: &AdapterSurveyReport) -> String {
    let mut out = String::from("Adapter persona survey\n\n");
    let _ = writeln!(out, "uid={}", report.uid);
    let _ = writeln!(out, "original_persona={}", report.original_persona);
    let _ = writeln!(out, "restore_status={}", report.restore_status);
    let _ = writeln!(
        out,
        "restored_persona={}",
        report.restored_persona.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(
        out,
        "expected_adapter_personas={}",
        format_string_list_or_none(&report.expected_adapter_personas)
    );
    let _ = writeln!(
        out,
        "attempted_personas={}",
        format_string_list_or_none(&report.attempted_personas)
    );
    let _ = writeln!(
        out,
        "missing_adapter_personas={}",
        format_string_list_or_none(&report.missing_adapter_personas)
    );
    let _ = writeln!(
        out,
        "failed_usb_diag_personas={}",
        format_string_list_or_none(&report.failed_usb_diag_personas)
    );
    let _ = writeln!(
        out,
        "current_no_usb_host_traffic={}",
        report.current_no_usb_host_traffic
    );
    let _ = writeln!(out, "coverage_status={}", report.coverage_status);
    let _ = writeln!(out, "stop_reason={}", report.stop_reason);
    if let Some(best) = report.best_candidate.as_ref() {
        let _ = writeln!(
            out,
            "selected_best={} accepted={} score_rank={} score={} verdict={}",
            best.persona, best.accepted, best.score_rank, best.score, best.verdict
        );
    } else {
        out.push_str(
            "selected_best=none accepted=false score_rank=0 score=none verdict=no_usb_diag\n",
        );
    }
    let _ = writeln!(out);
    out.push_str("attempts=\n");
    for attempt in &report.attempts {
        let _ = writeln!(
            out,
            "- persona={} current_at_start={} switched={} usb_diag_captured={} accepted={} verdict={} score_rank={} score={} device_desc_count={} config_desc_count={} mounts={} unmounts={} suspends={} resumes={} input_report_sent_count={} host_out_count={} raw_capture_attempted={} raw_packets={} events={} stats={} harvests={} raw_capture_status={}",
            attempt.persona,
            attempt.current_at_start,
            attempt.switched,
            attempt.usb_diag_captured.is_captured(),
            attempt.accepted,
            attempt.verdict,
            attempt.score_rank,
            attempt.score,
            attempt.device_desc_count,
            attempt.config_desc_count,
            attempt.mount_count,
            attempt.umount_count,
            attempt.suspend_count,
            attempt.resume_count,
            attempt.input_report_sent_count,
            attempt.host_out_count,
            attempt.raw_capture.attempted,
            attempt.raw_capture.raw_packet_lines,
            attempt.raw_capture.usb_event_lines,
            attempt.raw_capture.packet_stats_lines,
            attempt.raw_capture.harvest_lines,
            attempt.raw_capture.status
        );
    }
    let _ = writeln!(out);
    out.push_str("meaning=\n");
    out.push_str(
        "- accepted_by_adapter: adapter reached configured or polling state for that persona.\n",
    );
    out.push_str("- adapter_did_not_enumerate: device_desc_count was zero for that persona.\n");
    out.push_str("- descriptor_or_report_rejected: the adapter requested descriptors but did not keep the persona configured.\n");
    out.push_str("- debug_xinput_evidence_only: debug mode was observed, but it only proves the debug/XInput USB shape.\n");
    out
}

pub(super) fn adapter_survey_report_json(report: &AdapterSurveyReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|e| {
        format!(
            "{{\"artifact_schema_version\":1,\"error\":\"adapter survey serialization failed: {e}\"}}\n"
        )
    })
}
