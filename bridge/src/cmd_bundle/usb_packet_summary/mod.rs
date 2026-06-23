mod control_transfers;
mod decode;
mod enumeration;
mod hid_reports;
mod records;
mod summary;
mod support;
mod timeline;

use std::collections::BTreeMap;

use serde::Serialize;

pub(super) use control_transfers::{
    control_transfers_text_for_sources, control_transfers_text_for_text,
};
pub(super) use enumeration::{
    enumeration_analysis_text_for_sources, enumeration_analysis_text_for_text,
};
pub(super) use hid_reports::{hid_reports_text_for_sources, hid_reports_text_for_text};
pub(super) use records::{records_jsonl_for_sources, records_jsonl_for_text};
pub(super) use summary::{summarize_sources, summarize_text};
pub(super) use timeline::{packet_timeline_text_for_sources, packet_timeline_text_for_text};

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
#[cfg(test)]
mod tests;
