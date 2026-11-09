use super::AdapterSurveyReport;
use crate::cmd_bundle::PicoBundleCapture;
use anyhow::Result;
use serde::Serialize;
use std::fmt::Write as _;

pub(in crate::cmd_bundle) fn aggregate_adapter_survey_text(
    captures: &[PicoBundleCapture],
) -> String {
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
pub(in crate::cmd_bundle) struct AdapterSurveyBundleReport<'a> {
    pub(in crate::cmd_bundle) artifact_schema_version: u8,
    pub(in crate::cmd_bundle) survey_count: usize,
    pub(in crate::cmd_bundle) best_candidate: Option<AdapterSurveyBundleBest>,
    pub(in crate::cmd_bundle) per_pico: Vec<&'a AdapterSurveyReport>,
    pub(in crate::cmd_bundle) notes: Vec<&'static str>,
}

#[derive(Serialize)]
pub(in crate::cmd_bundle) struct AdapterSurveyBundleBest {
    pub(in crate::cmd_bundle) uid: String,
    pub(in crate::cmd_bundle) path: String,
    pub(in crate::cmd_bundle) persona: String,
    pub(in crate::cmd_bundle) score_rank: u8,
    pub(in crate::cmd_bundle) score: String,
    pub(in crate::cmd_bundle) accepted: bool,
    pub(in crate::cmd_bundle) verdict: String,
}

pub(in crate::cmd_bundle) fn adapter_survey_bundle_json(
    captures: &[PicoBundleCapture],
) -> Result<String> {
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

pub(in crate::cmd_bundle) fn best_adapter_survey_bundle_candidate(
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
        let replace = best.as_ref().is_none_or(|existing| {
            (candidate.accepted && !existing.accepted)
                || (candidate.accepted == existing.accepted
                    && candidate.score_rank > existing.score_rank)
        });
        if replace {
            best = Some(candidate);
        }
    }
    best
}
