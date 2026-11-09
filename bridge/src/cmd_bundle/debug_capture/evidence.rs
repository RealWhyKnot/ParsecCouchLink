use super::{
    debug_capture_evidence_grade, debug_capture_gate, debug_capture_missing_evidence_lines,
    debug_capture_overall_status, debug_capture_quality, debug_capture_summary_has_loss,
    state_json_persona,
};
use crate::cmd_bundle::usb_packet_summary::{
    UsbPacketBundleSummary, UsbPacketSummary, summarize_text,
};
use crate::cmd_bundle::{PicoBundleCapture, RetainedDebugPacketLog};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
pub(in crate::cmd_bundle) struct DebugCaptureEvidenceReport {
    pub(in crate::cmd_bundle) artifact_schema_version: u8,
    pub(in crate::cmd_bundle) overall_status: &'static str,
    pub(in crate::cmd_bundle) evidence_grade: &'static str,
    pub(in crate::cmd_bundle) capture_quality: &'static str,
    pub(in crate::cmd_bundle) adapter_reverse_engineering_gate: &'static str,
    pub(in crate::cmd_bundle) gate_reason: &'static str,
    pub(in crate::cmd_bundle) missing_evidence: Vec<&'static str>,
    pub(in crate::cmd_bundle) aggregate: UsbPacketSummary,
    pub(in crate::cmd_bundle) per_pico: Vec<DebugCaptureEvidencePico>,
    pub(in crate::cmd_bundle) retained_logs: Vec<DebugCaptureEvidenceRetainedLog>,
    pub(in crate::cmd_bundle) notes: Vec<&'static str>,
}

#[derive(Serialize)]
pub(in crate::cmd_bundle) struct DebugCaptureEvidencePico {
    pub(in crate::cmd_bundle) uid: String,
    pub(in crate::cmd_bundle) path: String,
    pub(in crate::cmd_bundle) peer: Option<String>,
    pub(in crate::cmd_bundle) live: bool,
    pub(in crate::cmd_bundle) source: String,
    pub(in crate::cmd_bundle) persona: Option<String>,
    pub(in crate::cmd_bundle) packet_status: String,
    pub(in crate::cmd_bundle) missing_evidence: Vec<&'static str>,
    pub(in crate::cmd_bundle) summary: UsbPacketSummary,
}

#[derive(Serialize)]
pub(in crate::cmd_bundle) struct DebugCaptureEvidenceRetainedLog {
    pub(in crate::cmd_bundle) name: String,
    pub(in crate::cmd_bundle) path: String,
    pub(in crate::cmd_bundle) missing_evidence: Vec<&'static str>,
    pub(in crate::cmd_bundle) summary: UsbPacketSummary,
}

pub(in crate::cmd_bundle) fn debug_capture_evidence_report_json(
    captures: &[PicoBundleCapture],
    retained_logs: &[RetainedDebugPacketLog],
    summary: &UsbPacketBundleSummary,
) -> Result<String> {
    let status = debug_capture_overall_status(summary, captures, retained_logs);
    let evidence_grade = debug_capture_evidence_grade(summary);
    let capture_quality = debug_capture_quality(summary);
    let (gate, gate_reason) = debug_capture_gate(summary);
    let debug_persona_captures = captures
        .iter()
        .filter(|capture| state_json_persona(&capture.state_json).as_deref() == Some("debug"))
        .count();
    let missing_evidence = debug_capture_missing_evidence_lines(
        summary,
        captures,
        retained_logs,
        debug_persona_captures,
    );
    let per_pico = captures
        .iter()
        .map(|capture| {
            let persona = state_json_persona(&capture.state_json);
            let source_summary = summarize_text(&capture.usb_packets_text);
            DebugCaptureEvidencePico {
                uid: capture.manifest.uid.clone(),
                path: capture.manifest.path.clone(),
                peer: capture.manifest.peer.clone(),
                live: capture.manifest.live,
                source: capture.manifest.source.clone(),
                persona: persona.clone(),
                packet_status: capture.manifest.usb_packet_dump_status.clone(),
                missing_evidence: debug_capture_source_missing_evidence(
                    &source_summary,
                    persona.as_deref(),
                    false,
                ),
                summary: source_summary,
            }
        })
        .collect();
    let retained_logs = retained_logs
        .iter()
        .map(|log| {
            let source_summary = summarize_text(&log.text);
            DebugCaptureEvidenceRetainedLog {
                name: log.name.clone(),
                path: format!("debug-packets/{}", log.name),
                missing_evidence: debug_capture_source_missing_evidence(
                    &source_summary,
                    None,
                    true,
                ),
                summary: source_summary,
            }
        })
        .collect();
    let report = DebugCaptureEvidenceReport {
        artifact_schema_version: 3,
        overall_status: status,
        evidence_grade,
        capture_quality,
        adapter_reverse_engineering_gate: gate,
        gate_reason,
        missing_evidence,
        aggregate: summary.aggregate.clone(),
        per_pico,
        retained_logs,
        notes: vec![
            "This file is machine-readable evidence for debug input packet capture quality.",
            "adapter_reverse_engineering_gate=pass requires raw debug input packet payload lines.",
            "Debug mode uses the XInput USB shape; adapter-survey.json records persona-specific adapter acceptance.",
            "capture_quality is lossy when packet payloads are truncated or GET_LOG harvests report lost bytes or missing chunks.",
            "Per-source summary counts are calculated independently; aggregate sequence gaps are summed per source.",
            "Raw packet dumps can come from debug input mode or a bundle-requested one-shot USB capture boot.",
            "USB lifecycle event rows can show mount, unmount, suspend, or resume even when raw packet payloads are absent.",
        ],
    };
    Ok(serde_json::to_string_pretty(&report)?)
}

pub(in crate::cmd_bundle) fn debug_capture_source_missing_evidence(
    summary: &UsbPacketSummary,
    persona: Option<&str>,
    retained_log: bool,
) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if summary.packet_lines == 0 {
        lines.push("raw USB packet payload lines from this source");
    }
    if summary.directions.get("setup").copied().unwrap_or(0) == 0
        && summary.directions.get("control-in").copied().unwrap_or(0) == 0
    {
        lines.push("USB setup/control-IN traffic from this source");
    }
    if summary.directions.get("in").copied().unwrap_or(0) == 0
        && summary.directions.get("out").copied().unwrap_or(0) == 0
    {
        lines.push("endpoint IN/OUT traffic from this source");
    }
    if !retained_log && persona != Some("debug") {
        lines.push("current state proving persona=debug for this Pico");
    }
    if summary.packet_lines == 0
        && summary.stats_lines == 0
        && summary.event_lines == 0
        && summary.harvest_lines == 0
    {
        lines.push("debug packet lifecycle, stats, or harvest records from this source");
    }
    if debug_capture_summary_has_loss(summary) {
        lines.push("lossless packet payload and harvest capture from this source");
    }
    if lines.is_empty() {
        lines.push("none");
    }
    lines
}
