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
    pub directions: BTreeMap<String, u64>,
    pub sources: BTreeMap<String, u64>,
    pub reasons: BTreeMap<String, u64>,
    pub first_seq: Option<u64>,
    pub last_seq: Option<u64>,
    pub min_seq: Option<u64>,
    pub max_seq: Option<u64>,
    pub missing_sequence_numbers: u64,
    pub duplicate_sequence_numbers: u64,
    pub out_of_order_sequence_lines: u64,
    pub max_reported_packet_len: Option<u64>,
    pub max_captured_len: Option<u64>,
    pub max_truncated_bytes: Option<u64>,
    pub max_suppressed_idle_reports: Option<u64>,
    pub last_stats_total_packets: Option<u64>,
    pub max_stats_total_packets: Option<u64>,
    pub max_stats_truncated_bytes: Option<u64>,
    pub max_stats_idle_in_suppressed: Option<u64>,
    pub stats_direction_max: BTreeMap<String, u64>,
    pub harvest_lines: u64,
    pub harvest_statuses: BTreeMap<String, u64>,
    pub max_harvest_duration_ms: Option<u64>,
    pub max_harvest_lost_bytes: Option<u64>,
    pub max_harvest_chunk_count: Option<u64>,
    pub max_harvest_packet_lines: Option<u64>,
    pub max_harvest_new_lines: Option<u64>,
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
    Packet {
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
        truncated_bytes_total: Option<u64>,
        suppressed_idle_reports: Option<u64>,
        data_hex: Option<String>,
        raw_line: String,
    },
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
        idle_in_suppressed: Option<u64>,
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
        lost_bytes: Option<u64>,
        packet_lines: Option<u64>,
        new_lines: Option<u64>,
        total_packet_lines: Option<u64>,
        error: Option<String>,
        raw_line: String,
    },
}

pub(super) fn summarize_text(text: &str) -> UsbPacketSummary {
    let mut summary = UsbPacketSummary::default();
    let mut seen_sequences = BTreeSet::new();
    let mut previous_seq = None;
    for line in text.lines() {
        if line.starts_with("usb-packet ") {
            summary.add_packet_line(line, &mut seen_sequences, &mut previous_seq);
        } else if line.starts_with("usb-packet-stats ") {
            summary.add_stats_line(line);
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
        artifact_schema_version: 2,
        aggregate,
        per_pico,
        retained_logs,
        notes: vec![
            "Counts are derived from bundled usb-packet and usb-packet-stats lines.",
            "Aggregate sequence gaps are summed per source; sequence numbers are not compared across different Pico/log sources.",
            "Stats lines are checkpoint summaries emitted by debug input firmware and may survive even when raw packet lines have rotated out.",
            "Harvest lines describe each retained host GET_LOG attempt used to collect debug input packets.",
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
        return Some(UsbPacketRecord::Packet {
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
            truncated_bytes_total: parsed_u64(fields.get("dropped")),
            suppressed_idle_reports: parsed_u64(fields.get("suppressed")),
            data_hex: cloned_field(fields.get("data")),
            raw_line: line.to_string(),
        });
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
            idle_in_suppressed: parsed_u64(fields.get("idle_in_suppressed")),
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
            lost_bytes: value
                .as_ref()
                .and_then(|value| json_u64(value, "lost_bytes")),
            packet_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "packet_lines")),
            new_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "new_lines")),
            total_packet_lines: value
                .as_ref()
                .and_then(|value| json_u64(value, "total_packet_lines")),
            error: value.as_ref().and_then(|value| json_string(value, "error")),
            raw_line: line.to_string(),
        });
    }
    None
}

impl UsbPacketSummary {
    fn add_packet_line(
        &mut self,
        line: &str,
        seen_sequences: &mut BTreeSet<u64>,
        previous_seq: &mut Option<u64>,
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
        max_assign(
            &mut self.max_reported_packet_len,
            parsed_u64(fields.get("len")),
        );
        max_assign(
            &mut self.max_captured_len,
            parsed_u64(fields.get("captured")),
        );
        max_assign(
            &mut self.max_truncated_bytes,
            parsed_u64(fields.get("dropped")),
        );
        max_assign(
            &mut self.max_suppressed_idle_reports,
            parsed_u64(fields.get("suppressed")),
        );

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
            &mut self.max_harvest_packet_lines,
            json_u64(&value, "packet_lines"),
        );
        max_assign(
            &mut self.max_harvest_new_lines,
            json_u64(&value, "new_lines"),
        );
    }

    fn merge_from(&mut self, other: &UsbPacketSummary) {
        self.packet_lines += other.packet_lines;
        self.stats_lines += other.stats_lines;
        self.harvest_lines += other.harvest_lines;
        merge_counts(&mut self.directions, &other.directions);
        merge_counts(&mut self.sources, &other.sources);
        merge_counts(&mut self.reasons, &other.reasons);
        merge_counts(&mut self.harvest_statuses, &other.harvest_statuses);
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
            &mut self.max_stats_idle_in_suppressed,
            other.max_stats_idle_in_suppressed,
        );
        for (key, value) in &other.stats_direction_max {
            let entry = self.stats_direction_max.entry(key.clone()).or_default();
            if *value > *entry {
                *entry = *value;
            }
        }
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
            &mut self.max_harvest_packet_lines,
            other.max_harvest_packet_lines,
        );
        max_assign(&mut self.max_harvest_new_lines, other.max_harvest_new_lines);
    }
}

fn fields(line: &str) -> BTreeMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|part| part.split_once('='))
        .collect()
}

fn parsed_u64(value: Option<&&str>) -> Option<u64> {
    value.and_then(|value| value.parse::<u64>().ok())
}

fn cloned_field(value: Option<&&str>) -> Option<String> {
    value.map(|value| (*value).to_string())
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

fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.to_string()).or_default() += 1;
}

fn merge_counts(into: &mut BTreeMap<String, u64>, from: &BTreeMap<String, u64>) {
    for (key, value) in from {
        *into.entry(key.clone()).or_default() += value;
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
usb-packet seq=2 t=13 dir=setup src=vendor-control len=8 captured=8 dropped=4 suppressed=0 reason=control-setup data=C0
usb-packet-stats t=14 total=64 in=2 out=1 setup=1 control_in=0 truncated_bytes=4 idle_in_suppressed=9
";
        let summary = summarize_text(text);
        assert_eq!(summary.packet_lines, 4);
        assert_eq!(summary.stats_lines, 1);
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
        assert_eq!(summary.max_truncated_bytes, Some(4));
        assert_eq!(summary.max_suppressed_idle_reports, Some(2));
        assert_eq!(summary.last_stats_total_packets, Some(64));
        assert_eq!(summary.max_stats_idle_in_suppressed, Some(9));
        assert_eq!(summary.stats_direction_max["setup"], 1);
        assert_eq!(summary.harvest_lines, 0);
    }

    #[test]
    fn summary_counts_harvest_health() {
        let text = "\
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":3,\"lost_bytes\":4,\"packet_lines\":8,\"new_lines\":2,\"total_packet_lines\":12}
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
        assert_eq!(summary.max_harvest_packet_lines, Some(8));
        assert_eq!(summary.max_harvest_new_lines, Some(2));
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
            text: "usb-packet-stats total=64 out=2 truncated_bytes=0 idle_in_suppressed=0\n",
        }];
        let summary = summarize_sources(&per_pico, &retained);
        assert_eq!(summary.aggregate.packet_lines, 2);
        assert_eq!(summary.aggregate.stats_lines, 1);
        assert_eq!(summary.aggregate.missing_sequence_numbers, 1);
        assert_eq!(summary.per_pico[0].summary.missing_sequence_numbers, 1);
        assert_eq!(
            summary.retained_logs[0].summary.last_stats_total_packets,
            Some(64)
        );
    }

    #[test]
    fn records_jsonl_normalizes_packet_and_stats_lines() {
        let text = "\
usb-packet seq=7 t=10 dir=control-in src=desc-device len=18 captured=18 dropped=0 suppressed=0 reason=control-reply data=12010002
usb-packet-stats t=20 total=64 in=4 out=3 setup=2 control_in=1 truncated_bytes=8 idle_in_suppressed=9
# harvest {\"at\":\"2026-06-15T22:30:00-05:00\",\"status\":\"ok\",\"duration_ms\":14,\"chunk_count\":3,\"lost_bytes\":4,\"packet_lines\":8,\"new_lines\":2,\"total_packet_lines\":12}
";
        let jsonl =
            records_jsonl_for_text("02E22DA9", "picos/02E22DA9/usb-packets.txt", text).unwrap();
        let records: Vec<serde_json::Value> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0]["kind"], "packet");
        assert_eq!(records[0]["source_label"], "02E22DA9");
        assert_eq!(records[0]["line_number"], 1);
        assert_eq!(records[0]["seq"], 7);
        assert_eq!(records[0]["direction"], "control-in");
        assert_eq!(records[0]["data_hex"], "12010002");
        assert_eq!(records[1]["kind"], "stats");
        assert_eq!(records[1]["total"], 64);
        assert_eq!(records[1]["in"], 4);
        assert_eq!(records[1]["idle_in_suppressed"], 9);
        assert_eq!(records[2]["kind"], "harvest");
        assert_eq!(records[2]["status"], "ok");
        assert_eq!(records[2]["duration_ms"], 14);
        assert_eq!(records[2]["chunk_count"], 3);
        assert_eq!(records[2]["lost_bytes"], 4);
        assert_eq!(records[2]["packet_lines"], 8);
        assert_eq!(records[2]["new_lines"], 2);
        assert_eq!(records[2]["total_packet_lines"], 12);
    }
}
