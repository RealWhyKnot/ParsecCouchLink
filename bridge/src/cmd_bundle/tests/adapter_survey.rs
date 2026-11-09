use super::pico_capture;
use crate::cmd_bundle::{
    AdapterSurveyAttempt, AdapterSurveyRawCapture, adapter_connection_json,
    adapter_connection_report, adapter_connection_text, adapter_survey_bundle_json,
    adapter_survey_candidates, adapter_survey_report_json, adapter_survey_text,
    aggregate_adapter_survey_text, aggregate_initial_usb_capture_text, build_adapter_survey_report,
    count_usb_packet_lines, usb_packets_text_from_diag,
};

#[test]
fn adapter_survey_candidates_cycle_after_debug_no_host_traffic() {
    assert_eq!(
        adapter_survey_candidates(crate::protocol::Persona::Debug, false),
        vec![
            crate::protocol::Persona::Ps3,
            crate::protocol::Persona::GenericHid,
            crate::protocol::Persona::Ps4,
            crate::protocol::Persona::Keyboard,
            crate::protocol::Persona::Xinput,
            crate::protocol::Persona::XboxOne,
            crate::protocol::Persona::Maple,
        ]
    );
}

#[test]
fn adapter_survey_candidates_stop_after_accepted_current_persona() {
    assert_eq!(
        adapter_survey_candidates(crate::protocol::Persona::Ps4, true),
        <[crate::protocol::Persona; 0]>::default()
    );
}

#[test]
fn adapter_survey_candidates_try_remaining_personas_after_rejected_current_persona() {
    assert_eq!(
        adapter_survey_candidates(crate::protocol::Persona::Ps4, false),
        vec![
            crate::protocol::Persona::Ps3,
            crate::protocol::Persona::GenericHid,
            crate::protocol::Persona::Keyboard,
            crate::protocol::Persona::Xinput,
            crate::protocol::Persona::XboxOne,
            crate::protocol::Persona::Maple,
        ]
    );
}

#[test]
fn adapter_survey_text_selects_accepted_ps4_candidate() {
    let attempts = vec![
        current_survey_attempt("debug", false, "debug_xinput_evidence_only", 4, 1),
        survey_attempt("ps4", true, "accepted_by_adapter", 5, 2),
        survey_attempt("keyboard", false, "adapter_did_not_enumerate", 0, 0),
    ];
    let report = test_survey_report("02E22DA9", "debug", "confirmed", Some("debug"), attempts);

    let text = adapter_survey_text(&report);
    assert!(text.contains("selected_best=ps4 accepted=true"));
    assert!(
        text.contains(
            "expected_adapter_personas=ps3,generic-hid,ps4,keyboard,xinput,xboxone,maple"
        )
    );
    assert!(text.contains("attempted_personas=debug,ps4,keyboard"));
    assert!(text.contains("missing_adapter_personas=ps3,generic-hid,xinput,xboxone,maple"));
    assert!(text.contains("current_no_usb_host_traffic=false"));
    assert!(text.contains("coverage_status=stopped_after_acceptance"));
    assert!(text.contains("stop_reason=accepted_candidate"));
    assert!(text.contains("persona=keyboard"));
    assert!(text.contains("verdict=adapter_did_not_enumerate"));
    assert!(text.contains("debug_xinput_evidence_only"));

    let json = adapter_survey_report_json(&report);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["best_candidate"]["persona"], "ps4");
    assert_eq!(value["best_candidate"]["accepted"], true);
    assert_eq!(value["coverage_status"], "stopped_after_acceptance");
    assert_eq!(value["stop_reason"], "accepted_candidate");
    assert_eq!(value["missing_adapter_personas"][0], "ps3");
    assert_eq!(value["attempts"][2]["device_desc_count"], 0);
}

#[test]
fn aggregate_adapter_survey_json_reports_bundle_best() {
    let attempts = vec![
        survey_attempt("ps4", false, "descriptor_or_report_rejected", 1, 1),
        survey_attempt("keyboard", true, "accepted_by_adapter", 4, 1),
    ];
    let report = test_survey_report(
        "02E22DA9",
        "xinput",
        "already_current",
        Some("xinput"),
        attempts,
    );
    let mut capture = pico_capture("02E22DA9", true, "{\"persona\":\"xinput\"}\n", "");
    capture.adapter_survey_text = adapter_survey_text(&report);
    capture.adapter_survey_json = adapter_survey_report_json(&report);
    capture.adapter_survey_report = Some(report);

    let text = aggregate_adapter_survey_text(std::slice::from_ref(&capture));
    assert!(text.contains("path=picos/02E22DA9/adapter-survey.txt"));
    assert!(text.contains("selected_best=keyboard accepted=true"));

    let json = adapter_survey_bundle_json(&[capture]).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["survey_count"], 1);
    assert_eq!(value["best_candidate"]["persona"], "keyboard");
    assert_eq!(value["best_candidate"]["accepted"], true);
    assert_eq!(value["per_pico"][0]["original_persona"], "xinput");
}

#[test]
fn adapter_connection_warns_when_live_survey_has_no_host_traffic() {
    let report = test_survey_report(
        "02E22DA9",
        "debug",
        "already_current",
        Some("debug"),
        all_no_host_survey_attempts(),
    );
    let mut capture = pico_capture("02E22DA9", true, "{\"persona\":\"debug\"}\n", "");
    capture.adapter_survey_report = Some(report);

    let connection = adapter_connection_report(&[capture]);
    assert_eq!(connection.status, "no_usb_host_traffic");
    assert!(connection.warning);
    assert_eq!(connection.surveyed_live_pico_count, 1);
    assert_eq!(connection.no_usb_host_pico_count, 1);
    assert_eq!(connection.host_traffic_pico_count, 0);
    assert_eq!(connection.per_pico[0].attempts, 8);
    assert_eq!(
        connection.per_pico[0].coverage_status,
        "all_adapter_personas_attempted"
    );
    assert_eq!(connection.per_pico[0].stop_reason, "exhausted_candidates");
    assert_eq!(
        connection.per_pico[0].missing_adapter_personas,
        <[String; 0]>::default()
    );
    assert!(connection.per_pico[0].warning);

    let text = adapter_connection_text(&connection);
    assert!(text.contains("warning_text=No USB host enumeration traffic was observed"));
    assert!(text.contains("coverage_status=all_adapter_personas_attempted"));
    assert!(
        text.contains("attempted_personas=debug,ps3,generic-hid,ps4,keyboard,xinput,xboxone,maple")
    );
    assert!(text.contains("missing_adapter_personas=none"));
    assert!(text.contains("If every attempted persona reports device_desc_count=0"));
    assert!(text.contains("power-cycle or physically replug"));

    let json = adapter_connection_json(&connection).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["status"], "no_usb_host_traffic");
    assert_eq!(value["warning"], true);
    assert_eq!(
        value["per_pico"][0]["coverage_status"],
        "all_adapter_personas_attempted"
    );
    assert_eq!(value["per_pico"][0]["attempted_personas"][7], "maple");
    assert_eq!(value["per_pico"][0]["device_desc_total"], 0);
}

#[test]
fn adapter_connection_does_not_warn_for_offline_or_cache_only_capture() {
    let capture = pico_capture("02E22DA9", false, "{\"persona\":\"xinput\"}\n", "");
    let connection = adapter_connection_report(&[capture]);
    assert_eq!(connection.status, "not_checked");
    assert!(!connection.warning);
    assert_eq!(connection.live_pico_count, 0);
    assert_eq!(connection.surveyed_live_pico_count, 0);
    assert!(connection.per_pico.is_empty());
}

#[test]
fn adapter_connection_reports_descriptor_rejection_as_actionable_evidence() {
    let attempts = vec![survey_attempt(
        "ps4",
        false,
        "descriptor_or_report_rejected",
        1,
        1,
    )];
    let report = test_survey_report("02E22DA9", "xinput", "confirmed", Some("xinput"), attempts);
    let mut capture = pico_capture(
        "02E22DA9",
        true,
        "{\"persona\":\"xinput\"}\n",
        "usb-packet seq=1 dir=setup data=8006000100001200\n",
    );
    capture.adapter_survey_report = Some(report);

    let connection = adapter_connection_report(&[capture]);
    assert_eq!(connection.status, "descriptor_or_report_rejected");
    assert!(!connection.warning);
    assert_eq!(connection.host_traffic_pico_count, 1);
    assert_eq!(connection.descriptor_or_report_rejected_pico_count, 1);
    assert_eq!(connection.per_pico[0].raw_packet_lines, 1);
}

#[test]
fn aggregate_initial_usb_capture_preserves_pre_survey_packet_lines() {
    let mut capture = pico_capture("02E22DA9", true, "{\"persona\":\"ps4\"}\n", "");
    capture.initial_usb_capture_text = usb_packets_text_from_diag(
        "02E22DA9",
        "usb-packet seq=1 dir=setup bm=0x80 req=0x06 data=8006000100001200\n",
    );

    let text = aggregate_initial_usb_capture_text(&[capture]);
    assert!(text.contains("path=picos/02E22DA9/initial-usb-capture.txt"));
    assert!(text.contains("usb-packet seq=1 dir=setup"));
    assert_eq!(count_usb_packet_lines(&text), 1);
}

pub(super) fn survey_attempt(
    persona: &str,
    accepted: bool,
    verdict: &str,
    score_rank: u8,
    device_desc_count: u32,
) -> AdapterSurveyAttempt {
    AdapterSurveyAttempt {
        persona: persona.to_string(),
        current_at_start: false,
        switched: true,
        usb_diag_captured: crate::cmd_bundle::flags::Capture::Captured,
        score_rank,
        score: "test score".to_string(),
        accepted,
        verdict: verdict.to_string(),
        device_desc_count,
        config_desc_count: u32::from(accepted),
        mount_count: u32::from(accepted),
        umount_count: 0,
        suspend_count: 0,
        resume_count: 0,
        input_report_sent_count: 0,
        host_out_count: 0,
        raw_capture: AdapterSurveyRawCapture::not_attempted("not_needed"),
    }
}

pub(super) fn current_survey_attempt(
    persona: &str,
    accepted: bool,
    verdict: &str,
    score_rank: u8,
    device_desc_count: u32,
) -> AdapterSurveyAttempt {
    let mut attempt = survey_attempt(persona, accepted, verdict, score_rank, device_desc_count);
    attempt.current_at_start = true;
    attempt.switched = false;
    attempt
}

pub(super) fn all_no_host_survey_attempts() -> Vec<AdapterSurveyAttempt> {
    vec![
        current_survey_attempt("debug", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("ps3", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("generic-hid", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("ps4", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("keyboard", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("xinput", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("xboxone", false, "adapter_did_not_enumerate", 0, 0),
        survey_attempt("maple", false, "adapter_did_not_enumerate", 0, 0),
    ]
}

pub(super) fn test_survey_report(
    uid: &str,
    original_persona: &str,
    restore_status: &str,
    restored_persona: Option<&str>,
    attempts: Vec<AdapterSurveyAttempt>,
) -> crate::cmd_bundle::AdapterSurveyReport {
    build_adapter_survey_report(
        uid.to_string(),
        original_persona.to_string(),
        restore_status.to_string(),
        restored_persona.map(std::string::ToString::to_string),
        attempts,
        vec![],
    )
}
