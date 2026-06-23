use std::collections::BTreeMap;

use super::{
    decode::{
        decode_control_payload_fields, decode_hid_report_metadata, decode_setup_fields, hex_bytes,
        hex_u16, le_u16,
    },
    support::{bump, fields, format_count_map, HARVEST_PREFIX},
    UsbPacketSummarySource,
};

pub(in crate::cmd_bundle) fn enumeration_analysis_text_for_text(
    label: &str,
    path: &str,
    text: &str,
) -> String {
    let mut out = String::from("# USB enumeration analysis\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_label={label}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("# source_path={path}\n\n"));
    let analysis = analyze_enumeration(text);
    write_enumeration_analysis(&mut out, &analysis);
    out
}

pub(in crate::cmd_bundle) fn enumeration_analysis_text_for_sources(
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

pub(super) fn known_device_identity(
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
        (0x2E8A, 0xCAF2, 0x00, 0x00, 0x00) => "couchlink_generic_hid_gamepad_shape",
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
