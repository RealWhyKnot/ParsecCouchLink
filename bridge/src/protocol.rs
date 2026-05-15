//! Wire protocol per wiki/Protocol.md. UDP port 4242, 17-byte fixed datagrams,
//! little-endian, CRC-8/SMBUS over the first 16 bytes.
//!
//! Constants and helpers in this module mirror the protocol spec
//! one-for-one; some aren't directly consumed by the Windows bridge but
//! are exported so external tooling (the throwaway listener, future Pico
//! firmware-side bindings, support-bundle annotations) can reference the
//! same names. `dead_code` is suppressed for that reason.

#![allow(dead_code)]

pub const PORT: u16 = 4242;
pub const PACKET_SIZE: usize = 17;
pub const MAGIC: u8 = 0xA5;

pub const TYPE_STATE: u8 = 0x01;
pub const TYPE_HEARTBEAT: u8 = 0x02;
pub const TYPE_DISCOVER: u8 = 0x03;
pub const TYPE_ACK: u8 = 0x04;

pub const FLAG_PARSEC_CONNECTED: u8 = 1 << 0;
pub const FLAG_NEUTRALIZE: u8 = 1 << 1;

/// Current wire-protocol version. Bumped whenever the on-wire layout for
/// any packet type changes.
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
}
