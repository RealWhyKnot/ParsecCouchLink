//! Adapter persona survey reports and derived connection verdicts.

use std::fmt::Write as _;

use anyhow::Result;
use serde::Serialize;

use crate::{cmd_auto, protocol};

use super::usb_packets::count_usb_packet_lines;
use super::PicoBundleCapture;

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
    pub(super) usb_diag_captured: bool,
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
    pub(super) accepted: bool,
    pub(super) host_traffic_seen: bool,
    pub(super) descriptor_or_report_rejected: bool,
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
    current_at_start: bool,
    switched: bool,
    diag: Option<protocol::UsbDiag>,
    raw_capture: AdapterSurveyRawCapture,
) -> AdapterSurveyAttempt {
    let score = diag
        .as_ref()
        .map(cmd_auto::score_usb_diag)
        .unwrap_or(cmd_auto::AutoScore::NoUsbTraffic);
    let accepted = diag
        .as_ref()
        .map(|diag| survey_diag_accepted(persona, diag))
        .unwrap_or(false);
    AdapterSurveyAttempt {
        persona: persona.label().to_string(),
        current_at_start,
        switched,
        usb_diag_captured: diag.is_some(),
        score_rank: score as u8,
        score: cmd_auto::score_label(score).to_string(),
        accepted,
        verdict: adapter_survey_verdict(persona, diag.as_ref(), score, accepted).to_string(),
        device_desc_count: diag
            .as_ref()
            .map(|diag| diag.device_desc_count)
            .unwrap_or(0),
        config_desc_count: diag
            .as_ref()
            .map(|diag| diag.config_desc_count)
            .unwrap_or(0),
        mount_count: diag.as_ref().map(|diag| diag.mount_count).unwrap_or(0),
        umount_count: diag.as_ref().map(|diag| diag.umount_count).unwrap_or(0),
        suspend_count: diag.as_ref().map(|diag| diag.suspend_count).unwrap_or(0),
        resume_count: diag.as_ref().map(|diag| diag.resume_count).unwrap_or(0),
        input_report_sent_count: diag
            .as_ref()
            .map(|diag| diag.xinput_in_sent_count)
            .unwrap_or(0),
        host_out_count: diag.as_ref().map(|diag| diag.xinput_out_count).unwrap_or(0),
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
        if attempt.persona == protocol::Persona::Debug.label() || !attempt.usb_diag_captured {
            continue;
        }
        let replace = best
            .map(|existing| {
                (attempt.accepted && !existing.accepted)
                    || (attempt.accepted == existing.accepted
                        && attempt.score_rank > existing.score_rank)
            })
            .unwrap_or(true);
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
        .filter(|attempt| !attempt.usb_diag_captured)
        .map(|attempt| attempt.persona.clone())
        .collect::<Vec<_>>();
    let current_no_usb_host_traffic = attempts
        .iter()
        .find(|attempt| attempt.current_at_start)
        .map(|attempt| attempt.usb_diag_captured && !attempt_has_usb_host_traffic(attempt))
        .unwrap_or(false);
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
            attempt.usb_diag_captured,
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
            "{{\"artifact_schema_version\":1,\"error\":\"adapter survey serialization failed: {}\"}}\n",
            e
        )
    })
}

pub(super) fn adapter_connection_report(captures: &[PicoBundleCapture]) -> AdapterConnectionReport {
    let live_pico_count = captures
        .iter()
        .filter(|capture| capture.manifest.live)
        .count();
    let mut per_pico = Vec::new();
    for capture in captures {
        let Some(report) = capture.adapter_survey_report.as_ref() else {
            continue;
        };
        let attempts = report.attempts.len();
        let accepted = report.attempts.iter().any(|attempt| attempt.accepted);
        let host_traffic_seen = report.attempts.iter().any(attempt_has_usb_host_traffic);
        let descriptor_or_report_rejected = report
            .attempts
            .iter()
            .any(|attempt| attempt.verdict == "descriptor_or_report_rejected");
        let usb_diag_missing = report
            .attempts
            .iter()
            .any(|attempt| !attempt.usb_diag_captured);
        let device_desc_total = report
            .attempts
            .iter()
            .map(|attempt| attempt.device_desc_count as u64)
            .sum();
        let config_desc_total = report
            .attempts
            .iter()
            .map(|attempt| attempt.config_desc_count as u64)
            .sum();
        let mount_total = report
            .attempts
            .iter()
            .map(|attempt| attempt.mount_count as u64)
            .sum();
        let raw_packet_lines = count_usb_packet_lines(&capture.usb_packets_text);
        let status = if accepted {
            "adapter_accepted"
        } else if descriptor_or_report_rejected {
            "descriptor_or_report_rejected"
        } else if host_traffic_seen {
            "usb_host_traffic_seen"
        } else if usb_diag_missing {
            "usb_diag_incomplete"
        } else {
            "no_usb_host_traffic"
        };
        per_pico.push(AdapterConnectionPico {
            uid: capture.manifest.uid.clone(),
            path: capture.manifest.path.clone(),
            live: capture.manifest.live,
            status,
            warning: status == "no_usb_host_traffic",
            attempts,
            coverage_status: report.coverage_status.to_string(),
            stop_reason: report.stop_reason.to_string(),
            restore_status: report.restore_status.clone(),
            attempted_personas: report.attempted_personas.clone(),
            missing_adapter_personas: report.missing_adapter_personas.clone(),
            failed_usb_diag_personas: report.failed_usb_diag_personas.clone(),
            accepted,
            host_traffic_seen,
            descriptor_or_report_rejected,
            usb_diag_missing,
            device_desc_total,
            config_desc_total,
            mount_total,
            raw_packet_lines,
        });
    }

    let surveyed_live_pico_count = per_pico.len();
    let no_usb_host_pico_count = per_pico
        .iter()
        .filter(|pico| pico.status == "no_usb_host_traffic")
        .count();
    let host_traffic_pico_count = per_pico
        .iter()
        .filter(|pico| pico.host_traffic_seen)
        .count();
    let accepted_pico_count = per_pico.iter().filter(|pico| pico.accepted).count();
    let descriptor_or_report_rejected_pico_count = per_pico
        .iter()
        .filter(|pico| pico.descriptor_or_report_rejected)
        .count();

    let status = if surveyed_live_pico_count == 0 {
        "not_checked"
    } else if accepted_pico_count > 0 {
        "adapter_accepted"
    } else if descriptor_or_report_rejected_pico_count > 0 {
        "descriptor_or_report_rejected"
    } else if host_traffic_pico_count > 0 {
        "usb_host_traffic_seen"
    } else if no_usb_host_pico_count == surveyed_live_pico_count {
        "no_usb_host_traffic"
    } else {
        "usb_diag_incomplete"
    };
    let warning = status == "no_usb_host_traffic";
    AdapterConnectionReport {
        artifact_schema_version: 1,
        status,
        warning,
        surveyed_live_pico_count,
        live_pico_count,
        no_usb_host_pico_count,
        host_traffic_pico_count,
        accepted_pico_count,
        descriptor_or_report_rejected_pico_count,
        per_pico,
        next_steps: adapter_connection_next_steps(status),
        notes: vec![
            "This verdict is based on live Pico USB counters captured during bundle.",
            "It does not infer physical cabling when no live Pico survey was captured.",
            "device_desc_count=0 means the Pico did not observe USB host enumeration traffic.",
            "Run bundle with the Pico connected to the console adapter and console USB host you want to support.",
        ],
    }
}

pub(super) fn adapter_connection_next_steps(status: &str) -> Vec<&'static str> {
    match status {
        "no_usb_host_traffic" => vec![
            "Confirm adapter-survey.txt lists PS3, generic HID, PS4, keyboard, XInput, Xbox One, and Maple attempts. If any are missing, use bundle-capture.txt to find the failed switch or USB diagnostic step.",
            "If every attempted persona reports device_desc_count=0, the Pico did not observe the console adapter as a USB host during the survey window.",
            "Plug the Pico into the console adapter and console USB host you want it to work on, then run couchlink bundle again.",
            "If the adapter only handshakes once, power-cycle or physically replug the console-side adapter path before running bundle.",
            "A bundle taken with no USB host traffic cannot prove whether PS3, generic HID, PS4, keyboard, XInput, Xbox One, or Maple personas work with the adapter.",
        ],
        "descriptor_or_report_rejected" => vec![
            "Keep the full bundle; descriptor traffic was seen, so adapter firmware or report-shape work can use this evidence.",
            "Review initial-usb-capture.txt and usb-enumeration-analysis.txt before changing descriptors.",
        ],
        "adapter_accepted" => vec![
            "Use adapter-survey.txt for the accepted persona and score.",
            "Use usb-enumeration-analysis.txt if runtime input still fails after configuration.",
        ],
        "not_checked" => vec![
            "No live Pico adapter survey was captured, so this bundle cannot determine console-adapter connection state.",
        ],
        _ => vec![
            "Check bundle-capture.txt for USB diagnostic failures before repeating the run.",
        ],
    }
}

pub(super) fn adapter_connection_text(report: &AdapterConnectionReport) -> String {
    let mut out = String::from("Adapter connection verdict\n\n");
    let _ = writeln!(out, "status={}", report.status);
    let _ = writeln!(out, "warning={}", report.warning);
    let _ = writeln!(out, "live_pico_count={}", report.live_pico_count);
    let _ = writeln!(
        out,
        "surveyed_live_pico_count={}",
        report.surveyed_live_pico_count
    );
    let _ = writeln!(
        out,
        "no_usb_host_pico_count={}",
        report.no_usb_host_pico_count
    );
    let _ = writeln!(
        out,
        "host_traffic_pico_count={}",
        report.host_traffic_pico_count
    );
    let _ = writeln!(out, "accepted_pico_count={}", report.accepted_pico_count);
    let _ = writeln!(
        out,
        "descriptor_or_report_rejected_pico_count={}",
        report.descriptor_or_report_rejected_pico_count
    );
    let _ = writeln!(out);
    if report.warning {
        out.push_str("warning_text=No USB host enumeration traffic was observed from a live Pico. This bundle will not contain the adapter diagnostics needed to prove console-adapter support.\n\n");
    }
    out.push_str("next_steps=\n");
    for step in &report.next_steps {
        let _ = writeln!(out, "- {step}");
    }
    let _ = writeln!(out);
    out.push_str("per_pico=\n");
    if report.per_pico.is_empty() {
        out.push_str("- none\n");
    } else {
        for pico in &report.per_pico {
            let _ = writeln!(
                out,
                "- uid={} status={} warning={} attempts={} coverage_status={} stop_reason={} restore_status={} attempted_personas={} missing_adapter_personas={} failed_usb_diag_personas={} accepted={} host_traffic_seen={} descriptor_or_report_rejected={} usb_diag_missing={} device_desc_total={} config_desc_total={} mount_total={} raw_packet_lines={} path={}",
                pico.uid,
                pico.status,
                pico.warning,
                pico.attempts,
                pico.coverage_status,
                pico.stop_reason,
                pico.restore_status,
                format_string_list_or_none(&pico.attempted_personas),
                format_string_list_or_none(&pico.missing_adapter_personas),
                format_string_list_or_none(&pico.failed_usb_diag_personas),
                pico.accepted,
                pico.host_traffic_seen,
                pico.descriptor_or_report_rejected,
                pico.usb_diag_missing,
                pico.device_desc_total,
                pico.config_desc_total,
                pico.mount_total,
                pico.raw_packet_lines,
                pico.path
            );
        }
    }
    out
}

pub(super) fn adapter_connection_json(report: &AdapterConnectionReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}
pub(super) fn aggregate_adapter_survey_text(captures: &[PicoBundleCapture]) -> String {
    let mut out = String::from("Aggregate adapter persona survey\n\n");
    let mut count = 0usize;
    for capture in captures {
        if capture.adapter_survey_text.is_empty() {
            continue;
        }
        count += 1;
        let _ = writeln!(
            out,
            "## uid={} path={}/adapter-survey.txt",
            capture.manifest.uid, capture.manifest.path
        );
        out.push_str(&capture.adapter_survey_text);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    if count == 0 {
        out.push_str("No live Pico adapter survey was captured.\n");
    }
    out
}
#[derive(Serialize)]
pub(super) struct AdapterSurveyBundleReport<'a> {
    pub(super) artifact_schema_version: u8,
    pub(super) survey_count: usize,
    pub(super) best_candidate: Option<AdapterSurveyBundleBest>,
    pub(super) per_pico: Vec<&'a AdapterSurveyReport>,
    pub(super) notes: Vec<&'static str>,
}

#[derive(Serialize)]
pub(super) struct AdapterSurveyBundleBest {
    pub(super) uid: String,
    pub(super) path: String,
    pub(super) persona: String,
    pub(super) score_rank: u8,
    pub(super) score: String,
    pub(super) accepted: bool,
    pub(super) verdict: String,
}

pub(super) fn adapter_survey_bundle_json(captures: &[PicoBundleCapture]) -> Result<String> {
    let per_pico = captures
        .iter()
        .filter_map(|capture| capture.adapter_survey_report.as_ref())
        .collect::<Vec<_>>();
    let report = AdapterSurveyBundleReport {
        artifact_schema_version: 1,
        survey_count: per_pico.len(),
        best_candidate: best_adapter_survey_bundle_candidate(captures),
        per_pico,
        notes: vec![
            "The survey is non-interactive and restores the original persona after the bundle pass.",
            "expected_adapter_personas, attempted_personas, missing_adapter_personas, failed_usb_diag_personas, coverage_status, and stop_reason describe whether the bundle captured a complete survey.",
            "Accepted personas reached configured or polling state according to firmware USB counters.",
            "Debug mode is retained only as debug/XInput evidence and is not selected as adapter proof.",
        ],
    };
    Ok(serde_json::to_string_pretty(&report)?)
}

pub(super) fn best_adapter_survey_bundle_candidate(
    captures: &[PicoBundleCapture],
) -> Option<AdapterSurveyBundleBest> {
    let mut best: Option<AdapterSurveyBundleBest> = None;
    for capture in captures {
        let Some(report) = capture.adapter_survey_report.as_ref() else {
            continue;
        };
        let Some(candidate) = report.best_candidate.as_ref() else {
            continue;
        };
        let candidate = AdapterSurveyBundleBest {
            uid: capture.manifest.uid.clone(),
            path: capture.manifest.path.clone(),
            persona: candidate.persona.clone(),
            score_rank: candidate.score_rank,
            score: candidate.score.clone(),
            accepted: candidate.accepted,
            verdict: candidate.verdict.clone(),
        };
        let replace = best
            .as_ref()
            .map(|existing| {
                (candidate.accepted && !existing.accepted)
                    || (candidate.accepted == existing.accepted
                        && candidate.score_rank > existing.score_rank)
            })
            .unwrap_or(true);
        if replace {
            best = Some(candidate);
        }
    }
    best
}
