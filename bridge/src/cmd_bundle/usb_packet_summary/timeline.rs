use super::{
    support::{
        display_field, fields, harvest_value, json_bool, json_string, json_u64, parsed_u64,
        HARVEST_PREFIX,
    },
    UsbPacketSummarySource,
};

pub(in crate::cmd_bundle) fn packet_timeline_text_for_text(
    label: &str,
    path: &str,
    text: &str,
) -> String {
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

pub(in crate::cmd_bundle) fn packet_timeline_text_for_sources(
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
