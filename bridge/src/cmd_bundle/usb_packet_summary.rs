use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

const HARVEST_PREFIX: &str = "# harvest ";

#[derive(Clone, Debug)]
pub(super) struct UsbPacketSummarySource<'a> {
    pub label: String,
    pub path: String,
    pub text: &'a str,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub(super) struct UsbPacketSummary {
    pub packet_lines: u64,
    pub stats_lines: u64,
    pub event_lines: u64,
    pub events: BTreeMap<String, u64>,
    pub directions: BTreeMap<String, u64>,
    pub sources: BTreeMap<String, u64>,
    pub reasons: BTreeMap<String, u64>,
    pub setup_directions: BTreeMap<String, u64>,
    pub setup_types: BTreeMap<String, u64>,
    pub setup_recipients: BTreeMap<String, u64>,
    pub setup_requests: BTreeMap<String, u64>,
    pub setup_descriptor_requests: BTreeMap<String, u64>,
    pub setup_known_requests: BTreeMap<String, u64>,
    pub control_payload_kinds: BTreeMap<String, u64>,
    pub control_descriptor_replies: BTreeMap<String, u64>,
    pub control_payload_summaries: BTreeMap<String, u64>,
    pub first_seq: Option<u64>,
    pub last_seq: Option<u64>,
    pub min_seq: Option<u64>,
    pub max_seq: Option<u64>,
    pub missing_sequence_numbers: u64,
    pub duplicate_sequence_numbers: u64,
    pub out_of_order_sequence_lines: u64,
    pub max_reported_packet_len: Option<u64>,
    pub max_captured_len: Option<u64>,
    pub truncated_packet_lines: u64,
    pub max_packet_truncated_bytes: Option<u64>,
    pub max_truncated_bytes: Option<u64>,
    pub max_suppressed_idle_reports: Option<u64>,
    pub last_stats_total_packets: Option<u64>,
    pub max_stats_total_packets: Option<u64>,
    pub max_stats_truncated_bytes: Option<u64>,
    pub max_stats_truncated_packets: Option<u64>,
    pub max_stats_idle_in_suppressed: Option<u64>,
    pub stats_direction_max: BTreeMap<String, u64>,
    pub first_packet_t_ms: Option<u64>,
    pub last_packet_t_ms: Option<u64>,
    pub min_packet_t_ms: Option<u64>,
    pub max_packet_t_ms: Option<u64>,
    pub packet_time_span_ms: Option<u64>,
    pub max_inter_packet_gap_ms: Option<u64>,
    pub packet_time_regressions: u64,
    pub harvest_lines: u64,
    pub harvest_statuses: BTreeMap<String, u64>,
    pub max_harvest_duration_ms: Option<u64>,
    pub max_harvest_lost_bytes: Option<u64>,
    pub max_harvest_chunk_count: Option<u64>,
    pub max_harvest_expected_chunks: Option<u64>,
    pub max_harvest_missing_chunks: Option<u64>,
    pub max_harvest_duplicate_chunks: Option<u64>,
    pub max_harvest_diag_bytes: Option<u64>,
    pub max_harvest_diag_lines: Option<u64>,
    pub max_harvest_packet_lines: Option<u64>,
    pub max_harvest_raw_packet_lines: Option<u64>,
    pub max_harvest_stats_lines: Option<u64>,
    pub max_harvest_event_lines: Option<u64>,
    pub max_harvest_new_lines: Option<u64>,
    pub max_harvest_duplicate_lines: Option<u64>,
    pub harvest_chunk_statuses: BTreeMap<String, u64>,
    pub hid_report_lines: u64,
    pub hid_report_types: BTreeMap<String, u64>,
    pub hid_report_ids: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct UsbPacketNamedSummary {
    pub label: String,
    pub path: String,
    pub summary: UsbPacketSummary,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct UsbPacketBundleSummary {
    pub artifact_schema_version: u8,
    pub aggregate: UsbPacketSummary,
    pub per_pico: Vec<UsbPacketNamedSummary>,
    pub retained_logs: Vec<UsbPacketNamedSummary>,
    pub notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum UsbPacketRecord {
    Packet(Box<UsbPacketRecordPacket>),
    Stats {
        source_label: String,
        source_path: String,
        line_number: u64,
        t_ms: Option<u64>,
        total: Option<u64>,
        #[serde(rename = "in")]
        in_count: Option<u64>,
        out: Option<u64>,
        setup: Option<u64>,
        control_in: Option<u64>,
        truncated_bytes: Option<u64>,
        truncated_packets: Option<u64>,
        idle_in_suppressed: Option<u64>,
        raw_line: String,
    },
    Event {
        source_label: String,
        source_path: String,
        line_number: u64,
        t_ms: Option<u64>,
        event: Option<String>,
        source: Option<String>,
        len: Option<u64>,
        bytes: Option<u64>,
        remote_wakeup: Option<u64>,
        raw_line: String,
    },
    Harvest {
        source_label: String,
        source_path: String,
        line_number: u64,
        at: Option<String>,
        status: Option<String>,
        duration_ms: Option<u64>,
        chunk_count: Option<u64>,
        expected_chunks: Option<u64>,
        missing_chunk_count: Option<u64>,
        duplicate_chunk_count: Option<u64>,
        got_last: Option<bool>,
        chunk_complete: Option<bool>,
        lost_bytes: Option<u64>,
        diag_bytes: Option<u64>,
        diag_lines: Option<u64>,
        packet_lines: Option<u64>,
        raw_packet_lines: Option<u64>,
        stats_lines: Option<u64>,
        event_lines: Option<u64>,
        new_lines: Option<u64>,
        duplicate_lines: Option<u64>,
        total_packet_lines: Option<u64>,
        error: Option<String>,
        raw_line: String,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct UsbPacketRecordPacket {
    source_label: String,
    source_path: String,
    line_number: u64,
    seq: Option<u64>,
    t_ms: Option<u64>,
    direction: Option<String>,
    source: Option<String>,
    reason: Option<String>,
    reported_len: Option<u64>,
    captured_len: Option<u64>,
    packet_truncated_bytes: Option<u64>,
    truncated_bytes_total: Option<u64>,
    suppressed_idle_reports: Option<u64>,
    setup_bm_request_type: Option<u64>,
    setup_request: Option<u64>,
    setup_value: Option<u64>,
    setup_index: Option<u64>,
    setup_length: Option<u64>,
    setup_direction: Option<&'static str>,
    setup_type: Option<&'static str>,
    setup_recipient: Option<&'static str>,
    setup_request_name: Option<String>,
    setup_descriptor_type: Option<&'static str>,
    setup_descriptor_index: Option<u64>,
    setup_language_id: Option<u64>,
    setup_known_request: Option<&'static str>,
    hid_report_id: Option<u64>,
    hid_report_type: Option<u64>,
    hid_report_type_name: Option<&'static str>,
    control_payload_kind: Option<&'static str>,
    control_descriptor_type: Option<&'static str>,
    control_payload_summary: Option<String>,
    data_hex: Option<String>,
    raw_line: String,
}

pub(super) fn summarize_text(text: &str) -> UsbPacketSummary {
    let mut summary = UsbPacketSummary::default();
    let mut seen_sequences = BTreeSet::new();
    let mut previous_seq = None;
    let mut previous_t_ms = None;
    for line in text.lines() {
        if line.starts_with("usb-packet ") {
            summary.add_packet_line(
                line,
                &mut seen_sequences,
                &mut previous_seq,
                &mut previous_t_ms,
            );
        } else if line.starts_with("usb-packet-stats ") {
            summary.add_stats_line(line);
        } else if line.starts_with("usb-event ") {
            summary.add_event_line(line);
        } else if line.starts_with(HARVEST_PREFIX) {
            summary.add_harvest_line(line);
        }
    }
    summary
}

pub(super) fn records_jsonl_for_text(
    label: &str,
    path: &str,
    text: &str,
) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for record in records_for_text(label, path, text) {
        out.push_str(&serde_json::to_string(&record)?);
        out.push('\n');
    }
    Ok(out)
}

pub(super) fn records_jsonl_for_sources(
    per_pico: &[UsbPacketSummarySource<'_>],
    retained_logs: &[UsbPacketSummarySource<'_>],
) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for source in per_pico.iter().chain(retained_logs.iter()) {
        out.push_str(&records_jsonl_for_text(
            &source.label,
            &source.path,
            source.text,
        )?);
    }
    Ok(out)
}

pub(super) fn control_transfers_text_for_text(label: &str, path: &str, text: &str) -> String {
    let mut out = String::from("# USB control transfer transcript\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_label={label}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_path={path}\n\n"));
    let rows = control_transfer_rows(text);
    if rows.is_empty() {
        out.push_str("No USB control setup or control-IN packet lines were captured.\n");
    } else {
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
    }
    out
}

pub(super) fn control_transfers_text_for_sources(
    per_pico: &[UsbPacketSummarySource<'_>],
    retained_logs: &[UsbPacketSummarySource<'_>],
) -> String {
    let mut out = String::from(
        "# USB control transfer transcript\n\n\
         # Includes debug input usb-packet lines where dir=setup or dir=control-in.\n\
         # Use usb-packets.jsonl for machine parsing and raw packet context.\n\n",
    );
    let mut section_count = 0usize;
    for source in per_pico.iter().chain(retained_logs.iter()) {
        let rows = control_transfer_rows(source.text);
        if rows.is_empty() {
            continue;
        }
        section_count += 1;
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!("## {} ({})\n", source.label, source.path),
        );
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
        out.push('\n');
    }
    if section_count == 0 {
        out.push_str("No USB control setup or control-IN packet lines were captured.\n");
    }
    out
}

pub(super) fn hid_reports_text_for_text(label: &str, path: &str, text: &str) -> String {
    let mut out = String::from("# USB HID report transcript\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_label={label}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_path={path}\n\n"));
    let rows = hid_report_rows(text);
    if rows.is_empty() {
        out.push_str("No HID report metadata packet lines were captured.\n");
    } else {
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
    }
    out
}

pub(super) fn hid_reports_text_for_sources(
    per_pico: &[UsbPacketSummarySource<'_>],
    retained_logs: &[UsbPacketSummarySource<'_>],
) -> String {
    let mut out = String::from(
        "# USB HID report transcript\n\n\
         # Includes debug input usb-packet lines with HID report id/type metadata.\n\
         # HID GET_REPORT and SET_REPORT setup requests are decoded from wValue.\n\
         # Use usb-packets.jsonl for machine parsing and raw packet context.\n\n",
    );
    let mut section_count = 0usize;
    for source in per_pico.iter().chain(retained_logs.iter()) {
        let rows = hid_report_rows(source.text);
        if rows.is_empty() {
            continue;
        }
        section_count += 1;
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!("## {} ({})\n", source.label, source.path),
        );
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
        out.push('\n');
    }
    if section_count == 0 {
        out.push_str("No HID report metadata packet lines were captured.\n");
    }
    out
}

pub(super) fn packet_timeline_text_for_text(label: &str, path: &str, text: &str) -> String {
    let mut out = String::from("# USB packet timeline\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_label={label}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_path={path}\n\n"));
    let rows = packet_timeline_rows(text);
    if rows.is_empty() {
        out.push_str("No USB packet, lifecycle event, packet-stat, or harvest timeline rows were captured.\n");
    } else {
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
    }
    out
}

pub(super) fn packet_timeline_text_for_sources(
    per_pico: &[UsbPacketSummarySource<'_>],
    retained_logs: &[UsbPacketSummarySource<'_>],
) -> String {
    let mut out = String::from(
        "# USB packet timeline\n\n\
         # Includes debug input usb-packet, usb-event, usb-packet-stats, and harvest records.\n\
         # dt_ms is measured from the previous timestamped packet/event/stat row in the same source.\n\
         # Use usb-packets.jsonl for machine parsing and raw packet context.\n\n",
    );
    let mut section_count = 0usize;
    for source in per_pico.iter().chain(retained_logs.iter()) {
        let rows = packet_timeline_rows(source.text);
        if rows.is_empty() {
            continue;
        }
        section_count += 1;
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!("## {} ({})\n", source.label, source.path),
        );
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
        out.push('\n');
    }
    if section_count == 0 {
        out.push_str("No USB packet, lifecycle event, packet-stat, or harvest timeline rows were captured.\n");
    }
    out
}

pub(super) fn enumeration_analysis_text_for_text(label: &str, path: &str, text: &str) -> String {
    let mut out = String::from("# USB enumeration analysis\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_label={label}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_path={path}\n\n"));
    let analysis = analyze_enumeration(text);
    write_enumeration_analysis(&mut out, &analysis);
    out
}

pub(super) fn enumeration_analysis_text_for_sources(
    per_pico: &[UsbPacketSummarySource<'_>],
    retained_logs: &[UsbPacketSummarySource<'_>],
) -> String {
    let mut out = String::from(
        "# USB enumeration analysis\n\n\
         # Derived from debug input usb-packet setup, control-IN, endpoint-IN, endpoint-OUT, HID report, and usb-event lifecycle lines.\n\
         # This file is a quick checklist for whether a host adapter enumerated, configured, probed, and exchanged runtime traffic with the Pico.\n\n",
    );
    let mut section_count = 0usize;
    for source in per_pico.iter().chain(retained_logs.iter()) {
        let analysis = analyze_enumeration(source.text);
        if analysis.packet_lines == 0
            && analysis.event_lines == 0
            && analysis.harvest_lines == 0
            && analysis.stats_lines == 0
        {
            continue;
        }
        section_count += 1;
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!("## {} ({})\n", source.label, source.path),
        );
        write_enumeration_analysis(&mut out, &analysis);
        out.push('\n');
    }
    if section_count == 0 {
        out.push_str(
            "No USB packet, lifecycle event, packet-stat, or harvest evidence was captured.\n",
        );
    }
    out
}

pub(super) fn summarize_sources(
    per_pico: &[UsbPacketSummarySource<'_>],
    retained_logs: &[UsbPacketSummarySource<'_>],
) -> UsbPacketBundleSummary {
    let mut aggregate = UsbPacketSummary::default();
    let per_pico = per_pico
        .iter()
        .map(|source| {
            let summary = summarize_text(source.text);
            aggregate.merge_from(&summary);
            UsbPacketNamedSummary {
                label: source.label.clone(),
                path: source.path.clone(),
                summary,
            }
        })
        .collect();
    let retained_logs = retained_logs
        .iter()
        .map(|source| {
            let summary = summarize_text(source.text);
            aggregate.merge_from(&summary);
            UsbPacketNamedSummary {
                label: source.label.clone(),
                path: source.path.clone(),
                summary,
            }
        })
        .collect();

    UsbPacketBundleSummary {
        artifact_schema_version: 8,
        aggregate,
        per_pico,
        retained_logs,
        notes: vec![
            "Counts are derived from bundled usb-packet, usb-event, and usb-packet-stats lines.",
            "Aggregate sequence gaps are summed per source; sequence numbers are not compared across different Pico/log sources.",
            "Packet and lifecycle event timing fields are derived from firmware t= milliseconds and gap calculations are per source.",
            "Stats lines are checkpoint summaries emitted by debug input firmware and may survive even when raw packet lines have rotated out.",
            "Lifecycle event lines record low-noise USB mount, unmount, suspend, resume, first host OUT, and first accepted IN callbacks from debug input firmware.",
            "Packet summaries separate per-packet truncation from cumulative firmware truncated_bytes so lossy captures are visible.",
            "Harvest lines describe each retained host GET_LOG attempt used to collect debug input packets.",
            "Setup/control summary maps expose observed enumeration requests, descriptor requests, known vendor probes, and control payload replies.",
            "Packet records decode USB setup direction, type, recipient, standard/class requests, descriptor types, and known CouchLink vendor requests.",
            "Packet records expose HID report id/type metadata from HID OUT/FEATURE lines and HID GET_REPORT/SET_REPORT setup requests.",
            "Control-IN packet records identify descriptor replies, MS OS descriptor payloads, and setup-mode diag-log payloads when the captured bytes are sufficient.",
            "Harvest metadata records GET_LOG chunk completeness, missing/duplicate chunks, returned diag bytes, and duplicate packet lines.",
        ],
    }
}

fn records_for_text(label: &str, path: &str, text: &str) -> Vec<UsbPacketRecord> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| record_from_line(label, path, (index + 1) as u64, line))
        .collect()
}

fn record_from_line(
    label: &str,
    path: &str,
    line_number: u64,
    line: &str,
) -> Option<UsbPacketRecord> {
    if line.starts_with("usb-packet ") {
        let fields = fields(line);
        let decoded_setup = decode_setup_fields(&fields);
        let decoded_control_payload = decode_control_payload_fields(&fields);
        let hid_report = decode_hid_report_metadata(&fields);
        return Some(UsbPacketRecord::Packet(Box::new(UsbPacketRecordPacket {
            source_label: label.to_string(),
            source_path: path.to_string(),
            line_number,
            seq: parsed_u64(fields.get("seq")),
            t_ms: parsed_u64(fields.get("t")),
            direction: cloned_field(fields.get("dir")),
            source: cloned_field(fields.get("src")),
            reason: cloned_field(fields.get("reason")),
            reported_len: parsed_u64(fields.get("len")),
            captured_len: parsed_u64(fields.get("captured")),
            packet_truncated_bytes: parsed_u64(fields.get("truncated")),
            truncated_bytes_total: parsed_u64(fields.get("dropped")),
            suppressed_idle_reports: parsed_u64(fields.get("suppressed")),
            setup_bm_request_type: parsed_u64(fields.get("bm")),
            setup_request: parsed_u64(fields.get("req")),
            setup_value: parsed_u64(fields.get("value")),
            setup_index: parsed_u64(fields.get("index")),
            setup_length: parsed_u64(fields.get("wlen")),
            setup_direction: decoded_setup.as_ref().map(|setup| setup.direction),
            setup_type: decoded_setup.as_ref().map(|setup| setup.request_type),
            setup_recipient: decoded_setup.as_ref().map(|setup| setup.recipient),
            setup_request_name: decoded_setup
                .as_ref()
                .map(|setup| setup.request_name.clone()),
            setup_descriptor_type: decoded_setup
                .as_ref()
                .and_then(|setup| setup.descriptor_type),
            setup_descriptor_index: decoded_setup
                .as_ref()
                .and_then(|setup| setup.descriptor_index),
            setup_language_id: decoded_setup.as_ref().and_then(|setup| setup.language_id),
            setup_known_request: decoded_setup.as_ref().and_then(|setup| setup.known_request),
            hid_report_id: hid_report.as_ref().and_then(|report| report.report_id),
            hid_report_type: hid_report.as_ref().and_then(|report| report.report_type),
            hid_report_type_name: hid_report
                .as_ref()
                .and_then(|report| report.report_type_name),
            control_payload_kind: decoded_control_payload.as_ref().map(|payload| payload.kind),
            control_descriptor_type: decoded_control_payload
                .as_ref()
                .and_then(|payload| payload.descriptor_type),
            control_payload_summary: decoded_control_payload
                .as_ref()
                .map(|payload| payload.summary.clone()),
            data_hex: cloned_field(fields.get("data")),
            raw_line: line.to_string(),
        })));
    }
    if line.starts_with("usb-packet-stats ") {
        let fields = fields(line);
        return Some(UsbPacketRecord::Stats {
            source_label: label.to_string(),
            source_path: path.to_string(),
            line_number,
            t_ms: parsed_u64(fields.get("t")),
            total: parsed_u64(fields.get("total")),
            in_count: parsed_u64(fields.get("in")),
            out: parsed_u64(fields.get("out")),
            setup: parsed_u64(fields.get("setup")),
            control_in: parsed_u64(fields.get("control_in")),
            truncated_bytes: parsed_u64(fields.get("truncated_bytes")),
            truncated_packets: parsed_u64(fields.get("truncated_packets")),
            idle_in_suppressed: parsed_u64(fields.get("idle_in_suppressed")),
            raw_line: line.to_string(),
        });
    }
    if line.starts_with("usb-event ") {
        let fields = fields(line);
        return Some(UsbPacketRecord::Event {
            source_label: label.to_string(),
            source_path: path.to_string(),
            line_number,
            t_ms: parsed_u64(fields.get("t")),
            event: cloned_field(fields.get("event")),
            source: cloned_field(fields.get("src")),
            len: parsed_u64(fields.get("len")),
            bytes: parsed_u64(fields.get("bytes")),
            remote_wakeup: parsed_u64(fields.get("remote_wakeup")),
            raw_line: line.to_string(),
        });
    }
    if line.starts_with(HARVEST_PREFIX) {
        let value = harvest_value(line);
        return Some(UsbPacketRecord::Harvest {
            source_label: label.to_string(),
            source_path: path.to_string(),
            line_number,
            at: value.as_ref().and_then(|value| json_string(value, "at")),
            status: value
                .as_ref()
                .and_then(|value| json_string(value, "status"))
                .or_else(|| Some("malformed".to_string())),
            duration_ms: value
                .as_ref()
                .and_then(|value| json_u64(value, "duration_ms")),
            chunk_count: value
                .as_ref()
                .and_then(|value| json_u64(value, "chunk_count")),
            expected_chunks: value
                .as_ref()
                .and_then(|value| json_u64(value, "expected_chunks")),
            missing_chunk_count: value
                .as_ref()
                .and_then(|value| json_u64(value, "missing_chunk_count")),
            duplicate_chunk_count: value
                .as_ref()
                .and_then(|value| json_u64(value, "duplicate_chunk_count")),
            got_last: value
                .as_ref()
                .and_then(|value| json_bool(value, "got_last")),
            chunk_complete: value
                .as_ref()
                .and_then(|value| json_bool(value, "chunk_complete")),
            lost_bytes: value
                .as_ref()
                .and_then(|value| json_u64(value, "lost_bytes")),
            diag_bytes: value
                .as_ref()
                .and_then(|value| json_u64(value, "diag_bytes")),
            diag_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "diag_lines")),
            packet_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "packet_lines")),
            raw_packet_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "raw_packet_lines")),
            stats_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "stats_lines")),
            event_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "event_lines")),
            new_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "new_lines")),
            duplicate_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "duplicate_lines")),
            total_packet_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "total_packet_lines")),
            error: value.as_ref().and_then(|value| json_string(value, "error")),
            raw_line: line.to_string(),
        });
    }
    None
}

fn packet_timeline_rows(text: &str) -> Vec<String> {
    let mut previous_t_ms = None;
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            packet_timeline_row((index + 1) as u64, line, &mut previous_t_ms)
        })
        .collect()
}

fn packet_timeline_row(
    line_number: u64,
    line: &str,
    previous_t_ms: &mut Option<u64>,
) -> Option<String> {
    if line.starts_with("usb-packet ") {
        let fields = fields(line);
        let t_ms = parsed_u64(fields.get("t"));
        let dt_ms = timeline_delta(t_ms, previous_t_ms);
        return Some(format!(
            "packet line={} seq={} t={} dt_ms={} dir={} src={} reason={} len={} captured={} truncated={} dropped={} suppressed={} data={}",
            line_number,
            display_field(fields.get("seq")),
            display_field(fields.get("t")),
            dt_ms,
            display_field(fields.get("dir")),
            display_field(fields.get("src")),
            display_field(fields.get("reason")),
            display_field(fields.get("len")),
            display_field(fields.get("captured")),
            display_field(fields.get("truncated")),
            display_field(fields.get("dropped")),
            display_field(fields.get("suppressed")),
            display_field(fields.get("data")),
        ));
    }
    if line.starts_with("usb-packet-stats ") {
        let fields = fields(line);
        let t_ms = parsed_u64(fields.get("t"));
        let dt_ms = timeline_delta(t_ms, previous_t_ms);
        return Some(format!(
            "stats line={} t={} dt_ms={} total={} in={} out={} setup={} control_in={} truncated_bytes={} truncated_packets={} idle_in_suppressed={}",
            line_number,
            display_field(fields.get("t")),
            dt_ms,
            display_field(fields.get("total")),
            display_field(fields.get("in")),
            display_field(fields.get("out")),
            display_field(fields.get("setup")),
            display_field(fields.get("control_in")),
            display_field(fields.get("truncated_bytes")),
            display_field(fields.get("truncated_packets")),
            display_field(fields.get("idle_in_suppressed")),
        ));
    }
    if line.starts_with("usb-event ") {
        let fields = fields(line);
        let t_ms = parsed_u64(fields.get("t"));
        let dt_ms = timeline_delta(t_ms, previous_t_ms);
        return Some(format!(
            "event line={} t={} dt_ms={} event={} src={} len={} bytes={} remote_wakeup={}",
            line_number,
            display_field(fields.get("t")),
            dt_ms,
            display_field(fields.get("event")),
            display_field(fields.get("src")),
            display_field(fields.get("len")),
            display_field(fields.get("bytes")),
            display_field(fields.get("remote_wakeup")),
        ));
    }
    if line.starts_with(HARVEST_PREFIX) {
        let value = harvest_value(line);
        return Some(format!(
            "harvest line={} at={} status={} duration_ms={} chunk_complete={} packet_lines={} raw_packet_lines={} new_lines={} error={}",
            line_number,
            value
                .as_ref()
                .and_then(|value| json_string(value, "at"))
                .unwrap_or_else(|| "-".to_string()),
            value
                .as_ref()
                .and_then(|value| json_string(value, "status"))
                .unwrap_or_else(|| "malformed".to_string()),
            value
                .as_ref()
                .and_then(|value| json_u64(value, "duration_ms"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            value
                .as_ref()
                .and_then(|value| json_bool(value, "chunk_complete"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            value
                .as_ref()
                .and_then(|value| json_u64(value, "packet_lines"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            value
                .as_ref()
                .and_then(|value| json_u64(value, "raw_packet_lines"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            value
                .as_ref()
                .and_then(|value| json_u64(value, "new_lines"))
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            value
                .as_ref()
                .and_then(|value| json_string(value, "error"))
                .map(|value| sanitize_timeline_field(&value))
                .unwrap_or_else(|| "-".to_string()),
        ));
    }
    None
}

fn timeline_delta(t_ms: Option<u64>, previous_t_ms: &mut Option<u64>) -> String {
    let Some(t_ms) = t_ms else {
        return "-".to_string();
    };
    let delta = previous_t_ms
        .map(|previous| {
            if t_ms >= previous {
                (t_ms - previous).to_string()
            } else {
                "regression".to_string()
            }
        })
        .unwrap_or_else(|| "-".to_string());
    *previous_t_ms = Some(t_ms);
    delta
}

fn sanitize_timeline_field(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hid_report_rows(text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| hid_report_row((index + 1) as u64, line))
        .collect()
}

fn hid_report_row(line_number: u64, line: &str) -> Option<String> {
    if !line.starts_with("usb-packet ") {
        return None;
    }
    let fields = fields(line);
    let report = decode_hid_report_metadata(&fields)?;
    Some(format!(
        "hid-report line={} seq={} t={} dir={} src={} request={} report_id={} report_type={} report_type_name={} len={} captured={} wlen={} data={}",
        line_number,
        display_field(fields.get("seq")),
        display_field(fields.get("t")),
        display_field(fields.get("dir")),
        display_field(fields.get("src")),
        report.request_name.unwrap_or("-"),
        report
            .report_id
            .map(hex_u8)
            .unwrap_or_else(|| "-".to_string()),
        report
            .report_type
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string()),
        report.report_type_name.unwrap_or("-"),
        display_field(fields.get("len")),
        display_field(fields.get("captured")),
        display_field(fields.get("wlen")),
        display_field(fields.get("data")),
    ))
}

fn control_transfer_rows(text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| control_transfer_row((index + 1) as u64, line))
        .collect()
}

fn control_transfer_row(line_number: u64, line: &str) -> Option<String> {
    if !line.starts_with("usb-packet ") {
        return None;
    }
    let fields = fields(line);
    let direction = fields.get("dir").copied()?;
    match direction {
        "setup" => Some(format!(
            "setup line={} seq={} t={} src={} bm={} req={} value={} index={} wlen={} len={} captured={} {} data={}",
            line_number,
            display_field(fields.get("seq")),
            display_field(fields.get("t")),
            display_field(fields.get("src")),
            display_field(fields.get("bm")),
            display_field(fields.get("req")),
            display_field(fields.get("value")),
            display_field(fields.get("index")),
            display_field(fields.get("wlen")),
            display_field(fields.get("len")),
            display_field(fields.get("captured")),
            setup_decode_text(&fields),
            display_field(fields.get("data")),
        )),
        "control-in" => Some(format!(
            "control-in line={} seq={} t={} src={} reason={} len={} captured={} dropped={} {} data={}",
            line_number,
            display_field(fields.get("seq")),
            display_field(fields.get("t")),
            display_field(fields.get("src")),
            display_field(fields.get("reason")),
            display_field(fields.get("len")),
            display_field(fields.get("captured")),
            display_field(fields.get("dropped")),
            control_payload_decode_text(&fields),
            display_field(fields.get("data")),
        )),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SetupDecode {
    direction: &'static str,
    request_type: &'static str,
    recipient: &'static str,
    request_name: String,
    descriptor_type: Option<&'static str>,
    descriptor_index: Option<u64>,
    language_id: Option<u64>,
    known_request: Option<&'static str>,
}

fn decode_setup_fields(fields: &BTreeMap<&str, &str>) -> Option<SetupDecode> {
    let bm = u8::try_from(parsed_u64(fields.get("bm"))? & 0xFF).ok()?;
    let request = u8::try_from(parsed_u64(fields.get("req"))? & 0xFF).ok()?;
    let value = parsed_u64(fields.get("value")).unwrap_or(0) & 0xFFFF;
    let index = parsed_u64(fields.get("index")).unwrap_or(0) & 0xFFFF;
    let request_type = setup_request_type(bm);
    let known_request = known_vendor_setup_request(bm, request, index);
    let request_name = known_request
        .map(str::to_string)
        .unwrap_or_else(|| setup_request_name(request_type, request));
    let decodes_descriptor = request_type == "standard" && matches!(request, 0x06 | 0x07);
    let descriptor_type =
        decodes_descriptor.then(|| descriptor_type_name(((value >> 8) & 0xFF) as u8));
    let descriptor_index = decodes_descriptor.then_some(value & 0xFF);
    let language_id = (descriptor_type == Some("string")).then_some(index);

    Some(SetupDecode {
        direction: setup_direction(bm),
        request_type,
        recipient: setup_recipient(bm),
        request_name,
        descriptor_type,
        descriptor_index,
        language_id,
        known_request,
    })
}

fn setup_decode_text(fields: &BTreeMap<&str, &str>) -> String {
    let Some(decoded) = decode_setup_fields(fields) else {
        return "decode=-".to_string();
    };
    format!(
        "decode={}/{}/{} request={} descriptor={} descriptor_index={} language_id={} known={}",
        decoded.direction,
        decoded.request_type,
        decoded.recipient,
        decoded.request_name,
        decoded.descriptor_type.unwrap_or("-"),
        decoded
            .descriptor_index
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string()),
        decoded
            .language_id
            .map(hex_u16)
            .unwrap_or_else(|| "-".to_string()),
        decoded.known_request.unwrap_or("-"),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HidReportMetadata {
    report_id: Option<u64>,
    report_type: Option<u64>,
    report_type_name: Option<&'static str>,
    request_name: Option<&'static str>,
}

fn decode_hid_report_metadata(fields: &BTreeMap<&str, &str>) -> Option<HidReportMetadata> {
    let explicit_report_id = parsed_u64(fields.get("report_id"));
    let explicit_report_type = parsed_u64(fields.get("report_type"));
    if explicit_report_id.is_some() || explicit_report_type.is_some() {
        return Some(HidReportMetadata {
            report_id: explicit_report_id,
            report_type: explicit_report_type,
            report_type_name: explicit_report_type.and_then(hid_report_type_name),
            request_name: None,
        });
    }

    let bm = u8::try_from(parsed_u64(fields.get("bm"))? & 0xFF).ok()?;
    let request = u8::try_from(parsed_u64(fields.get("req"))? & 0xFF).ok()?;
    let value = parsed_u64(fields.get("value")).unwrap_or(0) & 0xFFFF;
    if setup_request_type(bm) != "class" || !matches!(request, 0x01 | 0x09) {
        return None;
    }

    let report_id = value & 0xFF;
    let report_type = (value >> 8) & 0xFF;
    Some(HidReportMetadata {
        report_id: Some(report_id),
        report_type: Some(report_type),
        report_type_name: hid_report_type_name(report_type),
        request_name: Some(match request {
            0x01 => "hid_get_report",
            0x09 => "hid_set_report",
            _ => unreachable!(),
        }),
    })
}

fn hid_report_type_name(report_type: u64) -> Option<&'static str> {
    match report_type {
        1 => Some("input"),
        2 => Some("output"),
        3 => Some("feature"),
        _ => None,
    }
}

fn setup_direction(bm_request_type: u8) -> &'static str {
    if (bm_request_type & 0x80) != 0 {
        "device_to_host"
    } else {
        "host_to_device"
    }
}

fn setup_request_type(bm_request_type: u8) -> &'static str {
    match (bm_request_type >> 5) & 0x03 {
        0 => "standard",
        1 => "class",
        2 => "vendor",
        _ => "reserved",
    }
}

fn setup_recipient(bm_request_type: u8) -> &'static str {
    match bm_request_type & 0x1F {
        0 => "device",
        1 => "interface",
        2 => "endpoint",
        3 => "other",
        _ => "reserved",
    }
}

fn setup_request_name(request_type: &str, request: u8) -> String {
    let name = match request_type {
        "standard" => match request {
            0x00 => Some("get_status"),
            0x01 => Some("clear_feature"),
            0x03 => Some("set_feature"),
            0x05 => Some("set_address"),
            0x06 => Some("get_descriptor"),
            0x07 => Some("set_descriptor"),
            0x08 => Some("get_configuration"),
            0x09 => Some("set_configuration"),
            0x0A => Some("get_interface"),
            0x0B => Some("set_interface"),
            0x0C => Some("synch_frame"),
            _ => None,
        },
        "class" => match request {
            0x01 => Some("hid_get_report"),
            0x02 => Some("hid_get_idle"),
            0x03 => Some("hid_get_protocol"),
            0x09 => Some("hid_set_report"),
            0x0A => Some("hid_set_idle"),
            0x0B => Some("hid_set_protocol"),
            _ => None,
        },
        "vendor" => None,
        _ => None,
    };
    name.map(str::to_string)
        .unwrap_or_else(|| format!("{request_type}_{request:#04X}"))
}

fn descriptor_type_name(descriptor_type: u8) -> &'static str {
    match descriptor_type {
        0x01 => "device",
        0x02 => "configuration",
        0x03 => "string",
        0x04 => "interface",
        0x05 => "endpoint",
        0x06 => "device_qualifier",
        0x07 => "other_speed_configuration",
        0x08 => "interface_power",
        0x09 => "otg",
        0x0A => "debug",
        0x0B => "interface_association",
        0x0F => "bos",
        0x10 => "device_capability",
        0x21 => "hid",
        0x22 => "hid_report",
        0x23 => "hid_physical",
        0x29 => "hub",
        0x30 => "super_speed_endpoint_companion",
        _ => "unknown",
    }
}

fn known_vendor_setup_request(
    bm_request_type: u8,
    request: u8,
    index: u64,
) -> Option<&'static str> {
    match (bm_request_type, request, index) {
        (0xC0, 0x20, 0x0004) => Some("xgip-ms-os-10-compatible-id"),
        (0xC0, 0x20, 0x0007) => Some("ms-os-20-descriptor-set"),
        (0xC1, 0x01, index) if (index & 0x00FF) == 0x0002 => Some("couchlink-setup-diag-log"),
        _ => None,
    }
}

fn hex_u16(value: u64) -> String {
    format!("0x{:04X}", value & 0xFFFF)
}

fn hex_u8(value: u64) -> String {
    format!("0x{:02X}", value & 0xFF)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ControlPayloadDecode {
    kind: &'static str,
    descriptor_type: Option<&'static str>,
    summary: String,
}

fn decode_control_payload_fields(fields: &BTreeMap<&str, &str>) -> Option<ControlPayloadDecode> {
    if fields.get("dir").copied()? != "control-in" {
        return None;
    }
    let source = fields.get("src").copied().unwrap_or("");
    match source {
        "xgip-compat-id" => {
            return Some(ControlPayloadDecode {
                kind: "known_vendor_payload",
                descriptor_type: None,
                summary: "xgip-ms-os-10-compatible-id".to_string(),
            });
        }
        "ms-os-20" => {
            return Some(ControlPayloadDecode {
                kind: "known_vendor_payload",
                descriptor_type: None,
                summary: "ms-os-20-descriptor-set".to_string(),
            });
        }
        "setup-diag-log" => {
            return Some(ControlPayloadDecode {
                kind: "known_vendor_payload",
                descriptor_type: None,
                summary: "couchlink-setup-diag-log".to_string(),
            });
        }
        _ => {}
    }

    let bytes = hex_bytes(fields.get("data"))?;
    if bytes.len() < 2 {
        return None;
    }
    let descriptor_type = descriptor_type_name(bytes[1]);
    let summary = match bytes[1] {
        0x01 => device_descriptor_summary(&bytes),
        0x02 => configuration_descriptor_summary(&bytes),
        0x03 => string_descriptor_summary(&bytes),
        0x0F => descriptor_len_summary("bos", &bytes),
        0x21 => descriptor_len_summary("hid", &bytes),
        0x22 => descriptor_len_summary("hid_report", &bytes),
        other => format!(
            "descriptor={},captured_len={}",
            descriptor_type_name(other),
            bytes.len()
        ),
    };
    Some(ControlPayloadDecode {
        kind: "usb_descriptor",
        descriptor_type: Some(descriptor_type),
        summary,
    })
}

fn control_payload_decode_text(fields: &BTreeMap<&str, &str>) -> String {
    let Some(decoded) = decode_control_payload_fields(fields) else {
        return "payload_kind=- payload_descriptor=- payload_summary=-".to_string();
    };
    format!(
        "payload_kind={} payload_descriptor={} payload_summary={}",
        decoded.kind,
        decoded.descriptor_type.unwrap_or("-"),
        decoded.summary,
    )
}

fn hex_bytes(value: Option<&&str>) -> Option<Vec<u8>> {
    let text = *value?;
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for index in (0..text.len()).step_by(2) {
        let byte = u8::from_str_radix(&text[index..index + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

fn device_descriptor_summary(bytes: &[u8]) -> String {
    if bytes.len() < 18 {
        return format!("descriptor=device,captured_len={}", bytes.len());
    }
    format!(
        "descriptor=device,bcd_usb={},class=0x{:02X},subclass=0x{:02X},protocol=0x{:02X},max_packet={},vid={},pid={},bcd_device={},configs={}",
        hex_u16(le_u16(bytes, 2)),
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        hex_u16(le_u16(bytes, 8)),
        hex_u16(le_u16(bytes, 10)),
        hex_u16(le_u16(bytes, 12)),
        bytes[17],
    )
}

fn configuration_descriptor_summary(bytes: &[u8]) -> String {
    if bytes.len() < 9 {
        return format!("descriptor=configuration,captured_len={}", bytes.len());
    }
    let max_power_ma = u16::from(bytes[8]) * 2;
    format!(
        "descriptor=configuration,total_len={},interfaces={},configuration={},attributes=0x{:02X},max_power_ma={}",
        le_u16(bytes, 2),
        bytes[4],
        bytes[5],
        bytes[7],
        max_power_ma,
    )
}

fn string_descriptor_summary(bytes: &[u8]) -> String {
    if bytes.len() == 4 {
        return format!(
            "descriptor=string,language_id={}",
            hex_u16(le_u16(bytes, 2))
        );
    }
    format!(
        "descriptor=string,utf16_bytes={},captured_len={}",
        bytes.len().saturating_sub(2),
        bytes.len()
    )
}

fn descriptor_len_summary(name: &str, bytes: &[u8]) -> String {
    format!("descriptor={name},captured_len={}", bytes.len())
}

fn le_u16(bytes: &[u8], index: usize) -> u64 {
    u64::from(bytes[index]) | (u64::from(bytes[index + 1]) << 8)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct UsbEnumerationAnalysis {
    packet_lines: u64,
    stats_lines: u64,
    event_lines: u64,
    harvest_lines: u64,
    mount_events: u64,
    unmount_events: u64,
    suspend_events: u64,
    resume_events: u64,
    first_host_out_events: u64,
    first_in_accepted_events: u64,
    events: BTreeMap<String, u64>,
    setup_lines: u64,
    control_in_lines: u64,
    endpoint_in_lines: u64,
    endpoint_out_lines: u64,
    device_descriptor_requests: u64,
    device_descriptor_replies: u64,
    configuration_descriptor_requests: u64,
    configuration_descriptor_replies: u64,
    string_descriptor_requests: u64,
    bos_descriptor_requests: u64,
    hid_report_descriptor_requests: u64,
    set_address_requests: u64,
    set_configuration_requests: u64,
    hid_get_report_requests: u64,
    hid_set_report_requests: u64,
    hid_output_reports: u64,
    hid_feature_reports: u64,
    first_device_vid_pid: Option<String>,
    first_device_identity: Option<String>,
    first_device_class: Option<String>,
    first_device_bcd_usb: Option<String>,
    first_device_bcd_device: Option<String>,
    first_device_max_packet: Option<u64>,
    first_device_configurations: Option<u64>,
    first_configuration_interfaces: Option<u64>,
    known_vendor_requests: BTreeMap<String, u64>,
    control_payload_replies: BTreeMap<String, u64>,
}

fn analyze_enumeration(text: &str) -> UsbEnumerationAnalysis {
    let mut analysis = UsbEnumerationAnalysis::default();
    for line in text.lines() {
        if line.starts_with("usb-packet ") {
            analysis.add_packet_line(line);
        } else if line.starts_with("usb-packet-stats ") {
            analysis.stats_lines += 1;
        } else if line.starts_with("usb-event ") {
            analysis.add_event_line(line);
        } else if line.starts_with(HARVEST_PREFIX) {
            analysis.harvest_lines += 1;
        }
    }
    analysis
}

impl UsbEnumerationAnalysis {
    fn add_packet_line(&mut self, line: &str) {
        self.packet_lines += 1;
        let fields = fields(line);
        match fields.get("dir").copied() {
            Some("setup") => {
                self.setup_lines += 1;
                self.add_setup_fields(&fields);
            }
            Some("control-in") => {
                self.control_in_lines += 1;
                self.add_control_payload_fields(&fields);
            }
            Some("in") => self.endpoint_in_lines += 1,
            Some("out") => self.endpoint_out_lines += 1,
            _ => {}
        }
        self.add_hid_report_fields(&fields);
    }

    fn add_event_line(&mut self, line: &str) {
        self.event_lines += 1;
        let fields = fields(line);
        let event = fields.get("event").copied().unwrap_or("unknown");
        bump(&mut self.events, event);
        match event {
            "mount" => self.mount_events += 1,
            "unmount" => self.unmount_events += 1,
            "suspend" => self.suspend_events += 1,
            "resume" => self.resume_events += 1,
            "first-host-out" => self.first_host_out_events += 1,
            "first-in-accepted" => self.first_in_accepted_events += 1,
            _ => {}
        }
    }

    fn add_setup_fields(&mut self, fields: &BTreeMap<&str, &str>) {
        let Some(setup) = decode_setup_fields(fields) else {
            return;
        };
        match setup.request_name.as_str() {
            "set_address" => self.set_address_requests += 1,
            "set_configuration" => self.set_configuration_requests += 1,
            "hid_get_report" => self.hid_get_report_requests += 1,
            "hid_set_report" => self.hid_set_report_requests += 1,
            _ => {}
        }
        if setup.request_name == "get_descriptor" {
            match setup.descriptor_type {
                Some("device") => self.device_descriptor_requests += 1,
                Some("configuration") => self.configuration_descriptor_requests += 1,
                Some("string") => self.string_descriptor_requests += 1,
                Some("bos") => self.bos_descriptor_requests += 1,
                Some("hid_report") => self.hid_report_descriptor_requests += 1,
                _ => {}
            }
        }
        if let Some(known_request) = setup.known_request {
            bump(&mut self.known_vendor_requests, known_request);
        }
    }

    fn add_control_payload_fields(&mut self, fields: &BTreeMap<&str, &str>) {
        let Some(payload) = decode_control_payload_fields(fields) else {
            return;
        };
        bump(&mut self.control_payload_replies, &payload.summary);
        match payload.descriptor_type {
            Some("device") => {
                self.device_descriptor_replies += 1;
                if self.first_device_vid_pid.is_none() {
                    if let Some(facts) = device_descriptor_facts(fields) {
                        self.first_device_vid_pid = Some(facts.vid_pid);
                        self.first_device_identity = Some(facts.identity);
                        self.first_device_class = Some(facts.class);
                        self.first_device_bcd_usb = Some(facts.bcd_usb);
                        self.first_device_bcd_device = Some(facts.bcd_device);
                        self.first_device_max_packet = Some(facts.max_packet);
                        self.first_device_configurations = Some(facts.configurations);
                    }
                }
            }
            Some("configuration") => {
                self.configuration_descriptor_replies += 1;
                if self.first_configuration_interfaces.is_none() {
                    self.first_configuration_interfaces =
                        configuration_descriptor_interfaces(fields);
                }
            }
            _ => {}
        }
    }

    fn add_hid_report_fields(&mut self, fields: &BTreeMap<&str, &str>) {
        let Some(report) = decode_hid_report_metadata(fields) else {
            return;
        };
        match report.report_type_name {
            Some("output") => self.hid_output_reports += 1,
            Some("feature") => self.hid_feature_reports += 1,
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceDescriptorFacts {
    vid_pid: String,
    identity: String,
    class: String,
    bcd_usb: String,
    bcd_device: String,
    max_packet: u64,
    configurations: u64,
}

fn device_descriptor_facts(fields: &BTreeMap<&str, &str>) -> Option<DeviceDescriptorFacts> {
    let bytes = hex_bytes(fields.get("data"))?;
    if bytes.len() < 18 {
        return None;
    }
    let vid = le_u16(&bytes, 8);
    let pid = le_u16(&bytes, 10);
    let class = u64::from(bytes[4]);
    let subclass = u64::from(bytes[5]);
    let protocol = u64::from(bytes[6]);
    Some(DeviceDescriptorFacts {
        vid_pid: format!("{}:{}", hex_u16(vid), hex_u16(pid)),
        identity: known_device_identity(vid, pid, class, subclass, protocol).to_string(),
        class: format!(
            "class=0x{:02X},subclass=0x{:02X},protocol=0x{:02X}",
            class, subclass, protocol
        ),
        bcd_usb: hex_u16(le_u16(&bytes, 2)),
        bcd_device: hex_u16(le_u16(&bytes, 12)),
        max_packet: u64::from(bytes[7]),
        configurations: u64::from(bytes[17]),
    })
}

fn known_device_identity(
    vid: u64,
    pid: u64,
    class: u64,
    subclass: u64,
    protocol: u64,
) -> &'static str {
    match (vid, pid, class, subclass, protocol) {
        (0x2E8A, 0xCAF0, 0xEF, 0x02, 0x01) => "couchlink_setup_cdc_winusb",
        (0x045E, 0x028E, 0xFF, 0xFF, 0xFF) => "couchlink_xinput_maple_debug_shape",
        (0x2E8A, 0xCAF1, 0x00, 0x00, 0x00) => "couchlink_keyboard_hid_boot_shape",
        (0x054C, 0x0268, 0x00, 0x00, 0x00) => "couchlink_ps3_hid_shape",
        (0x054C, 0x09CC, 0x00, 0x00, 0x00) => "couchlink_ps4_hid_shape",
        (0x0E6F, 0x02A4, 0xFF, 0xFF, 0xFF) => "couchlink_xboxone_xgip_shape",
        _ => "unknown_usb_device_identity",
    }
}

fn configuration_descriptor_interfaces(fields: &BTreeMap<&str, &str>) -> Option<u64> {
    let bytes = hex_bytes(fields.get("data"))?;
    (bytes.len() >= 5).then_some(u64::from(bytes[4]))
}

fn write_enumeration_analysis(out: &mut String, analysis: &UsbEnumerationAnalysis) {
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("verdict={}\n", enumeration_verdict(analysis)),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("packet_lines={}\n", analysis.packet_lines),
    );
    let _ = std::fmt::Write::write_fmt(out, format_args!("stats_lines={}\n", analysis.stats_lines));
    let _ = std::fmt::Write::write_fmt(out, format_args!("event_lines={}\n", analysis.event_lines));
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("harvest_lines={}\n", analysis.harvest_lines),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("mount_events={}\n", analysis.mount_events),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("unmount_events={}\n", analysis.unmount_events),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("suspend_events={}\n", analysis.suspend_events),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("resume_events={}\n", analysis.resume_events),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("first_host_out_events={}\n", analysis.first_host_out_events),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "first_in_accepted_events={}\n",
            analysis.first_in_accepted_events
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("events={}\n", format_count_map(&analysis.events)),
    );
    let _ = std::fmt::Write::write_fmt(out, format_args!("setup_lines={}\n", analysis.setup_lines));
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("control_in_lines={}\n", analysis.control_in_lines),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("endpoint_in_lines={}\n", analysis.endpoint_in_lines),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("endpoint_out_lines={}\n", analysis.endpoint_out_lines),
    );
    write_bool(
        out,
        "device_descriptor_request",
        analysis.device_descriptor_requests > 0,
    );
    write_bool(
        out,
        "device_descriptor_reply",
        analysis.device_descriptor_replies > 0,
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_vid_pid={}\n",
            analysis.first_device_vid_pid.as_deref().unwrap_or("-")
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_identity={}\n",
            analysis.first_device_identity.as_deref().unwrap_or("-")
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_class={}\n",
            analysis.first_device_class.as_deref().unwrap_or("-")
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_bcd_usb={}\n",
            analysis.first_device_bcd_usb.as_deref().unwrap_or("-")
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_bcd_device={}\n",
            analysis.first_device_bcd_device.as_deref().unwrap_or("-")
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_max_packet={}\n",
            analysis
                .first_device_max_packet
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "device_configurations={}\n",
            analysis
                .first_device_configurations
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    );
    write_bool(
        out,
        "configuration_descriptor_request",
        analysis.configuration_descriptor_requests > 0,
    );
    write_bool(
        out,
        "configuration_descriptor_reply",
        analysis.configuration_descriptor_replies > 0,
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "configuration_interfaces={}\n",
            analysis
                .first_configuration_interfaces
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    );
    write_bool(
        out,
        "set_address_request",
        analysis.set_address_requests > 0,
    );
    write_bool(
        out,
        "set_configuration_request",
        analysis.set_configuration_requests > 0,
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "string_descriptor_requests={}\n",
            analysis.string_descriptor_requests
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "bos_descriptor_requests={}\n",
            analysis.bos_descriptor_requests
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "hid_report_descriptor_requests={}\n",
            analysis.hid_report_descriptor_requests
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "hid_get_report_requests={}\n",
            analysis.hid_get_report_requests
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "hid_set_report_requests={}\n",
            analysis.hid_set_report_requests
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("hid_output_reports={}\n", analysis.hid_output_reports),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("hid_feature_reports={}\n", analysis.hid_feature_reports),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "known_vendor_requests={}\n",
            format_count_map(&analysis.known_vendor_requests)
        ),
    );
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!(
            "control_payload_replies={}\n",
            format_count_map(&analysis.control_payload_replies)
        ),
    );
}

fn write_bool(out: &mut String, key: &str, value: bool) {
    let _ = std::fmt::Write::write_fmt(
        out,
        format_args!("{key}={}\n", if value { "yes" } else { "no" }),
    );
}

fn enumeration_verdict(analysis: &UsbEnumerationAnalysis) -> &'static str {
    if analysis.packet_lines == 0 {
        if analysis.first_host_out_events > 0 || analysis.first_in_accepted_events > 0 {
            "runtime_usb_events_no_raw_packets"
        } else if analysis.mount_events > 0 {
            "mounted_no_raw_packets"
        } else if analysis.event_lines > 0 {
            "usb_lifecycle_events_only_no_raw_packets"
        } else if analysis.harvest_lines > 0 || analysis.stats_lines > 0 {
            "harvest_or_stats_only_no_raw_packets"
        } else {
            "no_usb_packet_evidence"
        }
    } else if analysis.endpoint_in_lines > 0 || analysis.endpoint_out_lines > 0 {
        "endpoint_traffic_observed"
    } else if analysis.first_host_out_events > 0 || analysis.first_in_accepted_events > 0 {
        "runtime_usb_events_no_endpoint_packets"
    } else if analysis.mount_events > 0 {
        "mounted_no_endpoint_traffic"
    } else if analysis.set_configuration_requests > 0 {
        "configured_no_endpoint_traffic"
    } else if analysis.configuration_descriptor_replies > 0 {
        "configuration_descriptor_seen_not_configured"
    } else if analysis.device_descriptor_replies > 0 {
        "device_descriptor_seen_no_configuration_reply"
    } else if analysis.device_descriptor_requests > 0 || analysis.setup_lines > 0 {
        "setup_requests_seen_no_descriptor_reply"
    } else if analysis.control_in_lines > 0 {
        "control_replies_seen_without_setup_context"
    } else {
        "endpoint_only_or_unclassified_packets"
    }
}

impl UsbPacketSummary {
    fn add_packet_line(
        &mut self,
        line: &str,
        seen_sequences: &mut BTreeSet<u64>,
        previous_seq: &mut Option<u64>,
        previous_t_ms: &mut Option<u64>,
    ) {
        self.packet_lines += 1;
        let fields = fields(line);
        if let Some(value) = fields.get("dir") {
            bump(&mut self.directions, value);
        }
        if let Some(value) = fields.get("src") {
            bump(&mut self.sources, value);
        }
        if let Some(value) = fields.get("reason") {
            bump(&mut self.reasons, value);
        }
        if let Some(setup) = decode_setup_fields(&fields) {
            bump(&mut self.setup_directions, setup.direction);
            bump(&mut self.setup_types, setup.request_type);
            bump(&mut self.setup_recipients, setup.recipient);
            bump(&mut self.setup_requests, &setup.request_name);
            if let Some(descriptor_type) = setup.descriptor_type {
                bump(&mut self.setup_descriptor_requests, descriptor_type);
            }
            if let Some(known_request) = setup.known_request {
                bump(&mut self.setup_known_requests, known_request);
            }
        }
        if let Some(payload) = decode_control_payload_fields(&fields) {
            bump(&mut self.control_payload_kinds, payload.kind);
            if let Some(descriptor_type) = payload.descriptor_type {
                bump(&mut self.control_descriptor_replies, descriptor_type);
            }
            bump(&mut self.control_payload_summaries, &payload.summary);
        }
        if let Some(report) = decode_hid_report_metadata(&fields) {
            self.hid_report_lines += 1;
            if let Some(report_type_name) = report.report_type_name {
                bump(&mut self.hid_report_types, report_type_name);
            } else if let Some(report_type) = report.report_type {
                bump(
                    &mut self.hid_report_types,
                    &format!("unknown_{report_type}"),
                );
            }
            if let Some(report_id) = report.report_id {
                bump(&mut self.hid_report_ids, &hex_u8(report_id));
            }
        }
        max_assign(
            &mut self.max_reported_packet_len,
            parsed_u64(fields.get("len")),
        );
        max_assign(
            &mut self.max_captured_len,
            parsed_u64(fields.get("captured")),
        );
        if let Some(truncated) = parsed_u64(fields.get("truncated")) {
            if truncated > 0 {
                self.truncated_packet_lines += 1;
            }
            max_assign(&mut self.max_packet_truncated_bytes, Some(truncated));
        } else if let (Some(reported), Some(captured)) = (
            parsed_u64(fields.get("len")),
            parsed_u64(fields.get("captured")),
        ) {
            let truncated = reported.saturating_sub(captured);
            if truncated > 0 {
                self.truncated_packet_lines += 1;
                max_assign(&mut self.max_packet_truncated_bytes, Some(truncated));
            }
        }
        max_assign(
            &mut self.max_truncated_bytes,
            parsed_u64(fields.get("dropped")),
        );
        max_assign(
            &mut self.max_suppressed_idle_reports,
            parsed_u64(fields.get("suppressed")),
        );
        self.add_packet_time(parsed_u64(fields.get("t")), previous_t_ms);

        let Some(seq) = parsed_u64(fields.get("seq")) else {
            return;
        };
        if self.first_seq.is_none() {
            self.first_seq = Some(seq);
        }
        self.last_seq = Some(seq);
        min_assign(&mut self.min_seq, Some(seq));
        max_assign(&mut self.max_seq, Some(seq));
        if !seen_sequences.insert(seq) {
            self.duplicate_sequence_numbers += 1;
        }
        if let Some(prev) = *previous_seq {
            if seq > prev + 1 {
                self.missing_sequence_numbers += seq - prev - 1;
            } else if seq < prev {
                self.out_of_order_sequence_lines += 1;
            }
        }
        *previous_seq = Some(seq);
    }

    fn add_packet_time(&mut self, t_ms: Option<u64>, previous_t_ms: &mut Option<u64>) {
        let Some(t_ms) = t_ms else {
            return;
        };
        if self.first_packet_t_ms.is_none() {
            self.first_packet_t_ms = Some(t_ms);
        }
        self.last_packet_t_ms = Some(t_ms);
        min_assign(&mut self.min_packet_t_ms, Some(t_ms));
        max_assign(&mut self.max_packet_t_ms, Some(t_ms));
        if let (Some(min), Some(max)) = (self.min_packet_t_ms, self.max_packet_t_ms) {
            self.packet_time_span_ms = max.checked_sub(min);
        }
        if let Some(previous) = *previous_t_ms {
            if t_ms >= previous {
                max_assign(&mut self.max_inter_packet_gap_ms, Some(t_ms - previous));
            } else {
                self.packet_time_regressions += 1;
            }
        }
        *previous_t_ms = Some(t_ms);
    }

    fn add_stats_line(&mut self, line: &str) {
        self.stats_lines += 1;
        let fields = fields(line);
        if let Some(total) = parsed_u64(fields.get("total")) {
            self.last_stats_total_packets = Some(total);
            max_assign(&mut self.max_stats_total_packets, Some(total));
        }
        max_assign(
            &mut self.max_stats_truncated_bytes,
            parsed_u64(fields.get("truncated_bytes")),
        );
        max_assign(
            &mut self.max_stats_truncated_packets,
            parsed_u64(fields.get("truncated_packets")),
        );
        max_assign(
            &mut self.max_stats_idle_in_suppressed,
            parsed_u64(fields.get("idle_in_suppressed")),
        );
        for key in ["in", "out", "setup", "control_in"] {
            if let Some(value) = parsed_u64(fields.get(key)) {
                let entry = self.stats_direction_max.entry(key.to_string()).or_default();
                if value > *entry {
                    *entry = value;
                }
            }
        }
    }

    fn add_event_line(&mut self, line: &str) {
        self.event_lines += 1;
        let fields = fields(line);
        bump(
            &mut self.events,
            fields.get("event").copied().unwrap_or("unknown"),
        );
    }

    fn add_harvest_line(&mut self, line: &str) {
        self.harvest_lines += 1;
        let Some(value) = harvest_value(line) else {
            bump(&mut self.harvest_statuses, "malformed");
            return;
        };
        let status = json_string(&value, "status").unwrap_or_else(|| "unknown".to_string());
        bump(&mut self.harvest_statuses, &status);
        max_assign(
            &mut self.max_harvest_duration_ms,
            json_u64(&value, "duration_ms"),
        );
        max_assign(
            &mut self.max_harvest_lost_bytes,
            json_u64(&value, "lost_bytes"),
        );
        max_assign(
            &mut self.max_harvest_chunk_count,
            json_u64(&value, "chunk_count"),
        );
        max_assign(
            &mut self.max_harvest_expected_chunks,
            json_u64(&value, "expected_chunks"),
        );
        max_assign(
            &mut self.max_harvest_missing_chunks,
            json_u64(&value, "missing_chunk_count"),
        );
        max_assign(
            &mut self.max_harvest_duplicate_chunks,
            json_u64(&value, "duplicate_chunk_count"),
        );
        max_assign(
            &mut self.max_harvest_diag_bytes,
            json_u64(&value, "diag_bytes"),
        );
        max_assign(
            &mut self.max_harvest_diag_lines,
            json_u64(&value, "diag_lines"),
        );
        max_assign(
            &mut self.max_harvest_packet_lines,
            json_u64(&value, "packet_lines"),
        );
        max_assign(
            &mut self.max_harvest_raw_packet_lines,
            json_u64(&value, "raw_packet_lines"),
        );
        max_assign(
            &mut self.max_harvest_stats_lines,
            json_u64(&value, "stats_lines"),
        );
        max_assign(
            &mut self.max_harvest_event_lines,
            json_u64(&value, "event_lines"),
        );
        max_assign(
            &mut self.max_harvest_new_lines,
            json_u64(&value, "new_lines"),
        );
        max_assign(
            &mut self.max_harvest_duplicate_lines,
            json_u64(&value, "duplicate_lines"),
        );
        if let Some(chunk_complete) = json_bool(&value, "chunk_complete") {
            bump(
                &mut self.harvest_chunk_statuses,
                if chunk_complete {
                    "complete"
                } else {
                    "incomplete"
                },
            );
        }
    }

    fn merge_from(&mut self, other: &UsbPacketSummary) {
        self.packet_lines += other.packet_lines;
        self.stats_lines += other.stats_lines;
        self.event_lines += other.event_lines;
        self.harvest_lines += other.harvest_lines;
        merge_counts(&mut self.events, &other.events);
        merge_counts(&mut self.directions, &other.directions);
        merge_counts(&mut self.sources, &other.sources);
        merge_counts(&mut self.reasons, &other.reasons);
        merge_counts(&mut self.setup_directions, &other.setup_directions);
        merge_counts(&mut self.setup_types, &other.setup_types);
        merge_counts(&mut self.setup_recipients, &other.setup_recipients);
        merge_counts(&mut self.setup_requests, &other.setup_requests);
        merge_counts(
            &mut self.setup_descriptor_requests,
            &other.setup_descriptor_requests,
        );
        merge_counts(&mut self.setup_known_requests, &other.setup_known_requests);
        merge_counts(
            &mut self.control_payload_kinds,
            &other.control_payload_kinds,
        );
        merge_counts(
            &mut self.control_descriptor_replies,
            &other.control_descriptor_replies,
        );
        merge_counts(
            &mut self.control_payload_summaries,
            &other.control_payload_summaries,
        );
        merge_counts(&mut self.harvest_statuses, &other.harvest_statuses);
        self.hid_report_lines += other.hid_report_lines;
        merge_counts(&mut self.hid_report_types, &other.hid_report_types);
        merge_counts(&mut self.hid_report_ids, &other.hid_report_ids);
        if self.first_seq.is_none() {
            self.first_seq = other.first_seq;
        }
        if other.last_seq.is_some() {
            self.last_seq = other.last_seq;
        }
        min_assign(&mut self.min_seq, other.min_seq);
        max_assign(&mut self.max_seq, other.max_seq);
        self.missing_sequence_numbers += other.missing_sequence_numbers;
        self.duplicate_sequence_numbers += other.duplicate_sequence_numbers;
        self.out_of_order_sequence_lines += other.out_of_order_sequence_lines;
        max_assign(
            &mut self.max_reported_packet_len,
            other.max_reported_packet_len,
        );
        max_assign(&mut self.max_captured_len, other.max_captured_len);
        self.truncated_packet_lines += other.truncated_packet_lines;
        max_assign(
            &mut self.max_packet_truncated_bytes,
            other.max_packet_truncated_bytes,
        );
        max_assign(&mut self.max_truncated_bytes, other.max_truncated_bytes);
        max_assign(
            &mut self.max_suppressed_idle_reports,
            other.max_suppressed_idle_reports,
        );
        if other.last_stats_total_packets.is_some() {
            self.last_stats_total_packets = other.last_stats_total_packets;
        }
        max_assign(
            &mut self.max_stats_total_packets,
            other.max_stats_total_packets,
        );
        max_assign(
            &mut self.max_stats_truncated_bytes,
            other.max_stats_truncated_bytes,
        );
        max_assign(
            &mut self.max_stats_truncated_packets,
            other.max_stats_truncated_packets,
        );
        max_assign(
            &mut self.max_stats_idle_in_suppressed,
            other.max_stats_idle_in_suppressed,
        );
        for (key, value) in &other.stats_direction_max {
            let entry = self.stats_direction_max.entry(key.clone()).or_default();
            if *value > *entry {
                *entry = *value;
            }
        }
        if self.first_packet_t_ms.is_none() {
            self.first_packet_t_ms = other.first_packet_t_ms;
        }
        if other.last_packet_t_ms.is_some() {
            self.last_packet_t_ms = other.last_packet_t_ms;
        }
        min_assign(&mut self.min_packet_t_ms, other.min_packet_t_ms);
        max_assign(&mut self.max_packet_t_ms, other.max_packet_t_ms);
        max_assign(&mut self.packet_time_span_ms, other.packet_time_span_ms);
        max_assign(
            &mut self.max_inter_packet_gap_ms,
            other.max_inter_packet_gap_ms,
        );
        self.packet_time_regressions += other.packet_time_regressions;
        max_assign(
            &mut self.max_harvest_duration_ms,
            other.max_harvest_duration_ms,
        );
        max_assign(
            &mut self.max_harvest_lost_bytes,
            other.max_harvest_lost_bytes,
        );
        max_assign(
            &mut self.max_harvest_chunk_count,
            other.max_harvest_chunk_count,
        );
        max_assign(
            &mut self.max_harvest_expected_chunks,
            other.max_harvest_expected_chunks,
        );
        max_assign(
            &mut self.max_harvest_missing_chunks,
            other.max_harvest_missing_chunks,
        );
        max_assign(
            &mut self.max_harvest_duplicate_chunks,
            other.max_harvest_duplicate_chunks,
        );
        max_assign(
            &mut self.max_harvest_diag_bytes,
            other.max_harvest_diag_bytes,
        );
        max_assign(
            &mut self.max_harvest_diag_lines,
            other.max_harvest_diag_lines,
        );
        max_assign(
            &mut self.max_harvest_packet_lines,
            other.max_harvest_packet_lines,
        );
        max_assign(
            &mut self.max_harvest_raw_packet_lines,
            other.max_harvest_raw_packet_lines,
        );
        max_assign(
            &mut self.max_harvest_stats_lines,
            other.max_harvest_stats_lines,
        );
        max_assign(
            &mut self.max_harvest_event_lines,
            other.max_harvest_event_lines,
        );
        max_assign(&mut self.max_harvest_new_lines, other.max_harvest_new_lines);
        max_assign(
            &mut self.max_harvest_duplicate_lines,
            other.max_harvest_duplicate_lines,
        );
        merge_counts(
            &mut self.harvest_chunk_statuses,
            &other.harvest_chunk_statuses,
        );
    }
}

fn fields(line: &str) -> BTreeMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|part| part.split_once('='))
        .collect()
}

fn parsed_u64(value: Option<&&str>) -> Option<u64> {
    value.and_then(|value| {
        value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .map_or_else(
                || value.parse::<u64>().ok(),
                |hex| u64::from_str_radix(hex, 16).ok(),
            )
    })
}

fn cloned_field(value: Option<&&str>) -> Option<String> {
    value.map(|value| (*value).to_string())
}

fn display_field<'a>(value: Option<&&'a str>) -> &'a str {
    value.copied().unwrap_or("-")
}

fn harvest_value(line: &str) -> Option<Value> {
    let json = line.strip_prefix(HARVEST_PREFIX)?;
    serde_json::from_str(json).ok()
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(|value| value.to_string())
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    let value = value.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
}

fn json_bool(value: &Value, key: &str) -> Option<bool> {
    let value = value.get(key)?;
    value.as_bool().or_else(|| {
        value.as_str().and_then(|value| match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
    })
}

fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.to_string()).or_default() += 1;
}

fn merge_counts(into: &mut BTreeMap<String, u64>, from: &BTreeMap<String, u64>) {
    for (key, value) in from {
        *into.entry(key.clone()).or_default() += value;
    }
}

fn format_count_map(map: &BTreeMap<String, u64>) -> String {
    if map.is_empty() {
        "-".to_string()
    } else {
        map.iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect::<Vec<_>>()
            .join(";")
    }
}

fn min_assign(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        if target.map(|current| value < current).unwrap_or(true) {
            *target = Some(value);
        }
    }
}

fn max_assign(target: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        if target.map(|current| value > current).unwrap_or(true) {
            *target = Some(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_packets_stats_and_sequence_gaps() {
        let text = "\
usb-packet seq=1 t=10 dir=out src=vendor len=3 captured=3 dropped=0 suppressed=0 reason=host-out data=010203
usb-packet seq=3 t=11 dir=in src=xinput len=20 captured=20 dropped=0 suppressed=2 reason=changed data=00
usb-packet seq=3 t=12 dir=in src=xinput len=20 captured=20 dropped=0 suppressed=0 reason=changed data=00
usb-packet seq=2 t=13 dir=setup src=vendor-control len=10 captured=8 truncated=2 dropped=4 suppressed=0 reason=control-setup data=C0
usb-event t=13 event=mount
usb-packet-stats t=14 total=64 in=2 out=1 setup=1 control_in=0 truncated_bytes=4 truncated_packets=1 idle_in_suppressed=9
";
        let summary = summarize_text(text);
        assert_eq!(summary.packet_lines, 4);
        assert_eq!(summary.event_lines, 1);
        assert_eq!(summary.stats_lines, 1);
        assert_eq!(summary.events["mount"], 1);
        assert_eq!(summary.directions["in"], 2);
        assert_eq!(summary.sources["xinput"], 2);
        assert_eq!(summary.reasons["changed"], 2);
        assert_eq!(summary.first_seq, Some(1));
        assert_eq!(summary.last_seq, Some(2));
        assert_eq!(summary.min_seq, Some(1));
        assert_eq!(summary.max_seq, Some(3));
        assert_eq!(summary.missing_sequence_numbers, 1);
        assert_eq!(summary.duplicate_sequence_numbers, 1);
        assert_eq!(summary.out_of_order_sequence_lines, 1);
        assert_eq!(summary.max_reported_packet_len, Some(20));
        assert_eq!(summary.max_captured_len, Some(20));
        assert_eq!(summary.truncated_packet_lines, 1);
        assert_eq!(summary.max_packet_truncated_bytes, Some(2));
        assert_eq!(summary.max_truncated_bytes, Some(4));
        assert_eq!(summary.max_suppressed_idle_reports, Some(2));
        assert_eq!(summary.last_stats_total_packets, Some(64));
        assert_eq!(summary.max_stats_truncated_packets, Some(1));
        assert_eq!(summary.max_stats_idle_in_suppressed, Some(9));
        assert_eq!(summary.stats_direction_max["setup"], 1);
        assert_eq!(summary.first_packet_t_ms, Some(10));
        assert_eq!(summary.last_packet_t_ms, Some(13));
        assert_eq!(summary.min_packet_t_ms, Some(10));
        assert_eq!(summary.max_packet_t_ms, Some(13));
        assert_eq!(summary.packet_time_span_ms, Some(3));
        assert_eq!(summary.max_inter_packet_gap_ms, Some(1));
        assert_eq!(summary.packet_time_regressions, 0);
        assert_eq!(summary.harvest_lines, 0);
    }

    #[test]
    fn summary_counts_harvest_health() {
        let text = "\
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":3,\"expected_chunks\":4,\"missing_chunk_count\":1,\"duplicate_chunk_count\":2,\"got_last\":true,\"chunk_complete\":false,\"lost_bytes\":4,\"diag_bytes\":256,\"diag_lines\":12,\"packet_lines\":8,\"raw_packet_lines\":6,\"stats_lines\":2,\"event_lines\":1,\"new_lines\":2,\"duplicate_lines\":6,\"total_packet_lines\":12}
# harvest {\"at\":\"2026-06-15T22:30:01-05:00\",\"status\":\"error\",\"duration_ms\":1200,\"error\":\"no log chunks received\"}
# harvest not-json
";
        let summary = summarize_text(text);
        assert_eq!(summary.harvest_lines, 3);
        assert_eq!(summary.harvest_statuses["ok"], 1);
        assert_eq!(summary.harvest_statuses["error"], 1);
        assert_eq!(summary.harvest_statuses["malformed"], 1);
        assert_eq!(summary.max_harvest_duration_ms, Some(1200));
        assert_eq!(summary.max_harvest_lost_bytes, Some(4));
        assert_eq!(summary.max_harvest_chunk_count, Some(3));
        assert_eq!(summary.max_harvest_expected_chunks, Some(4));
        assert_eq!(summary.max_harvest_missing_chunks, Some(1));
        assert_eq!(summary.max_harvest_duplicate_chunks, Some(2));
        assert_eq!(summary.max_harvest_diag_bytes, Some(256));
        assert_eq!(summary.max_harvest_diag_lines, Some(12));
        assert_eq!(summary.max_harvest_packet_lines, Some(8));
        assert_eq!(summary.max_harvest_raw_packet_lines, Some(6));
        assert_eq!(summary.max_harvest_stats_lines, Some(2));
        assert_eq!(summary.max_harvest_event_lines, Some(1));
        assert_eq!(summary.max_harvest_new_lines, Some(2));
        assert_eq!(summary.max_harvest_duplicate_lines, Some(6));
        assert_eq!(summary.harvest_chunk_statuses["incomplete"], 1);
    }

    #[test]
    fn bundle_summary_keeps_per_source_sequence_accounting() {
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: "usb-packet seq=1 dir=out src=vendor reason=host-out\nusb-packet seq=3 dir=out src=vendor reason=host-out\n",
        }];
        let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text: "usb-event t=22 event=mount\nusb-packet-stats total=64 out=2 truncated_bytes=0 truncated_packets=0 idle_in_suppressed=0\n",
        }];
        let summary = summarize_sources(&per_pico, &retained);
        assert_eq!(summary.artifact_schema_version, 8);
        assert_eq!(summary.aggregate.packet_lines, 2);
        assert_eq!(summary.aggregate.event_lines, 1);
        assert_eq!(summary.aggregate.stats_lines, 1);
        assert_eq!(summary.aggregate.events["mount"], 1);
        assert_eq!(summary.aggregate.missing_sequence_numbers, 1);
        assert_eq!(summary.per_pico[0].summary.missing_sequence_numbers, 1);
        assert_eq!(
            summary.retained_logs[0].summary.last_stats_total_packets,
            Some(64)
        );
    }

    #[test]
    fn packet_timeline_keeps_packets_stats_harvest_and_deltas() {
        let text = "\
usb-packet seq=7 t=10 dir=out src=vendor len=3 captured=3 truncated=0 dropped=0 suppressed=0 reason=host-out data=010203
usb-packet seq=8 t=15 dir=setup src=vendor-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup data=C020
usb-event t=18 event=mount
usb-packet-stats t=40 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_complete\":true,\"packet_lines\":8,\"raw_packet_lines\":6,\"new_lines\":2}
usb-packet seq=9 t=12 dir=in src=xinput len=20 captured=20 dropped=0 suppressed=0 reason=changed data=00
";
        let out = packet_timeline_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        assert!(out.contains("# USB packet timeline"));
        assert!(out.contains("packet line=1 seq=7 t=10 dt_ms=- dir=out src=vendor"));
        assert!(out.contains("truncated=0 dropped=0 suppressed=0"));
        assert!(out.contains("packet line=2 seq=8 t=15 dt_ms=5 dir=setup"));
        assert!(out
            .contains("event line=3 t=18 dt_ms=3 event=mount src=- len=- bytes=- remote_wakeup=-"));
        assert!(out.contains(
            "stats line=4 t=40 dt_ms=22 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9"
        ));
        assert!(out.contains("harvest line=5 at=2026-06-15T22:30:00-05:00 status=ok duration_ms=14 chunk_complete=true packet_lines=8 raw_packet_lines=6 new_lines=2 error=-"));
        assert!(out.contains("packet line=6 seq=9 t=12 dt_ms=regression dir=in"));

        let summary = summarize_text(text);
        assert_eq!(summary.max_inter_packet_gap_ms, Some(5));
        assert_eq!(summary.packet_time_regressions, 1);
    }

    #[test]
    fn packet_timeline_sources_omit_empty_sources() {
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: "not packet evidence\n",
        }];
        let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text: "usb-packet seq=2 t=22 dir=out src=hid-output data=050607\n",
        }];
        let out = packet_timeline_text_for_sources(&per_pico, &retained);
        assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
        assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
        assert!(out.contains("packet line=1 seq=2 t=22"));
    }

    #[test]
    fn enumeration_analysis_reports_descriptor_configuration_and_endpoint_phases() {
        let text = "\
usb-packet seq=1 t=10 dir=setup src=standard-control bm=0x80 req=0x06 value=0x0100 index=0x0000 wlen=18 len=8 captured=8 data=8006000100001200
usb-packet seq=2 t=11 dir=control-in src=desc-device len=18 captured=18 dropped=0 reason=control-reply data=12010002FFFFFF405E048E02140101020301
usb-packet seq=3 t=12 dir=setup src=standard-control bm=0x00 req=0x05 value=0x0005 index=0x0000 wlen=0 len=8 captured=8 data=0005050000000000
usb-packet seq=4 t=13 dir=setup src=standard-control bm=0x80 req=0x06 value=0x0200 index=0x0000 wlen=32 len=8 captured=8 data=8006000200002000
usb-packet seq=5 t=14 dir=control-in src=desc-config len=9 captured=9 dropped=0 reason=control-reply data=09022000010100A032
usb-packet seq=6 t=15 dir=setup src=standard-control bm=0x00 req=0x09 value=0x0001 index=0x0000 wlen=0 len=8 captured=8 data=0009010000000000
usb-packet seq=7 t=16 dir=setup src=vendor-control bm=0xC0 req=0x20 value=0x0000 index=0x0007 wlen=38 len=8 captured=8 data=C020000007002600
usb-packet seq=8 t=17 dir=control-in src=ms-os-20 len=38 captured=38 dropped=0 reason=control-reply data=0A000000000003062600
usb-packet seq=9 t=18 dir=out src=vendor len=3 captured=3 dropped=0 reason=host-out data=010203
usb-event t=19 event=mount
";
        let out =
            enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        assert!(out.contains("# USB enumeration analysis"));
        assert!(out.contains("verdict=endpoint_traffic_observed"));
        assert!(out.contains("packet_lines=9"));
        assert!(out.contains("event_lines=1"));
        assert!(out.contains("mount_events=1"));
        assert!(out.contains("events=mount:1"));
        assert!(out.contains("device_descriptor_request=yes"));
        assert!(out.contains("device_descriptor_reply=yes"));
        assert!(out.contains("device_vid_pid=0x045E:0x028E"));
        assert!(out.contains("device_identity=couchlink_xinput_maple_debug_shape"));
        assert!(out.contains("device_class=class=0xFF,subclass=0xFF,protocol=0xFF"));
        assert!(out.contains("device_bcd_usb=0x0200"));
        assert!(out.contains("device_bcd_device=0x0114"));
        assert!(out.contains("device_max_packet=64"));
        assert!(out.contains("device_configurations=1"));
        assert!(out.contains("configuration_descriptor_request=yes"));
        assert!(out.contains("configuration_descriptor_reply=yes"));
        assert!(out.contains("configuration_interfaces=1"));
        assert!(out.contains("set_address_request=yes"));
        assert!(out.contains("set_configuration_request=yes"));
        assert!(out.contains("known_vendor_requests=ms-os-20-descriptor-set:1"));
        assert!(out.contains("control_payload_replies="));
        assert!(out.contains("ms-os-20-descriptor-set:1"));
    }

    #[test]
    fn enumeration_analysis_distinguishes_harvest_only_evidence() {
        let text = "# harvest {\"status\":\"ok\",\"duration_ms\":14,\"packet_lines\":0}\n";
        let out =
            enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        assert!(out.contains("verdict=harvest_or_stats_only_no_raw_packets"));
        assert!(out.contains("packet_lines=0"));
        assert!(out.contains("harvest_lines=1"));
        assert!(out.contains("device_descriptor_request=no"));
    }

    #[test]
    fn enumeration_analysis_distinguishes_lifecycle_only_evidence() {
        let text = "usb-event t=22 event=mount\nusb-event t=24 event=suspend remote_wakeup=1\n";
        let out =
            enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        assert!(out.contains("verdict=mounted_no_raw_packets"));
        assert!(out.contains("packet_lines=0"));
        assert!(out.contains("event_lines=2"));
        assert!(out.contains("mount_events=1"));
        assert!(out.contains("suspend_events=1"));
        assert!(out.contains("first_host_out_events=0"));
        assert!(out.contains("first_in_accepted_events=0"));
        assert!(out.contains("events=mount:1;suspend:1"));
        assert!(out.contains("device_descriptor_request=no"));
    }

    #[test]
    fn enumeration_analysis_distinguishes_runtime_events_without_packets() {
        let text = "\
usb-event t=30 event=first-in-accepted src=xinput bytes=20
usb-event t=31 event=first-host-out src=vendor len=3
";
        let out =
            enumeration_analysis_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        assert!(out.contains("verdict=runtime_usb_events_no_raw_packets"));
        assert!(out.contains("packet_lines=0"));
        assert!(out.contains("event_lines=2"));
        assert!(out.contains("first_host_out_events=1"));
        assert!(out.contains("first_in_accepted_events=1"));
        assert!(out.contains("events=first-host-out:1;first-in-accepted:1"));
    }

    #[test]
    fn enumeration_analysis_sources_omit_empty_sources() {
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: "not packet evidence\n",
        }];
        let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text: "usb-event t=22 event=mount\n",
        }];
        let out = enumeration_analysis_text_for_sources(&per_pico, &retained);
        assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
        assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
        assert!(out.contains("verdict=mounted_no_raw_packets"));
    }

    #[test]
    fn known_device_identity_names_couchlink_usb_shapes() {
        assert_eq!(
            known_device_identity(0x2E8A, 0xCAF0, 0xEF, 0x02, 0x01),
            "couchlink_setup_cdc_winusb"
        );
        assert_eq!(
            known_device_identity(0x045E, 0x028E, 0xFF, 0xFF, 0xFF),
            "couchlink_xinput_maple_debug_shape"
        );
        assert_eq!(
            known_device_identity(0x2E8A, 0xCAF1, 0x00, 0x00, 0x00),
            "couchlink_keyboard_hid_boot_shape"
        );
        assert_eq!(
            known_device_identity(0x054C, 0x0268, 0x00, 0x00, 0x00),
            "couchlink_ps3_hid_shape"
        );
        assert_eq!(
            known_device_identity(0x054C, 0x09CC, 0x00, 0x00, 0x00),
            "couchlink_ps4_hid_shape"
        );
        assert_eq!(
            known_device_identity(0x0E6F, 0x02A4, 0xFF, 0xFF, 0xFF),
            "couchlink_xboxone_xgip_shape"
        );
        assert_eq!(
            known_device_identity(0x1234, 0x5678, 0x00, 0x00, 0x00),
            "unknown_usb_device_identity"
        );
    }

    #[test]
    fn control_transfer_text_keeps_setup_and_control_in_rows() {
        let text = "\
usb-packet seq=7 t=10 dir=out src=vendor len=3 captured=3 dropped=0 reason=host-out data=010203
usb-packet seq=8 t=11 dir=setup src=vendor-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 data=C020020104030040
usb-packet seq=9 t=12 dir=control-in src=desc-device len=18 captured=18 dropped=0 suppressed=0 reason=control-reply data=12010002
usb-packet seq=10 t=13 dir=setup src=standard-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0x80 req=0x06 value=0x0301 index=0x0409 wlen=255 data=800601030904FF00
usb-packet-stats t=20 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9
";
        let out =
            control_transfers_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        let summary = summarize_text(text);
        assert_eq!(summary.setup_directions["device_to_host"], 2);
        assert_eq!(summary.setup_types["vendor"], 1);
        assert_eq!(summary.setup_types["standard"], 1);
        assert_eq!(summary.setup_recipients["device"], 2);
        assert_eq!(summary.setup_requests["vendor_0x20"], 1);
        assert_eq!(summary.setup_requests["get_descriptor"], 1);
        assert_eq!(summary.setup_descriptor_requests["string"], 1);
        assert_eq!(summary.control_payload_kinds["usb_descriptor"], 1);
        assert_eq!(summary.control_descriptor_replies["device"], 1);
        assert_eq!(
            summary.control_payload_summaries["descriptor=device,captured_len=4"],
            1
        );
        assert!(out.contains("# source_label=02E22DA9"));
        assert!(out.contains("setup line=2 seq=8 t=11 src=vendor-control bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 len=8 captured=8 decode=device_to_host/vendor/device request=vendor_0x20 descriptor=- descriptor_index=- language_id=- known=- data=C020020104030040"));
        assert!(out.contains("control-in line=3 seq=9 t=12 src=desc-device reason=control-reply len=18 captured=18 dropped=0 payload_kind=usb_descriptor payload_descriptor=device payload_summary=descriptor=device,captured_len=4 data=12010002"));
        assert!(out.contains("setup line=4 seq=10 t=13 src=standard-control bm=0x80 req=0x06 value=0x0301 index=0x0409 wlen=255 len=8 captured=8 decode=device_to_host/standard/device request=get_descriptor descriptor=string descriptor_index=1 language_id=0x0409 known=- data=800601030904FF00"));
        assert!(!out.contains("host-out"));
        assert!(!out.contains("usb-packet-stats"));
    }

    #[test]
    fn control_transfer_sources_omit_empty_sources() {
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: "usb-packet seq=1 dir=out src=vendor reason=host-out\n",
        }];
        let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text: "usb-packet seq=8 t=11 dir=setup src=vendor-control bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 len=8 captured=8 data=C020020104030040\n",
        }];
        let out = control_transfers_text_for_sources(&per_pico, &retained);
        assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
        assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
        assert!(out.contains("setup line=1 seq=8"));
    }

    #[test]
    fn hid_report_summary_and_transcript_extract_report_metadata() {
        let text = "\
usb-packet seq=1 t=10 dir=setup src=hid-get-report len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0xA1 req=0x01 value=0x03EF index=0x0002 wlen=64 data=A101EF0302004000
usb-packet seq=2 t=11 dir=setup src=hid-set-report len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0x21 req=0x09 value=0x0201 index=0x0002 wlen=4 data=2109010202000400
usb-packet seq=3 t=12 dir=out src=hid-output len=3 captured=3 dropped=0 suppressed=0 reason=host-out report_id=0x01 report_type=2 data=050607
usb-packet seq=4 t=13 dir=out src=hid-feature len=2 captured=2 dropped=0 suppressed=0 reason=host-out report_id=0xEF report_type=3 data=AABB
";
        let summary = summarize_text(text);
        assert_eq!(summary.hid_report_lines, 4);
        assert_eq!(summary.hid_report_types["feature"], 2);
        assert_eq!(summary.hid_report_types["output"], 2);
        assert_eq!(summary.hid_report_ids["0xEF"], 2);
        assert_eq!(summary.hid_report_ids["0x01"], 2);

        let out = hid_reports_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        assert!(out.contains("# USB HID report transcript"));
        assert!(out.contains(
            "request=hid_get_report report_id=0xEF report_type=3 report_type_name=feature"
        ));
        assert!(out.contains(
            "request=hid_set_report report_id=0x01 report_type=2 report_type_name=output"
        ));
        assert!(out.contains(
            "dir=out src=hid-output request=- report_id=0x01 report_type=2 report_type_name=output"
        ));
        assert!(out.contains("dir=out src=hid-feature request=- report_id=0xEF report_type=3 report_type_name=feature"));
    }

    #[test]
    fn hid_report_sources_omit_empty_sources() {
        let per_pico = [UsbPacketSummarySource {
            label: "02E22DA9".to_string(),
            path: "picos/02E22DA9/usb-packets.txt".to_string(),
            text: "usb-packet seq=1 dir=out src=vendor reason=host-out\n",
        }];
        let retained = [UsbPacketSummarySource {
            label: "usb-packets.log".to_string(),
            path: "debug-packets/usb-packets.log".to_string(),
            text:
                "usb-packet seq=2 dir=out src=hid-output report_id=0x01 report_type=2 data=050607\n",
        }];
        let out = hid_reports_text_for_sources(&per_pico, &retained);
        assert!(!out.contains("picos/02E22DA9/usb-packets.txt"));
        assert!(out.contains("## usb-packets.log (debug-packets/usb-packets.log)"));
        assert!(out.contains("hid-report line=1 seq=2"));
    }

    #[test]
    fn setup_decode_names_known_vendor_requests() {
        let text = "\
usb-packet seq=1 t=10 dir=setup src=vendor-control bm=0xC0 req=0x20 value=0x0000 index=0x0007 wlen=38 len=8 captured=8 data=C020000007002600
usb-packet seq=2 t=11 dir=setup src=vendor-control bm=0xC1 req=0x01 value=0x0000 index=0x0002 wlen=16388 len=8 captured=8 data=C101000002000440
usb-packet seq=3 t=12 dir=control-in src=ms-os-20 len=38 captured=38 dropped=0 reason=control-reply data=0A000000000003062600
";
        let out =
            control_transfers_text_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text);
        let summary = summarize_text(text);
        assert_eq!(summary.setup_requests["ms-os-20-descriptor-set"], 1);
        assert_eq!(summary.setup_requests["couchlink-setup-diag-log"], 1);
        assert_eq!(summary.setup_known_requests["ms-os-20-descriptor-set"], 1);
        assert_eq!(summary.setup_known_requests["couchlink-setup-diag-log"], 1);
        assert_eq!(summary.control_payload_kinds["known_vendor_payload"], 1);
        assert_eq!(
            summary.control_payload_summaries["ms-os-20-descriptor-set"],
            1
        );
        assert!(out.contains("request=ms-os-20-descriptor-set"));
        assert!(out.contains("known=ms-os-20-descriptor-set"));
        assert!(out.contains("request=couchlink-setup-diag-log"));
        assert!(out.contains("known=couchlink-setup-diag-log"));
        assert!(out.contains("payload_kind=known_vendor_payload payload_descriptor=- payload_summary=ms-os-20-descriptor-set"));
    }

    #[test]
    fn records_jsonl_normalizes_packet_and_stats_lines() {
        let text = "\
usb-packet seq=7 t=10 dir=control-in src=desc-device len=18 captured=18 truncated=0 dropped=0 suppressed=0 reason=control-reply data=12010002
usb-packet seq=8 t=11 dir=setup src=vendor-control len=8 captured=8 dropped=0 suppressed=0 reason=control-setup bm=0xC0 req=0x20 value=0x0102 index=0x0304 wlen=16384 data=C020020104030040
usb-packet seq=9 t=12 dir=out src=hid-output len=3 captured=3 truncated=0 dropped=0 suppressed=0 reason=host-out report_id=0x01 report_type=2 data=050607
usb-event t=13 event=suspend remote_wakeup=1
usb-event t=14 event=first-in-accepted src=xinput bytes=20
usb-event t=15 event=first-host-out src=vendor len=3
usb-packet-stats t=20 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 truncated_packets=1 idle_in_suppressed=9
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":3,\"expected_chunks\":3,\"missing_chunk_count\":0,\"duplicate_chunk_count\":1,\"got_last\":true,\"chunk_complete\":true,\"lost_bytes\":4,\"diag_bytes\":512,\"diag_lines\":20,\"packet_lines\":8,\"raw_packet_lines\":6,\"stats_lines\":2,\"event_lines\":3,\"new_lines\":2,\"duplicate_lines\":6,\"total_packet_lines\":12}
";
        let jsonl =
            records_jsonl_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text).unwrap();
        let records: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 8);
        assert_eq!(records[0]["kind"], "packet");
        assert_eq!(records[0]["source_label"], "02E22DA9");
        assert_eq!(records[0]["line_number"], 1);
        assert_eq!(records[0]["seq"], 7);
        assert_eq!(records[0]["direction"], "control-in");
        assert_eq!(records[0]["packet_truncated_bytes"], 0);
        assert_eq!(records[0]["control_payload_kind"], "usb_descriptor");
        assert_eq!(records[0]["control_descriptor_type"], "device");
        assert_eq!(
            records[0]["control_payload_summary"],
            "descriptor=device,captured_len=4"
        );
        assert_eq!(records[0]["data_hex"], "12010002");
        assert_eq!(records[1]["kind"], "packet");
        assert_eq!(records[1]["direction"], "setup");
        assert_eq!(records[1]["setup_bm_request_type"], 192);
        assert_eq!(records[1]["setup_request"], 32);
        assert_eq!(records[1]["setup_value"], 258);
        assert_eq!(records[1]["setup_index"], 772);
        assert_eq!(records[1]["setup_length"], 16384);
        assert_eq!(records[1]["setup_direction"], "device_to_host");
        assert_eq!(records[1]["setup_type"], "vendor");
        assert_eq!(records[1]["setup_recipient"], "device");
        assert_eq!(records[1]["setup_request_name"], "vendor_0x20");
        assert!(records[1]["setup_descriptor_type"].is_null());
        assert!(records[1]["setup_descriptor_index"].is_null());
        assert!(records[1]["setup_language_id"].is_null());
        assert!(records[1]["setup_known_request"].is_null());
        assert_eq!(records[1]["data_hex"], "C020020104030040");
        assert_eq!(records[2]["kind"], "packet");
        assert_eq!(records[2]["direction"], "out");
        assert_eq!(records[2]["hid_report_id"], 1);
        assert_eq!(records[2]["hid_report_type"], 2);
        assert_eq!(records[2]["hid_report_type_name"], "output");
        assert_eq!(records[3]["kind"], "event");
        assert_eq!(records[3]["t_ms"], 13);
        assert_eq!(records[3]["event"], "suspend");
        assert_eq!(records[3]["remote_wakeup"], 1);
        assert_eq!(records[4]["kind"], "event");
        assert_eq!(records[4]["event"], "first-in-accepted");
        assert_eq!(records[4]["source"], "xinput");
        assert!(records[4]["len"].is_null());
        assert_eq!(records[4]["bytes"], 20);
        assert_eq!(records[5]["kind"], "event");
        assert_eq!(records[5]["event"], "first-host-out");
        assert_eq!(records[5]["source"], "vendor");
        assert_eq!(records[5]["len"], 3);
        assert!(records[5]["bytes"].is_null());
        assert_eq!(records[6]["kind"], "stats");
        assert_eq!(records[6]["total"], 64);
        assert_eq!(records[6]["in"], 4);
        assert_eq!(records[6]["truncated_packets"], 1);
        assert_eq!(records[6]["idle_in_suppressed"], 9);
        assert_eq!(records[7]["kind"], "harvest");
        assert_eq!(records[7]["status"], "ok");
        assert_eq!(records[7]["duration_ms"], 14);
        assert_eq!(records[7]["chunk_count"], 3);
        assert_eq!(records[7]["expected_chunks"], 3);
        assert_eq!(records[7]["missing_chunk_count"], 0);
        assert_eq!(records[7]["duplicate_chunk_count"], 1);
        assert_eq!(records[7]["got_last"], true);
        assert_eq!(records[7]["chunk_complete"], true);
        assert_eq!(records[7]["lost_bytes"], 4);
        assert_eq!(records[7]["diag_bytes"], 512);
        assert_eq!(records[7]["diag_lines"], 20);
        assert_eq!(records[7]["packet_lines"], 8);
        assert_eq!(records[7]["raw_packet_lines"], 6);
        assert_eq!(records[7]["stats_lines"], 2);
        assert_eq!(records[7]["event_lines"], 3);
        assert_eq!(records[7]["new_lines"], 2);
        assert_eq!(records[7]["duplicate_lines"], 6);
        assert_eq!(records[7]["total_packet_lines"], 12);
    }
}
