use super::wire::{put_u16_le, put_u32_le, read_u16_le, read_u32_le};
use super::{
    usb_in_blocked_reason_label, Persona, PicoStateDecodeError, BT_HID_STATUS_CONNECTED,
    BT_HID_STATUS_SEND_REQUESTED, BT_HID_STATUS_STARTED, MAGIC, PICO_STATE_V1_VERSION,
    PICO_STATE_V1_WIRE_SIZE, PICO_STATE_V2_VERSION, PICO_STATE_V2_WIRE_SIZE, PICO_STATE_VERSION,
    PICO_STATE_WIRE_SIZE, TYPE_PICO_STATE,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PicoStateDiag {
    pub seq: u8,
    pub flags: u8,
    pub version: u8,
    pub proto_version: u8,
    pub board_type: u8,
    pub persona_byte: u8,
    pub unique_id_short: u32,
    pub uptime_seconds: u32,
    pub tx_count: u32,
    pub rx_count: u32,
    pub now_ms: u32,
    pub last_bridge_packet_ms: u32,
    pub mount_count: u32,
    pub umount_count: u32,
    pub suspend_count: u32,
    pub resume_count: u32,
    pub device_desc_count: u32,
    pub config_desc_count: u32,
    pub xinput_in_queued_count: u32,
    pub xinput_in_sent_count: u32,
    pub xinput_out_count: u32,
    pub xinput_in_blocked_not_mounted_count: u32,
    pub xinput_in_blocked_not_ready_count: u32,
    pub xinput_in_blocked_short_write_count: u32,
    pub xinput_in_idle_suppressed_count: u32,
    pub last_mount_ms: u32,
    pub last_umount_ms: u32,
    pub last_in_queued_ms: u32,
    pub last_in_sent_ms: u32,
    pub last_out_ms: u32,
    pub last_in_blocked_ms: u32,
    pub last_in_blocked_reason: u8,
    pub last_in_blocked_want: u16,
    pub last_in_blocked_got: u16,
    pub last_out_len: u8,
    pub last_out_byte0: u8,
    pub last_out_byte1: u8,
    pub usb_flags: u8,
    pub activity_flags: u8,
    pub malformed_udp_count: u32,
    pub bt_flags: u8,
    pub bt_target: u8,
    pub bt_last_status: u8,
    pub bt_report_len: u8,
    pub bt_cid: u16,
    pub bt_init_count: u32,
    pub bt_ready_count: u32,
    pub bt_open_count: u32,
    pub bt_close_count: u32,
    pub bt_can_send_count: u32,
    pub bt_report_build_count: u32,
    pub bt_report_send_count: u32,
    pub bt_send_request_count: u32,
    pub bt_last_event_ms: u32,
    pub bt_last_send_ms: u32,
}

impl PicoStateDiag {
    pub fn encode(&self) -> [u8; PICO_STATE_WIRE_SIZE] {
        let mut buf = [0u8; PICO_STATE_WIRE_SIZE];
        buf[0] = MAGIC;
        buf[1] = TYPE_PICO_STATE;
        buf[2] = self.seq;
        buf[3] = self.flags;
        buf[4] = self.version;
        buf[5] = self.proto_version;
        buf[6] = self.board_type;
        buf[7] = self.persona_byte;
        put_u32_le(&mut buf, 8, self.unique_id_short);
        put_u32_le(&mut buf, 12, self.uptime_seconds);
        put_u32_le(&mut buf, 16, self.tx_count);
        put_u32_le(&mut buf, 20, self.rx_count);
        put_u32_le(&mut buf, 24, self.now_ms);
        put_u32_le(&mut buf, 28, self.last_bridge_packet_ms);
        put_u32_le(&mut buf, 32, self.mount_count);
        put_u32_le(&mut buf, 36, self.umount_count);
        put_u32_le(&mut buf, 40, self.suspend_count);
        put_u32_le(&mut buf, 44, self.resume_count);
        put_u32_le(&mut buf, 48, self.device_desc_count);
        put_u32_le(&mut buf, 52, self.config_desc_count);
        put_u32_le(&mut buf, 56, self.xinput_in_queued_count);
        put_u32_le(&mut buf, 60, self.xinput_in_sent_count);
        put_u32_le(&mut buf, 64, self.xinput_out_count);
        put_u32_le(&mut buf, 68, self.last_mount_ms);
        put_u32_le(&mut buf, 72, self.last_umount_ms);
        put_u32_le(&mut buf, 76, self.last_in_queued_ms);
        put_u32_le(&mut buf, 80, self.last_in_sent_ms);
        put_u32_le(&mut buf, 84, self.last_out_ms);
        buf[88] = self.last_out_len;
        buf[89] = self.last_out_byte0;
        buf[90] = self.last_out_byte1;
        buf[91] = self.usb_flags;
        buf[92] = self.activity_flags;
        put_u32_le(&mut buf, 96, self.malformed_udp_count);
        put_u32_le(&mut buf, 100, self.xinput_in_blocked_not_mounted_count);
        put_u32_le(&mut buf, 104, self.xinput_in_blocked_not_ready_count);
        put_u32_le(&mut buf, 108, self.xinput_in_blocked_short_write_count);
        put_u32_le(&mut buf, 112, self.xinput_in_idle_suppressed_count);
        put_u32_le(&mut buf, 116, self.last_in_blocked_ms);
        buf[120] = self.last_in_blocked_reason;
        put_u16_le(&mut buf, 122, self.last_in_blocked_want);
        put_u16_le(&mut buf, 124, self.last_in_blocked_got);
        buf[126] = self.bt_flags;
        buf[127] = self.bt_target;
        buf[128] = self.bt_last_status;
        buf[129] = self.bt_report_len;
        put_u16_le(&mut buf, 130, self.bt_cid);
        put_u32_le(&mut buf, 132, self.bt_init_count);
        put_u32_le(&mut buf, 136, self.bt_ready_count);
        put_u32_le(&mut buf, 140, self.bt_open_count);
        put_u32_le(&mut buf, 144, self.bt_close_count);
        put_u32_le(&mut buf, 148, self.bt_can_send_count);
        put_u32_le(&mut buf, 152, self.bt_report_build_count);
        put_u32_le(&mut buf, 156, self.bt_report_send_count);
        put_u32_le(&mut buf, 160, self.bt_send_request_count);
        put_u32_le(&mut buf, 164, self.bt_last_event_ms);
        put_u32_le(&mut buf, 168, self.bt_last_send_ms);
        let crc =
            crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..PICO_STATE_WIRE_SIZE - 2]);
        buf[PICO_STATE_WIRE_SIZE - 2] = (crc & 0xFF) as u8;
        buf[PICO_STATE_WIRE_SIZE - 1] = (crc >> 8) as u8;
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PicoStateDecodeError> {
        if buf.len() != PICO_STATE_WIRE_SIZE
            && buf.len() != PICO_STATE_V2_WIRE_SIZE
            && buf.len() != PICO_STATE_V1_WIRE_SIZE
        {
            return Err(PicoStateDecodeError::WrongSize {
                got: buf.len(),
                want: PICO_STATE_WIRE_SIZE,
            });
        }
        if buf[0] != MAGIC {
            return Err(PicoStateDecodeError::WrongMagic);
        }
        if buf[1] != TYPE_PICO_STATE {
            return Err(PicoStateDecodeError::WrongType(buf[1]));
        }
        let crc_offset = buf.len() - 2;
        let crc_lo = buf[crc_offset] as u16;
        let crc_hi = buf[crc_offset + 1] as u16;
        let crc_got = crc_lo | (crc_hi << 8);
        let crc_want = crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..crc_offset]);
        if crc_got != crc_want {
            return Err(PicoStateDecodeError::BadCrc {
                got: crc_got,
                want: crc_want,
            });
        }
        let version = buf[4];
        if (buf.len() == PICO_STATE_V1_WIRE_SIZE && version != PICO_STATE_V1_VERSION)
            || (buf.len() == PICO_STATE_V2_WIRE_SIZE && version != PICO_STATE_V2_VERSION)
            || (buf.len() == PICO_STATE_WIRE_SIZE && version != PICO_STATE_VERSION)
        {
            return Err(PicoStateDecodeError::UnsupportedVersion(buf[4]));
        }
        Ok(Self {
            seq: buf[2],
            flags: buf[3],
            version: buf[4],
            proto_version: buf[5],
            board_type: buf[6],
            persona_byte: buf[7],
            unique_id_short: read_u32_le(buf, 8),
            uptime_seconds: read_u32_le(buf, 12),
            tx_count: read_u32_le(buf, 16),
            rx_count: read_u32_le(buf, 20),
            now_ms: read_u32_le(buf, 24),
            last_bridge_packet_ms: read_u32_le(buf, 28),
            mount_count: read_u32_le(buf, 32),
            umount_count: read_u32_le(buf, 36),
            suspend_count: read_u32_le(buf, 40),
            resume_count: read_u32_le(buf, 44),
            device_desc_count: read_u32_le(buf, 48),
            config_desc_count: read_u32_le(buf, 52),
            xinput_in_queued_count: read_u32_le(buf, 56),
            xinput_in_sent_count: read_u32_le(buf, 60),
            xinput_out_count: read_u32_le(buf, 64),
            xinput_in_blocked_not_mounted_count: if version >= PICO_STATE_V2_VERSION {
                read_u32_le(buf, 100)
            } else {
                0
            },
            xinput_in_blocked_not_ready_count: if version >= PICO_STATE_V2_VERSION {
                read_u32_le(buf, 104)
            } else {
                0
            },
            xinput_in_blocked_short_write_count: if version >= PICO_STATE_V2_VERSION {
                read_u32_le(buf, 108)
            } else {
                0
            },
            xinput_in_idle_suppressed_count: if version >= PICO_STATE_V2_VERSION {
                read_u32_le(buf, 112)
            } else {
                0
            },
            last_mount_ms: read_u32_le(buf, 68),
            last_umount_ms: read_u32_le(buf, 72),
            last_in_queued_ms: read_u32_le(buf, 76),
            last_in_sent_ms: read_u32_le(buf, 80),
            last_out_ms: read_u32_le(buf, 84),
            last_in_blocked_ms: if version >= PICO_STATE_V2_VERSION {
                read_u32_le(buf, 116)
            } else {
                0
            },
            last_in_blocked_reason: if version >= PICO_STATE_V2_VERSION {
                buf[120]
            } else {
                0
            },
            last_in_blocked_want: if version >= PICO_STATE_V2_VERSION {
                read_u16_le(buf, 122)
            } else {
                0
            },
            last_in_blocked_got: if version >= PICO_STATE_V2_VERSION {
                read_u16_le(buf, 124)
            } else {
                0
            },
            last_out_len: buf[88],
            last_out_byte0: buf[89],
            last_out_byte1: buf[90],
            usb_flags: buf[91],
            activity_flags: buf[92],
            malformed_udp_count: read_u32_le(buf, 96),
            bt_flags: if version >= PICO_STATE_VERSION {
                buf[126]
            } else {
                0
            },
            bt_target: if version >= PICO_STATE_VERSION {
                buf[127]
            } else {
                0
            },
            bt_last_status: if version >= PICO_STATE_VERSION {
                buf[128]
            } else {
                0
            },
            bt_report_len: if version >= PICO_STATE_VERSION {
                buf[129]
            } else {
                0
            },
            bt_cid: if version >= PICO_STATE_VERSION {
                read_u16_le(buf, 130)
            } else {
                0
            },
            bt_init_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 132)
            } else {
                0
            },
            bt_ready_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 136)
            } else {
                0
            },
            bt_open_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 140)
            } else {
                0
            },
            bt_close_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 144)
            } else {
                0
            },
            bt_can_send_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 148)
            } else {
                0
            },
            bt_report_build_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 152)
            } else {
                0
            },
            bt_report_send_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 156)
            } else {
                0
            },
            bt_send_request_count: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 160)
            } else {
                0
            },
            bt_last_event_ms: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 164)
            } else {
                0
            },
            bt_last_send_ms: if version >= PICO_STATE_VERSION {
                read_u32_le(buf, 168)
            } else {
                0
            },
        })
    }

    pub fn persona(&self) -> Option<Persona> {
        match self.persona_byte {
            0 => Some(Persona::Xinput),
            1 => Some(Persona::Keyboard),
            2 => Some(Persona::Maple),
            3 => Some(Persona::Ps3),
            4 => Some(Persona::Ps4),
            5 => Some(Persona::XboxOne),
            6 => Some(Persona::Debug),
            7 => Some(Persona::GenericHid),
            8 => Some(Persona::BluetoothHid),
            9 => Some(Persona::BluetoothXbox),
            10 => Some(Persona::BluetoothPlaystation),
            _ => None,
        }
    }

    pub fn bt_target_label(&self) -> &'static str {
        bt_hid_target_label(self.bt_target)
    }

    pub fn to_json_map(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        let mut out = std::collections::BTreeMap::new();
        out.insert("version".into(), serde_json::json!(self.version));
        out.insert(
            "proto_version".into(),
            serde_json::json!(self.proto_version),
        );
        out.insert("board_type".into(), serde_json::json!(self.board_type));
        out.insert("persona_byte".into(), serde_json::json!(self.persona_byte));
        out.insert(
            "persona".into(),
            serde_json::json!(self.persona().map(|p| p.label())),
        );
        out.insert(
            "unique_id_short".into(),
            serde_json::json!(format!("{:08X}", self.unique_id_short)),
        );
        out.insert(
            "uptime_seconds".into(),
            serde_json::json!(self.uptime_seconds),
        );
        out.insert("tx_count".into(), serde_json::json!(self.tx_count));
        out.insert("rx_count".into(), serde_json::json!(self.rx_count));
        out.insert(
            "malformed_udp_count".into(),
            serde_json::json!(self.malformed_udp_count),
        );
        out.insert("now_ms".into(), serde_json::json!(self.now_ms));
        out.insert(
            "last_bridge_packet_ms".into(),
            serde_json::json!(self.last_bridge_packet_ms),
        );
        out.insert("usb_flags".into(), serde_json::json!(self.usb_flags));
        out.insert(
            "activity_flags".into(),
            serde_json::json!(self.activity_flags),
        );
        out.insert("mount_count".into(), serde_json::json!(self.mount_count));
        out.insert("umount_count".into(), serde_json::json!(self.umount_count));
        out.insert(
            "device_desc_count".into(),
            serde_json::json!(self.device_desc_count),
        );
        out.insert(
            "config_desc_count".into(),
            serde_json::json!(self.config_desc_count),
        );
        out.insert(
            "host_accepted_reports".into(),
            serde_json::json!(self.xinput_in_sent_count),
        );
        out.insert(
            "host_out_reports".into(),
            serde_json::json!(self.xinput_out_count),
        );
        out.insert(
            "in_blocked_not_mounted".into(),
            serde_json::json!(self.xinput_in_blocked_not_mounted_count),
        );
        out.insert(
            "in_blocked_not_ready".into(),
            serde_json::json!(self.xinput_in_blocked_not_ready_count),
        );
        out.insert(
            "in_blocked_short_write".into(),
            serde_json::json!(self.xinput_in_blocked_short_write_count),
        );
        out.insert(
            "in_idle_suppressed".into(),
            serde_json::json!(self.xinput_in_idle_suppressed_count),
        );
        out.insert(
            "last_in_blocked_ms".into(),
            serde_json::json!(self.last_in_blocked_ms),
        );
        out.insert(
            "last_in_blocked_reason".into(),
            serde_json::json!(usb_in_blocked_reason_label(self.last_in_blocked_reason)),
        );
        out.insert(
            "last_in_blocked_want".into(),
            serde_json::json!(self.last_in_blocked_want),
        );
        out.insert(
            "last_in_blocked_got".into(),
            serde_json::json!(self.last_in_blocked_got),
        );
        out.insert("bt_flags".into(), serde_json::json!(self.bt_flags));
        out.insert("bt_target".into(), serde_json::json!(self.bt_target));
        out.insert(
            "bt_target_label".into(),
            serde_json::json!(bt_hid_target_label(self.bt_target)),
        );
        out.insert(
            "bt_started".into(),
            serde_json::json!(self.bt_flags & BT_HID_STATUS_STARTED != 0),
        );
        out.insert(
            "bt_connected".into(),
            serde_json::json!(self.bt_flags & BT_HID_STATUS_CONNECTED != 0),
        );
        out.insert(
            "bt_send_requested".into(),
            serde_json::json!(self.bt_flags & BT_HID_STATUS_SEND_REQUESTED != 0),
        );
        out.insert(
            "bt_last_status".into(),
            serde_json::json!(self.bt_last_status),
        );
        out.insert(
            "bt_report_len".into(),
            serde_json::json!(self.bt_report_len),
        );
        out.insert("bt_cid".into(), serde_json::json!(self.bt_cid));
        out.insert(
            "bt_init_count".into(),
            serde_json::json!(self.bt_init_count),
        );
        out.insert(
            "bt_ready_count".into(),
            serde_json::json!(self.bt_ready_count),
        );
        out.insert(
            "bt_open_count".into(),
            serde_json::json!(self.bt_open_count),
        );
        out.insert(
            "bt_close_count".into(),
            serde_json::json!(self.bt_close_count),
        );
        out.insert(
            "bt_can_send_count".into(),
            serde_json::json!(self.bt_can_send_count),
        );
        out.insert(
            "bt_report_build_count".into(),
            serde_json::json!(self.bt_report_build_count),
        );
        out.insert(
            "bt_report_send_count".into(),
            serde_json::json!(self.bt_report_send_count),
        );
        out.insert(
            "bt_send_request_count".into(),
            serde_json::json!(self.bt_send_request_count),
        );
        out.insert(
            "bt_last_event_ms".into(),
            serde_json::json!(self.bt_last_event_ms),
        );
        out.insert(
            "bt_last_send_ms".into(),
            serde_json::json!(self.bt_last_send_ms),
        );
        out
    }
}

pub fn bt_hid_target_label(target: u8) -> &'static str {
    match target {
        1 => "bluetooth-xbox",
        2 => "bluetooth-playstation",
        _ => "bluetooth",
    }
}
