use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

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

pub(super) fn summarize_text(text: &str) -> UsbPacketSummary {
    let mut summary = UsbPacketSummary::default();
    let mut seen_sequences = BTreeSet::new();
    let mut previous_seq = None;
    for line in text.lines() {
        if line.starts_with("usb-packet ") {
            summary.add_packet_line(line, &mut seen_sequences, &mut previous_seq);
        } else if line.starts_with("usb-packet-stats ") {
            summary.add_stats_line(line);
        }
    }
    summary
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
        artifact_schema_version: 1,
        aggregate,
        per_pico,
        retained_logs,
        notes: vec![
            "Counts are derived from bundled usb-packet and usb-packet-stats lines.",
            "Aggregate sequence gaps are summed per source; sequence numbers are not compared across different Pico/log sources.",
            "Stats lines are checkpoint summaries emitted by debug input firmware and may survive even when raw packet lines have rotated out.",
        ],
    }
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

    fn merge_from(&mut self, other: &UsbPacketSummary) {
        self.packet_lines += other.packet_lines;
        self.stats_lines += other.stats_lines;
        merge_counts(&mut self.directions, &other.directions);
        merge_counts(&mut self.sources, &other.sources);
        merge_counts(&mut self.reasons, &other.reasons);
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
}
