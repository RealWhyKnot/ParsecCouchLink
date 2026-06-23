use super::{
    decode::{decode_hid_report_metadata, hex_u8},
    support::{display_field, fields},
    UsbPacketSummarySource,
};

pub(in crate::cmd_bundle) fn hid_reports_text_for_text(
    label: &str,
    path: &str,
    text: &str,
) -> String {
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

pub(in crate::cmd_bundle) fn hid_reports_text_for_sources(
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
