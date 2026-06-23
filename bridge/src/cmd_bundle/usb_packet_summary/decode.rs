use std::collections::BTreeMap;

use super::support::parsed_u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SetupDecode {
    pub(super) direction: &'static str,
    pub(super) request_type: &'static str,
    pub(super) recipient: &'static str,
    pub(super) request_name: String,
    pub(super) descriptor_type: Option<&'static str>,
    pub(super) descriptor_index: Option<u64>,
    pub(super) language_id: Option<u64>,
    pub(super) known_request: Option<&'static str>,
}

pub(super) fn decode_setup_fields(fields: &BTreeMap<&str, &str>) -> Option<SetupDecode> {
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

pub(super) fn setup_decode_text(fields: &BTreeMap<&str, &str>) -> String {
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
pub(super) struct HidReportMetadata {
    pub(super) report_id: Option<u64>,
    pub(super) report_type: Option<u64>,
    pub(super) report_type_name: Option<&'static str>,
    pub(super) request_name: Option<&'static str>,
}

pub(super) fn decode_hid_report_metadata(
    fields: &BTreeMap<&str, &str>,
) -> Option<HidReportMetadata> {
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

pub(super) fn hex_u16(value: u64) -> String {
    format!("0x{:04X}", value & 0xFFFF)
}

pub(super) fn hex_u8(value: u64) -> String {
    format!("0x{:02X}", value & 0xFF)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ControlPayloadDecode {
    pub(super) kind: &'static str,
    pub(super) descriptor_type: Option<&'static str>,
    pub(super) summary: String,
}

pub(super) fn decode_control_payload_fields(
    fields: &BTreeMap<&str, &str>,
) -> Option<ControlPayloadDecode> {
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

pub(super) fn control_payload_decode_text(fields: &BTreeMap<&str, &str>) -> String {
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

pub(super) fn hex_bytes(value: Option<&&str>) -> Option<Vec<u8>> {
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

pub(super) fn le_u16(bytes: &[u8], index: usize) -> u64 {
    u64::from(bytes[index]) | (u64::from(bytes[index + 1]) << 8)
}
