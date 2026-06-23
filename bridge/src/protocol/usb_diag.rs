use super::wire::{put_u16_le, put_u32_le, read_u16_le, read_u32_le};
use super::{
    UsbConfigurationState, UsbDiagDecodeError, MAGIC, TYPE_USB_DIAG, USB_DIAG_ACTIVITY_OUT,
    USB_DIAG_ACTIVITY_PARSEC, USB_DIAG_ACTIVITY_PEER, USB_DIAG_ACTIVITY_SENT,
    USB_DIAG_FLAG_MOUNTED, USB_DIAG_FLAG_SUSPENDED, USB_DIAG_V1_VERSION, USB_DIAG_V1_WIRE_SIZE,
    USB_DIAG_VERSION, USB_DIAG_WIRE_SIZE,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbDiag {
    pub seq: u8,
    pub flags: u8,
    pub version: u8,
    pub usb_flags: u8,
    pub activity_flags: u8,
    pub last_out_len: u8,
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
    pub last_out_byte0: u8,
    pub last_out_byte1: u8,
}

impl UsbDiag {
    pub fn mounted(&self) -> bool {
        self.usb_flags & USB_DIAG_FLAG_MOUNTED != 0
    }

    pub fn suspended(&self) -> bool {
        self.usb_flags & USB_DIAG_FLAG_SUSPENDED != 0
    }

    pub fn xinput_report_sent(&self) -> bool {
        self.activity_flags & USB_DIAG_ACTIVITY_SENT != 0
    }

    pub fn xinput_out_seen(&self) -> bool {
        self.activity_flags & USB_DIAG_ACTIVITY_OUT != 0
    }

    pub fn bridge_peer_latched(&self) -> bool {
        self.activity_flags & USB_DIAG_ACTIVITY_PEER != 0
    }

    pub fn parsec_connected(&self) -> bool {
        self.activity_flags & USB_DIAG_ACTIVITY_PARSEC != 0
    }

    pub fn in_blocked_total(&self) -> u32 {
        self.xinput_in_blocked_not_mounted_count
            .saturating_add(self.xinput_in_blocked_not_ready_count)
            .saturating_add(self.xinput_in_blocked_short_write_count)
    }

    pub fn configuration_state(&self) -> UsbConfigurationState {
        if self.mounted() {
            return UsbConfigurationState::Configured;
        }
        if self.suspended() {
            return UsbConfigurationState::Suspended;
        }
        if self.mount_count > 0 {
            if self.umount_count == 0 {
                return UsbConfigurationState::ConfiguredThenUnmountedWithoutCallback;
            }
            return UsbConfigurationState::ConfiguredThenUnmounted;
        }
        if self.device_desc_count > 0 || self.config_desc_count > 0 {
            UsbConfigurationState::EnumerationStarted
        } else {
            UsbConfigurationState::NoHostTraffic
        }
    }

    pub fn age_ms(&self, timestamp_ms: u32) -> Option<u32> {
        if timestamp_ms == 0 {
            None
        } else {
            Some(self.now_ms.wrapping_sub(timestamp_ms))
        }
    }

    pub fn encode(&self) -> [u8; USB_DIAG_WIRE_SIZE] {
        let mut buf = [0u8; USB_DIAG_WIRE_SIZE];
        buf[0] = MAGIC;
        buf[1] = TYPE_USB_DIAG;
        buf[2] = self.seq;
        buf[3] = self.flags;
        buf[4] = self.version;
        buf[5] = self.usb_flags;
        buf[6] = self.activity_flags;
        buf[7] = self.last_out_len;
        put_u32_le(&mut buf, 8, self.now_ms);
        put_u32_le(&mut buf, 12, self.last_bridge_packet_ms);
        put_u32_le(&mut buf, 16, self.mount_count);
        put_u32_le(&mut buf, 20, self.umount_count);
        put_u32_le(&mut buf, 24, self.suspend_count);
        put_u32_le(&mut buf, 28, self.resume_count);
        put_u32_le(&mut buf, 32, self.device_desc_count);
        put_u32_le(&mut buf, 36, self.config_desc_count);
        put_u32_le(&mut buf, 40, self.xinput_in_queued_count);
        put_u32_le(&mut buf, 44, self.xinput_in_sent_count);
        put_u32_le(&mut buf, 48, self.xinput_out_count);
        put_u32_le(&mut buf, 52, self.last_mount_ms);
        put_u32_le(&mut buf, 56, self.last_umount_ms);
        put_u32_le(&mut buf, 60, self.last_in_queued_ms);
        put_u32_le(&mut buf, 64, self.last_in_sent_ms);
        put_u32_le(&mut buf, 68, self.last_out_ms);
        buf[72] = self.last_out_byte0;
        buf[73] = self.last_out_byte1;
        put_u32_le(&mut buf, 74, self.xinput_in_blocked_not_mounted_count);
        put_u32_le(&mut buf, 78, self.xinput_in_blocked_not_ready_count);
        put_u32_le(&mut buf, 82, self.xinput_in_blocked_short_write_count);
        put_u32_le(&mut buf, 86, self.xinput_in_idle_suppressed_count);
        put_u32_le(&mut buf, 90, self.last_in_blocked_ms);
        buf[94] = self.last_in_blocked_reason;
        put_u16_le(&mut buf, 96, self.last_in_blocked_want);
        put_u16_le(&mut buf, 98, self.last_in_blocked_got);
        let crc =
            crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..USB_DIAG_WIRE_SIZE - 2]);
        buf[USB_DIAG_WIRE_SIZE - 2] = (crc & 0xFF) as u8;
        buf[USB_DIAG_WIRE_SIZE - 1] = (crc >> 8) as u8;
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, UsbDiagDecodeError> {
        if buf.len() != USB_DIAG_WIRE_SIZE && buf.len() != USB_DIAG_V1_WIRE_SIZE {
            return Err(UsbDiagDecodeError::WrongSize {
                got: buf.len(),
                want: USB_DIAG_WIRE_SIZE,
            });
        }
        if buf[0] != MAGIC {
            return Err(UsbDiagDecodeError::WrongMagic);
        }
        if buf[1] != TYPE_USB_DIAG {
            return Err(UsbDiagDecodeError::WrongType(buf[1]));
        }
        let crc_offset = buf.len() - 2;
        let crc_lo = buf[crc_offset] as u16;
        let crc_hi = buf[crc_offset + 1] as u16;
        let crc_got = crc_lo | (crc_hi << 8);
        let crc_want = crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..crc_offset]);
        if crc_got != crc_want {
            return Err(UsbDiagDecodeError::BadCrc {
                got: crc_got,
                want: crc_want,
            });
        }
        let version = buf[4];
        if (buf.len() == USB_DIAG_V1_WIRE_SIZE && version != USB_DIAG_V1_VERSION)
            || (buf.len() == USB_DIAG_WIRE_SIZE && version != USB_DIAG_VERSION)
        {
            return Err(UsbDiagDecodeError::UnsupportedVersion(buf[4]));
        }
        Ok(UsbDiag {
            seq: buf[2],
            flags: buf[3],
            version: buf[4],
            usb_flags: buf[5],
            activity_flags: buf[6],
            last_out_len: buf[7],
            now_ms: read_u32_le(buf, 8),
            last_bridge_packet_ms: read_u32_le(buf, 12),
            mount_count: read_u32_le(buf, 16),
            umount_count: read_u32_le(buf, 20),
            suspend_count: read_u32_le(buf, 24),
            resume_count: read_u32_le(buf, 28),
            device_desc_count: read_u32_le(buf, 32),
            config_desc_count: read_u32_le(buf, 36),
            xinput_in_queued_count: read_u32_le(buf, 40),
            xinput_in_sent_count: read_u32_le(buf, 44),
            xinput_out_count: read_u32_le(buf, 48),
            xinput_in_blocked_not_mounted_count: if version >= USB_DIAG_VERSION {
                read_u32_le(buf, 74)
            } else {
                0
            },
            xinput_in_blocked_not_ready_count: if version >= USB_DIAG_VERSION {
                read_u32_le(buf, 78)
            } else {
                0
            },
            xinput_in_blocked_short_write_count: if version >= USB_DIAG_VERSION {
                read_u32_le(buf, 82)
            } else {
                0
            },
            xinput_in_idle_suppressed_count: if version >= USB_DIAG_VERSION {
                read_u32_le(buf, 86)
            } else {
                0
            },
            last_mount_ms: read_u32_le(buf, 52),
            last_umount_ms: read_u32_le(buf, 56),
            last_in_queued_ms: read_u32_le(buf, 60),
            last_in_sent_ms: read_u32_le(buf, 64),
            last_out_ms: read_u32_le(buf, 68),
            last_in_blocked_ms: if version >= USB_DIAG_VERSION {
                read_u32_le(buf, 90)
            } else {
                0
            },
            last_in_blocked_reason: if version >= USB_DIAG_VERSION {
                buf[94]
            } else {
                0
            },
            last_in_blocked_want: if version >= USB_DIAG_VERSION {
                read_u16_le(buf, 96)
            } else {
                0
            },
            last_in_blocked_got: if version >= USB_DIAG_VERSION {
                read_u16_le(buf, 98)
            } else {
                0
            },
            last_out_byte0: buf[72],
            last_out_byte1: buf[73],
        })
    }
}
