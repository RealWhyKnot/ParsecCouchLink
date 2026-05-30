//! Wire protocol per wiki/Protocol.md. UDP port 4242, 17-byte fixed datagrams,
//! little-endian, CRC-8/SMBUS over the first 16 bytes.
//!
//! Constants and helpers in this module mirror the protocol spec
//! one-for-one; some aren't directly consumed by the Windows bridge but
//! are exported so external tooling (the throwaway listener, future Pico
//! firmware-side bindings, support-bundle annotations) can reference the
//! same names. `dead_code` is suppressed for that reason.

#![allow(dead_code)]

use crate::firmware_version::FirmwareVersion;

pub const PORT: u16 = 4242;
pub const PACKET_SIZE: usize = 17;
pub const MAGIC: u8 = 0xA5;

pub const TYPE_STATE: u8 = 0x01;
pub const TYPE_HEARTBEAT: u8 = 0x02;
pub const TYPE_DISCOVER: u8 = 0x03;
pub const TYPE_ACK: u8 = 0x04;
/// Request for the firmware's diag-log ring buffer over UDP. Carried in
/// the same 17-byte fixed-shape datagram as the streaming types so the
/// firmware's existing on_recv() RX path accepts it. The reply is a
/// sequence of variable-length `TYPE_LOG_CHUNK` datagrams.
pub const TYPE_GET_LOG: u8 = 0x05;
/// Request for the firmware's current USB/XInput status over UDP.
pub const TYPE_GET_USB_DIAG: u8 = 0x06;
/// Request for run-mode firmware to reboot into setup-mode USB-CDC.
pub const TYPE_REBOOT_TO_SETUP: u8 = 0x07;
/// Chunk of diag-log payload sent by the firmware in reply to
/// `TYPE_GET_LOG`. Variable-length (12-byte header + up to 256 bytes of
/// payload + 2 bytes CRC-16). High bit set, matching the CDC convention
/// for response opcodes.
pub const TYPE_LOG_CHUNK: u8 = 0x85;
/// USB/XInput status reply sent by run-mode firmware.
pub const TYPE_USB_DIAG: u8 = 0x86;

pub const FLAG_PARSEC_CONNECTED: u8 = 1 << 0;
pub const FLAG_NEUTRALIZE: u8 = 1 << 1;

/// Set in the ACK packet's `flags` byte (which was always zero before)
/// when the firmware supports the `TYPE_GET_LOG` / `TYPE_LOG_CHUNK`
/// exchange. The wire-protocol version stays at 1 so old bridges
/// continue to interoperate; new bridges gate the diag pull on this
/// bit.
pub const ACK_FLAG_LOG_CHUNK_SUPPORTED: u8 = 1 << 0;
/// Set in the ACK flags byte when the firmware supports
/// `TYPE_GET_USB_DIAG` / `TYPE_USB_DIAG`.
pub const ACK_FLAG_USB_DIAG_SUPPORTED: u8 = 1 << 1;
/// Set in the ACK flags byte when the firmware supports
/// `TYPE_REBOOT_TO_SETUP`.
pub const ACK_FLAG_REBOOT_TO_SETUP_SUPPORTED: u8 = 1 << 2;

pub const USB_DIAG_WIRE_SIZE: usize = 78;
pub const USB_DIAG_VERSION: u8 = 1;
pub const USB_DIAG_FLAG_MOUNTED: u8 = 1 << 0;
pub const USB_DIAG_FLAG_SUSPENDED: u8 = 1 << 1;
pub const USB_DIAG_ACTIVITY_QUEUED: u8 = 1 << 0;
pub const USB_DIAG_ACTIVITY_SENT: u8 = 1 << 1;
pub const USB_DIAG_ACTIVITY_OUT: u8 = 1 << 2;
pub const USB_DIAG_ACTIVITY_PEER: u8 = 1 << 3;
pub const USB_DIAG_ACTIVITY_PARSEC: u8 = 1 << 4;

/// Set in a `TYPE_LOG_CHUNK` datagram's `flags` byte to mark the final
/// chunk in the reply sequence.
pub const LOG_CHUNK_FLAG_LAST: u8 = 1 << 0;

/// Maximum log-chunk payload size in bytes. With a 4 KiB diag ring on
/// the firmware, a complete snapshot fits in 16 chunks. Comfortably
/// below the Wi-Fi MTU after IP+UDP headers.
pub const LOG_CHUNK_MAX_PAYLOAD: usize = 256;

/// Header length for a `TYPE_LOG_CHUNK` datagram, excluding the 2-byte
/// CRC-16 trailer. Total on-wire size is `LOG_CHUNK_HEADER_LEN + payload_len + 2`.
pub const LOG_CHUNK_HEADER_LEN: usize = 12;

/// Current wire-protocol version for the streaming/discovery path. Stays
/// at 1 across optional diagnostics; new behaviour is gated on ACK
/// capability bits instead.
pub const PROTO_VERSION: u8 = 1;

pub const BOARD_PICO_2_W: u8 = 0x01;
pub const BOARD_PICO_W_RP2040: u8 = 0x02;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AckInfo {
    pub proto_version: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub board_type: u8,
    pub uptime_seconds: u32, // wire is u24 LE; high byte must be zero
    pub unique_id_short: u32,
}

impl AckInfo {
    pub fn firmware_version(&self) -> FirmwareVersion {
        FirmwareVersion::from_triplet(self.fw_major, self.fw_minor, self.fw_patch)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    State(GamepadState),
    Heartbeat(GamepadState),
    Discover,
    Ack(AckInfo),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet {
    pub kind: PacketKind,
    pub seq: u8,
    pub flags: u8,
}

impl Packet {
    pub fn state(seq: u8, flags: u8, state: GamepadState) -> Self {
        Self {
            kind: PacketKind::State(state),
            seq,
            flags,
        }
    }

    pub fn heartbeat(seq: u8, flags: u8, state: GamepadState) -> Self {
        Self {
            kind: PacketKind::Heartbeat(state),
            seq,
            flags,
        }
    }

    pub fn discover(seq: u8) -> Self {
        Self {
            kind: PacketKind::Discover,
            seq,
            flags: 0,
        }
    }

    pub fn ack(seq: u8, info: AckInfo) -> Self {
        Self {
            kind: PacketKind::Ack(info),
            seq,
            flags: 0,
        }
    }

    pub fn encode(&self) -> [u8; PACKET_SIZE] {
        let mut buf = [0u8; PACKET_SIZE];
        buf[0] = MAGIC;
        buf[1] = match self.kind {
            PacketKind::State(_) => TYPE_STATE,
            PacketKind::Heartbeat(_) => TYPE_HEARTBEAT,
            PacketKind::Discover => TYPE_DISCOVER,
            PacketKind::Ack(_) => TYPE_ACK,
        };
        buf[2] = self.seq;
        buf[3] = self.flags;
        let body: &mut [u8; 12] = (&mut buf[4..16]).try_into().unwrap();
        match self.kind {
            PacketKind::State(st) | PacketKind::Heartbeat(st) => {
                write_state(body, &st);
            }
            PacketKind::Discover => {
                // body stays zero
            }
            PacketKind::Ack(info) => {
                body[0] = info.proto_version;
                body[1] = info.fw_major;
                body[2] = info.fw_minor;
                body[3] = info.fw_patch;
                body[4] = info.board_type;
                let up = info.uptime_seconds.to_le_bytes();
                body[5] = up[0];
                body[6] = up[1];
                body[7] = up[2]; // u24, top byte dropped
                body[8..12].copy_from_slice(&info.unique_id_short.to_le_bytes());
            }
        }
        buf[16] = crc8(&buf[..16]);
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        if buf.len() != PACKET_SIZE {
            return Err(DecodeError::WrongSize);
        }
        if buf[0] != MAGIC {
            return Err(DecodeError::WrongMagic);
        }
        let computed = crc8(&buf[..16]);
        if buf[16] != computed {
            return Err(DecodeError::BadCrc);
        }
        let body: &[u8; 12] = buf[4..16].try_into().unwrap();
        let kind = match buf[1] {
            TYPE_STATE => PacketKind::State(read_state(body)),
            TYPE_HEARTBEAT => PacketKind::Heartbeat(read_state(body)),
            TYPE_DISCOVER => PacketKind::Discover,
            TYPE_ACK => PacketKind::Ack(AckInfo {
                proto_version: body[0],
                fw_major: body[1],
                fw_minor: body[2],
                fw_patch: body[3],
                board_type: body[4],
                uptime_seconds: u32::from_le_bytes([body[5], body[6], body[7], 0]),
                unique_id_short: u32::from_le_bytes([body[8], body[9], body[10], body[11]]),
            }),
            other => return Err(DecodeError::UnknownType(other)),
        };
        Ok(Self {
            kind,
            seq: buf[2],
            flags: buf[3],
        })
    }
}

fn write_state(body: &mut [u8; 12], st: &GamepadState) {
    body[0..2].copy_from_slice(&st.buttons.to_le_bytes());
    body[2] = st.left_trigger;
    body[3] = st.right_trigger;
    body[4..6].copy_from_slice(&st.left_x.to_le_bytes());
    body[6..8].copy_from_slice(&st.left_y.to_le_bytes());
    body[8..10].copy_from_slice(&st.right_x.to_le_bytes());
    body[10..12].copy_from_slice(&st.right_y.to_le_bytes());
}

fn read_state(body: &[u8; 12]) -> GamepadState {
    GamepadState {
        buttons: u16::from_le_bytes([body[0], body[1]]),
        left_trigger: body[2],
        right_trigger: body[3],
        left_x: i16::from_le_bytes([body[4], body[5]]),
        left_y: i16::from_le_bytes([body[6], body[7]]),
        right_x: i16::from_le_bytes([body[8], body[9]]),
        right_y: i16::from_le_bytes([body[10], body[11]]),
    }
}

/// CRC-8/SMBUS: poly 0x07, init 0x00, no reflect, no final XOR.
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    WrongSize,
    WrongMagic,
    BadCrc,
    UnknownType(u8),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongSize => write!(f, "wrong size: expected {} bytes", PACKET_SIZE),
            Self::WrongMagic => write!(f, "wrong magic: expected 0x{:02X}", MAGIC),
            Self::BadCrc => write!(f, "CRC-8 mismatch"),
            Self::UnknownType(t) => write!(f, "unknown packet type 0x{:02X}", t),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Sequence numbers wrap. Returns true if `new` is strictly newer than `old`
/// using modular arithmetic with a half-window of 128.
pub fn seq_is_newer(new: u8, old: u8) -> bool {
    new.wrapping_sub(old) != 0 && new.wrapping_sub(old) < 128
}

/// Build a `TYPE_GET_LOG` request datagram. Same shape as STATE/HEARTBEAT
/// (17 bytes, magic + type + seq + flags + 12-byte body + CRC-8) so the
/// firmware's existing fixed-shape RX path accepts it; the body is
/// reserved for future parameters and is sent as zeros today.
pub fn encode_get_log(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_LOG;
    buf[2] = seq;
    buf[3] = 0;
    // body[0..12] stays zero
    buf[16] = crc8(&buf[..16]);
    buf
}

/// Build a `TYPE_GET_USB_DIAG` request datagram. Same fixed shape as
/// DISCOVER and GET_LOG so the firmware can process it in the existing
/// UDP receive path.
pub fn encode_get_usb_diag(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_USB_DIAG;
    buf[2] = seq;
    buf[3] = 0;
    buf[16] = crc8(&buf[..16]);
    buf
}

/// Build a `TYPE_REBOOT_TO_SETUP` request datagram. Same fixed shape as
/// DISCOVER so run-mode firmware can accept it in the existing UDP path.
pub fn encode_reboot_to_setup(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_REBOOT_TO_SETUP;
    buf[2] = seq;
    buf[3] = 0;
    buf[16] = crc8(&buf[..16]);
    buf
}

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
    pub last_mount_ms: u32,
    pub last_umount_ms: u32,
    pub last_in_queued_ms: u32,
    pub last_in_sent_ms: u32,
    pub last_out_ms: u32,
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
        let crc =
            crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..USB_DIAG_WIRE_SIZE - 2]);
        buf[USB_DIAG_WIRE_SIZE - 2] = (crc & 0xFF) as u8;
        buf[USB_DIAG_WIRE_SIZE - 1] = (crc >> 8) as u8;
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, UsbDiagDecodeError> {
        if buf.len() != USB_DIAG_WIRE_SIZE {
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
        let crc_lo = buf[USB_DIAG_WIRE_SIZE - 2] as u16;
        let crc_hi = buf[USB_DIAG_WIRE_SIZE - 1] as u16;
        let crc_got = crc_lo | (crc_hi << 8);
        let crc_want =
            crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..USB_DIAG_WIRE_SIZE - 2]);
        if crc_got != crc_want {
            return Err(UsbDiagDecodeError::BadCrc {
                got: crc_got,
                want: crc_want,
            });
        }
        if buf[4] != USB_DIAG_VERSION {
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
            last_mount_ms: read_u32_le(buf, 52),
            last_umount_ms: read_u32_le(buf, 56),
            last_in_queued_ms: read_u32_le(buf, 60),
            last_in_sent_ms: read_u32_le(buf, 64),
            last_out_ms: read_u32_le(buf, 68),
            last_out_byte0: buf[72],
            last_out_byte1: buf[73],
        })
    }
}

fn put_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}

#[derive(Debug, PartialEq, Eq)]
pub enum UsbDiagDecodeError {
    WrongSize { got: usize, want: usize },
    WrongMagic,
    WrongType(u8),
    UnsupportedVersion(u8),
    BadCrc { got: u16, want: u16 },
}

impl std::fmt::Display for UsbDiagDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongSize { got, want } => {
                write!(f, "USB diag wrong size: got {got}, want {want}")
            }
            Self::WrongMagic => write!(f, "USB diag wrong magic"),
            Self::WrongType(t) => write!(f, "USB diag wrong type 0x{t:02X}"),
            Self::UnsupportedVersion(v) => write!(f, "USB diag unsupported version {v}"),
            Self::BadCrc { got, want } => {
                write!(
                    f,
                    "USB diag CRC-16 mismatch: got 0x{got:04X}, want 0x{want:04X}"
                )
            }
        }
    }
}

impl std::error::Error for UsbDiagDecodeError {}

/// One chunk of the firmware's diag-log ring, sent in reply to a
/// `TYPE_GET_LOG` request. The bridge reassembles a multi-chunk reply
/// by ordering on `chunk_index`. `lost_bytes` and `total_chunks` are
/// populated only in chunk 0; readers must ignore those fields in
/// later chunks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogChunk {
    pub chunk_index: u8,
    pub flags: u8,
    pub total_chunks: u8,
    pub lost_bytes: u32,
    pub payload: Vec<u8>,
}

impl LogChunk {
    pub fn is_last(&self) -> bool {
        self.flags & LOG_CHUNK_FLAG_LAST != 0
    }

    /// Serialize the chunk to its on-wire form. Used by the firmware (and
    /// by tests that want to drive `decode` against round-trip data).
    pub fn encode(&self) -> Vec<u8> {
        assert!(
            self.payload.len() <= LOG_CHUNK_MAX_PAYLOAD,
            "LogChunk payload over LOG_CHUNK_MAX_PAYLOAD"
        );
        let mut buf = Vec::with_capacity(LOG_CHUNK_HEADER_LEN + self.payload.len() + 2);
        buf.push(MAGIC);
        buf.push(TYPE_LOG_CHUNK);
        buf.push(self.chunk_index);
        buf.push(self.flags);
        buf.push(self.total_chunks);
        let len = self.payload.len() as u16;
        buf.push((len & 0xFF) as u8);
        buf.push((len >> 8) as u8);
        buf.push(0); // reserved
        buf.extend_from_slice(&self.lost_bytes.to_le_bytes());
        buf.extend_from_slice(&self.payload);
        let crc = crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf);
        buf.push((crc & 0xFF) as u8);
        buf.push((crc >> 8) as u8);
        buf
    }

    /// Parse a LogChunk from a UDP datagram. Returns `Err` for any of:
    /// wrong size, wrong magic, wrong type, payload-length disagrees with
    /// total length, or CRC-16 mismatch.
    pub fn decode(buf: &[u8]) -> Result<Self, LogChunkDecodeError> {
        if buf.len() < LOG_CHUNK_HEADER_LEN + 2 {
            return Err(LogChunkDecodeError::TooShort {
                got: buf.len(),
                min: LOG_CHUNK_HEADER_LEN + 2,
            });
        }
        if buf[0] != MAGIC {
            return Err(LogChunkDecodeError::WrongMagic);
        }
        if buf[1] != TYPE_LOG_CHUNK {
            return Err(LogChunkDecodeError::WrongType(buf[1]));
        }
        let payload_len = u16::from_le_bytes([buf[5], buf[6]]) as usize;
        if payload_len > LOG_CHUNK_MAX_PAYLOAD {
            return Err(LogChunkDecodeError::PayloadTooLarge(payload_len));
        }
        let expected_total = LOG_CHUNK_HEADER_LEN + payload_len + 2;
        if buf.len() != expected_total {
            return Err(LogChunkDecodeError::LengthMismatch {
                claimed_payload: payload_len,
                got_total: buf.len(),
                want_total: expected_total,
            });
        }
        let crc_lo = buf[expected_total - 2] as u16;
        let crc_hi = buf[expected_total - 1] as u16;
        let crc_got = crc_lo | (crc_hi << 8);
        let crc_want =
            crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..expected_total - 2]);
        if crc_got != crc_want {
            return Err(LogChunkDecodeError::BadCrc {
                got: crc_got,
                want: crc_want,
            });
        }
        Ok(LogChunk {
            chunk_index: buf[2],
            flags: buf[3],
            total_chunks: buf[4],
            lost_bytes: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            payload: buf[LOG_CHUNK_HEADER_LEN..LOG_CHUNK_HEADER_LEN + payload_len].to_vec(),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LogChunkDecodeError {
    TooShort {
        got: usize,
        min: usize,
    },
    WrongMagic,
    WrongType(u8),
    PayloadTooLarge(usize),
    LengthMismatch {
        claimed_payload: usize,
        got_total: usize,
        want_total: usize,
    },
    BadCrc {
        got: u16,
        want: u16,
    },
}

impl std::fmt::Display for LogChunkDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { got, min } => {
                write!(
                    f,
                    "log chunk too short: got {got} bytes, need at least {min}"
                )
            }
            Self::WrongMagic => write!(f, "log chunk wrong magic"),
            Self::WrongType(t) => write!(f, "log chunk wrong type 0x{t:02X}"),
            Self::PayloadTooLarge(n) => {
                write!(
                    f,
                    "log chunk payload_len {n} exceeds {LOG_CHUNK_MAX_PAYLOAD}"
                )
            }
            Self::LengthMismatch {
                claimed_payload,
                got_total,
                want_total,
            } => write!(
                f,
                "log chunk length mismatch: payload_len={claimed_payload} \
                 total_size={got_total} want_total={want_total}"
            ),
            Self::BadCrc { got, want } => {
                write!(
                    f,
                    "log chunk CRC-16 mismatch: got 0x{got:04X}, want 0x{want:04X}"
                )
            }
        }
    }
}

impl std::error::Error for LogChunkDecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_smbus_check_value() {
        // Canonical CRC-8/SMBUS check over ASCII "123456789" is 0xF4.
        assert_eq!(crc8(b"123456789"), 0xF4);
    }

    #[test]
    fn empty_input_crc_is_zero() {
        assert_eq!(crc8(&[]), 0x00);
    }

    #[test]
    fn state_roundtrip() {
        let pkt = Packet::state(
            42,
            FLAG_PARSEC_CONNECTED,
            GamepadState {
                buttons: 0xABCD,
                left_trigger: 0x12,
                right_trigger: 0xFE,
                left_x: -32000,
                left_y: 32000,
                right_x: 0,
                right_y: -1,
            },
        );
        let buf = pkt.encode();
        assert_eq!(buf[0], MAGIC);
        assert_eq!(buf[1], TYPE_STATE);
        assert_eq!(buf[2], 42);
        assert_eq!(buf[3], FLAG_PARSEC_CONNECTED);
        let back = Packet::decode(&buf).unwrap();
        assert_eq!(pkt, back);
    }

    #[test]
    fn heartbeat_roundtrip() {
        let st = GamepadState {
            buttons: 0x1234,
            ..Default::default()
        };
        let pkt = Packet::heartbeat(7, 0, st);
        let buf = pkt.encode();
        assert_eq!(buf[1], TYPE_HEARTBEAT);
        let back = Packet::decode(&buf).unwrap();
        assert_eq!(pkt, back);
    }

    #[test]
    fn discover_roundtrip() {
        let pkt = Packet::discover(0);
        let buf = pkt.encode();
        assert_eq!(buf[1], TYPE_DISCOVER);
        let back = Packet::decode(&buf).unwrap();
        assert_eq!(pkt, back);
    }

    #[test]
    fn ack_roundtrip() {
        let info = AckInfo {
            proto_version: PROTO_VERSION,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 2,
            board_type: BOARD_PICO_2_W,
            uptime_seconds: 0x123456, // fits in u24
            unique_id_short: 0xDEADBEEF,
        };
        let pkt = Packet::ack(99, info);
        let buf = pkt.encode();
        assert_eq!(buf[1], TYPE_ACK);
        let back = Packet::decode(&buf).unwrap();
        assert_eq!(pkt, back);
        if let PacketKind::Ack(got) = back.kind {
            assert_eq!(got, info);
        } else {
            panic!("decoded ack as wrong kind");
        }
    }

    #[test]
    fn ack_uptime_u24_clamp() {
        // Anything above 0xFFFFFF won't survive the wire roundtrip.
        let info = AckInfo {
            uptime_seconds: 0x01_AB_CD_EF, // top byte 0x01 will be dropped
            ..Default::default()
        };
        let buf = Packet::ack(0, info).encode();
        let back = Packet::decode(&buf).unwrap();
        if let PacketKind::Ack(got) = back.kind {
            assert_eq!(got.uptime_seconds, 0x00_AB_CD_EF);
        } else {
            unreachable!();
        }
    }

    #[test]
    fn bad_crc_rejected() {
        let mut buf = Packet::discover(0).encode();
        buf[16] ^= 0xFF;
        assert_eq!(Packet::decode(&buf), Err(DecodeError::BadCrc));
    }

    #[test]
    fn wrong_magic_rejected() {
        let mut buf = Packet::discover(0).encode();
        buf[0] = 0x00;
        assert_eq!(Packet::decode(&buf), Err(DecodeError::WrongMagic));
    }

    #[test]
    fn wrong_size_rejected() {
        let buf = [0u8; 16];
        assert_eq!(Packet::decode(&buf), Err(DecodeError::WrongSize));
    }

    #[test]
    fn unknown_type_rejected() {
        let mut buf = Packet::discover(0).encode();
        buf[1] = 0xFE;
        buf[16] = crc8(&buf[..16]);
        assert_eq!(Packet::decode(&buf), Err(DecodeError::UnknownType(0xFE)));
    }

    #[test]
    fn seq_wraparound() {
        assert!(seq_is_newer(1, 0));
        assert!(seq_is_newer(0, 255));
        assert!(!seq_is_newer(0, 1));
        assert!(!seq_is_newer(0, 0));
        // half-window boundary
        assert!(seq_is_newer(127, 0));
        assert!(!seq_is_newer(128, 0));
    }

    #[test]
    fn get_log_encode_shape() {
        let buf = encode_get_log(42);
        assert_eq!(buf.len(), PACKET_SIZE);
        assert_eq!(buf[0], MAGIC);
        assert_eq!(buf[1], TYPE_GET_LOG);
        assert_eq!(buf[2], 42);
        assert_eq!(buf[3], 0);
        // body[0..12] reserved -> zeros
        for b in &buf[4..16] {
            assert_eq!(*b, 0);
        }
        // CRC-8 is good
        assert_eq!(buf[16], crc8(&buf[..16]));
        // And it still decodes via the existing Packet path so old code
        // paths see a known shape (even though they ignore the new type).
        // Decode is lenient on unknown types: TYPE_GET_LOG is not in
        // PacketKind, so this should error with UnknownType(0x05).
        assert_eq!(
            Packet::decode(&buf),
            Err(DecodeError::UnknownType(TYPE_GET_LOG))
        );
    }

    #[test]
    fn usb_diag_request_encode_shape() {
        let buf = encode_get_usb_diag(7);
        assert_eq!(buf.len(), PACKET_SIZE);
        assert_eq!(buf[0], MAGIC);
        assert_eq!(buf[1], TYPE_GET_USB_DIAG);
        assert_eq!(buf[2], 7);
        assert_eq!(buf[3], 0);
        for b in &buf[4..16] {
            assert_eq!(*b, 0);
        }
        assert_eq!(buf[16], crc8(&buf[..16]));
    }

    #[test]
    fn reboot_to_setup_request_encode_shape() {
        let buf = encode_reboot_to_setup(9);
        assert_eq!(buf.len(), PACKET_SIZE);
        assert_eq!(buf[0], MAGIC);
        assert_eq!(buf[1], TYPE_REBOOT_TO_SETUP);
        assert_eq!(buf[2], 9);
        assert_eq!(buf[3], 0);
        for b in &buf[4..16] {
            assert_eq!(*b, 0);
        }
        assert_eq!(buf[16], crc8(&buf[..16]));
        assert_eq!(
            Packet::decode(&buf),
            Err(DecodeError::UnknownType(TYPE_REBOOT_TO_SETUP))
        );
    }

    #[test]
    fn usb_diag_roundtrip() {
        let diag = UsbDiag {
            seq: 9,
            flags: 0,
            version: USB_DIAG_VERSION,
            usb_flags: USB_DIAG_FLAG_MOUNTED,
            activity_flags: USB_DIAG_ACTIVITY_SENT | USB_DIAG_ACTIVITY_OUT,
            last_out_len: 8,
            now_ms: 1000,
            last_bridge_packet_ms: 900,
            mount_count: 1,
            umount_count: 2,
            suspend_count: 3,
            resume_count: 4,
            device_desc_count: 5,
            config_desc_count: 6,
            xinput_in_queued_count: 7,
            xinput_in_sent_count: 8,
            xinput_out_count: 9,
            last_mount_ms: 10,
            last_umount_ms: 11,
            last_in_queued_ms: 12,
            last_in_sent_ms: 13,
            last_out_ms: 14,
            last_out_byte0: 0x01,
            last_out_byte1: 0x08,
        };
        let buf = diag.encode();
        assert_eq!(buf.len(), USB_DIAG_WIRE_SIZE);
        assert_eq!(buf[1], TYPE_USB_DIAG);
        let back = UsbDiag::decode(&buf).unwrap();
        assert_eq!(back, diag);
        assert!(back.mounted());
        assert!(back.xinput_report_sent());
        assert!(back.xinput_out_seen());
        assert_eq!(back.age_ms(990), Some(10));
    }

    #[test]
    fn usb_diag_bad_crc_rejected() {
        let mut buf = UsbDiag {
            seq: 0,
            flags: 0,
            version: USB_DIAG_VERSION,
            usb_flags: 0,
            activity_flags: 0,
            last_out_len: 0,
            now_ms: 0,
            last_bridge_packet_ms: 0,
            mount_count: 0,
            umount_count: 0,
            suspend_count: 0,
            resume_count: 0,
            device_desc_count: 0,
            config_desc_count: 0,
            xinput_in_queued_count: 0,
            xinput_in_sent_count: 0,
            xinput_out_count: 0,
            last_mount_ms: 0,
            last_umount_ms: 0,
            last_in_queued_ms: 0,
            last_in_sent_ms: 0,
            last_out_ms: 0,
            last_out_byte0: 0,
            last_out_byte1: 0,
        }
        .encode();
        buf[10] ^= 0xFF;
        match UsbDiag::decode(&buf) {
            Err(UsbDiagDecodeError::BadCrc { .. }) => (),
            other => panic!("expected BadCrc, got {other:?}"),
        }
    }

    fn make_chunk(idx: u8, flags: u8, total: u8, lost: u32, payload: &[u8]) -> LogChunk {
        LogChunk {
            chunk_index: idx,
            flags,
            total_chunks: total,
            lost_bytes: lost,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn log_chunk_roundtrip_empty_payload() {
        let c = make_chunk(0, LOG_CHUNK_FLAG_LAST, 1, 0, &[]);
        let buf = c.encode();
        assert_eq!(buf.len(), LOG_CHUNK_HEADER_LEN + 2);
        let back = LogChunk::decode(&buf).unwrap();
        assert_eq!(back, c);
        assert!(back.is_last());
    }

    #[test]
    fn log_chunk_roundtrip_max_payload() {
        let payload: Vec<u8> = (0..LOG_CHUNK_MAX_PAYLOAD)
            .map(|i| (i & 0xFF) as u8)
            .collect();
        assert_eq!(payload.len(), LOG_CHUNK_MAX_PAYLOAD);
        let c = make_chunk(5, 0, 16, 1234, &payload);
        let buf = c.encode();
        assert_eq!(buf.len(), LOG_CHUNK_HEADER_LEN + LOG_CHUNK_MAX_PAYLOAD + 2);
        let back = LogChunk::decode(&buf).unwrap();
        assert_eq!(back, c);
        assert!(!back.is_last());
        assert_eq!(back.lost_bytes, 1234);
    }

    #[test]
    fn log_chunk_last_flag_decoded() {
        let c = make_chunk(15, LOG_CHUNK_FLAG_LAST, 16, 0, b"final-chunk");
        let buf = c.encode();
        let back = LogChunk::decode(&buf).unwrap();
        assert!(back.is_last());
    }

    #[test]
    fn log_chunk_bad_crc_rejected() {
        let c = make_chunk(0, 0, 1, 0, b"hello");
        let mut buf = c.encode();
        let last = buf.len() - 1;
        buf[last] ^= 0xFF;
        match LogChunk::decode(&buf) {
            Err(LogChunkDecodeError::BadCrc { .. }) => (),
            other => panic!("expected BadCrc, got {other:?}"),
        }
    }

    #[test]
    fn log_chunk_wrong_magic_rejected() {
        let c = make_chunk(0, 0, 1, 0, b"hi");
        let mut buf = c.encode();
        buf[0] = 0x00;
        match LogChunk::decode(&buf) {
            Err(LogChunkDecodeError::WrongMagic) => (),
            other => panic!("expected WrongMagic, got {other:?}"),
        }
    }

    #[test]
    fn log_chunk_wrong_type_rejected() {
        let c = make_chunk(0, 0, 1, 0, b"hi");
        let mut buf = c.encode();
        buf[1] = TYPE_ACK;
        match LogChunk::decode(&buf) {
            Err(LogChunkDecodeError::WrongType(t)) if t == TYPE_ACK => (),
            other => panic!("expected WrongType(TYPE_ACK), got {other:?}"),
        }
    }

    #[test]
    fn log_chunk_length_mismatch_rejected() {
        let c = make_chunk(0, 0, 1, 0, b"hi");
        let mut buf = c.encode();
        // Claim a longer payload than the buffer actually contains.
        buf[5] = 99; // payload_len LE lo
        match LogChunk::decode(&buf) {
            Err(LogChunkDecodeError::LengthMismatch { .. }) => (),
            other => panic!("expected LengthMismatch, got {other:?}"),
        }
    }

    #[test]
    fn ack_capability_flag_decoded() {
        // Hand-roll an ACK datagram with the capability bit set in the
        // flags byte so the bridge sees what new firmware will send.
        let info = AckInfo {
            proto_version: PROTO_VERSION,
            fw_major: 1,
            fw_minor: 2,
            fw_patch: 3,
            board_type: BOARD_PICO_W_RP2040,
            uptime_seconds: 60,
            unique_id_short: 0xABCD1234,
        };
        let mut buf = Packet::ack(7, info).encode();
        // Stomp the flags byte with the capability bit and re-CRC.
        buf[3] = ACK_FLAG_LOG_CHUNK_SUPPORTED;
        buf[16] = crc8(&buf[..16]);
        let back = Packet::decode(&buf).unwrap();
        assert_eq!(
            back.flags & ACK_FLAG_LOG_CHUNK_SUPPORTED,
            ACK_FLAG_LOG_CHUNK_SUPPORTED
        );
        match back.kind {
            PacketKind::Ack(got) => assert_eq!(got, info),
            other => panic!("expected Ack, got {other:?}"),
        }
    }
}
