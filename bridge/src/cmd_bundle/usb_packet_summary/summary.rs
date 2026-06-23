use std::collections::BTreeSet;

use super::{
    decode::{
        decode_control_payload_fields, decode_hid_report_metadata, decode_setup_fields, hex_u8,
    },
    support::{
        bump, fields, harvest_value, json_bool, json_string, json_u64, max_assign, merge_counts,
        min_assign, parsed_u64, HARVEST_PREFIX,
    },
    UsbPacketBundleSummary, UsbPacketNamedSummary, UsbPacketSummary, UsbPacketSummarySource,
};

pub(in crate::cmd_bundle) fn summarize_text(text: &str) -> UsbPacketSummary {
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

pub(in crate::cmd_bundle) fn summarize_sources(
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
