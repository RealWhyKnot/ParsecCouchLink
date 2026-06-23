use serde::Serialize;

use super::{
    decode::{decode_control_payload_fields, decode_hid_report_metadata, decode_setup_fields},
    support::{
        cloned_field, fields, harvest_value, json_bool, json_string, json_u64, parsed_u64,
        HARVEST_PREFIX,
    },
    UsbPacketSummarySource,
};

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

pub(in crate::cmd_bundle) fn records_jsonl_for_text(
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

pub(in crate::cmd_bundle) fn records_jsonl_for_sources(
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
