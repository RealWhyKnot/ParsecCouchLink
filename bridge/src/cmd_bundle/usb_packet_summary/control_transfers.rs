use super::{
    decode::{control_payload_decode_text, setup_decode_text},
    support::{display_field, fields},
    UsbPacketSummarySource,
};

pub(in crate::cmd_bundle) fn control_transfers_text_for_text(
    label: &str,
    path: &str,
    text: &str,
) -> String {
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

pub(in crate::cmd_bundle) fn control_transfers_text_for_sources(
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
