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

mod log_chunk;
mod pico_state;
pub(crate) mod usb_diag;
mod wire;

pub use log_chunk::LogChunk;
#[cfg(test)]
pub(crate) use log_chunk::{
    LogChunkDecodeError, LOG_CHUNK_FLAG_LAST, LOG_CHUNK_HEADER_LEN, LOG_CHUNK_MAX_PAYLOAD,
};
pub use pico_state::{bt_hid_target_label, PicoStateDiag};
pub use usb_diag::UsbDiag;
pub use wire::crc8;
#[cfg(test)]
use wire::put_u32_le;

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
/// Request for the firmware's current USB status over UDP.
pub const TYPE_GET_USB_DIAG: u8 = 0x06;
/// Request for run-mode firmware to reboot into setup-mode USB-CDC.
pub const TYPE_REBOOT_TO_SETUP: u8 = 0x07;
/// Keyboard report for the HID keyboard persona. Same 17-byte shape as
/// STATE; the 8-byte HID boot report lives in the first 8 body bytes
/// (`modifiers`, reserved, then six key usage codes).
pub const TYPE_KEY_STATE: u8 = 0x08;
/// Keyboard heartbeat, sent when the report is unchanged so the firmware
/// watchdog stays fed. Mirrors `TYPE_HEARTBEAT` for the pad path.
pub const TYPE_KEY_HEARTBEAT: u8 = 0x09;
/// Ask a run-mode Pico to persist a new output persona and reboot into it.
/// `body[0]` carries the desired persona's flash byte (see `Persona`).
pub const TYPE_SET_PERSONA: u8 = 0x0A;
/// Request the full firmware version from run-mode firmware. The ACK keeps a
/// compact legacy date triplet; this optional follow-up carries revision and
/// development suffix without changing the ACK shape.
pub const TYPE_GET_VERSION: u8 = 0x0B;
/// Request a richer current-state diagnostic snapshot from run-mode
/// firmware. Optional: older firmware ignores it and the bridge falls back
/// to ACK/version/USB-diag/log/cache evidence.
pub const TYPE_GET_PICO_STATE: u8 = 0x0C;
/// Request a run-mode Pico to reboot into a persona with one-shot raw USB
/// packet capture enabled before TinyUSB starts. `body[0]` carries the
/// persona flash byte and `body[1]` is 1 to enable capture or 0 to clear
/// runtime capture without rebooting.
pub const TYPE_SET_USB_CAPTURE: u8 = 0x0D;
/// Chunk of diag-log payload sent by the firmware in reply to
/// `TYPE_GET_LOG`. Variable-length (12-byte header + up to 256 bytes of
/// payload + 2 bytes CRC-16). High bit set, matching the CDC convention
/// for response opcodes.
pub const TYPE_LOG_CHUNK: u8 = 0x85;
/// USB status reply sent by run-mode firmware.
pub const TYPE_USB_DIAG: u8 = 0x86;
/// Fixed 17-byte reply to `TYPE_GET_VERSION`.
pub const TYPE_VERSION: u8 = 0x87;
/// Current-state diagnostic reply sent by run-mode firmware.
pub const TYPE_PICO_STATE: u8 = 0x88;

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
/// Set in the ACK flags byte when the Pico is currently presenting the
/// HID keyboard persona. Implies `TYPE_SET_PERSONA` and the keyboard
/// streaming types are understood; the bridge streams key reports rather
/// than pad state to this Pico.
pub const ACK_FLAG_KEYBOARD_PERSONA: u8 = 1 << 3;
/// Set in the ACK flags byte when firmware supports `TYPE_GET_VERSION` /
/// `TYPE_VERSION`.
pub const ACK_FLAG_FULL_VERSION_SUPPORTED: u8 = 1 << 4;
/// Set in the ACK flags byte when the Pico is presenting the Dreamcast
/// Maple adapter persona. Combined with `ACK_FLAG_ALT_PERSONA` for the
/// Xbox One-compatible persona, and with `ACK_FLAG_DINPUT_PERSONA` for the
/// generic HID gamepad persona.
pub const ACK_FLAG_MAPLE_PERSONA: u8 = 1 << 5;
/// Set in the ACK flags byte when the Pico is presenting the PS3 HID
/// persona. Combined with `ACK_FLAG_ALT_PERSONA` for the PS4 HID persona,
/// and with `ACK_FLAG_MAPLE_PERSONA` for the generic HID gamepad persona.
pub const ACK_FLAG_DINPUT_PERSONA: u8 = 1 << 6;
/// Extends the persona bits without changing the fixed ACK packet shape.
/// Alone it marks generic Bluetooth HID; exact combinations with the
/// other persona bits mark additional Bluetooth HID target layouts.
pub const ACK_FLAG_ALT_PERSONA: u8 = 1 << 7;

pub const USB_DIAG_V1_WIRE_SIZE: usize = 78;
pub const USB_DIAG_WIRE_SIZE: usize = 104;
pub const USB_DIAG_V1_VERSION: u8 = 1;
pub const USB_DIAG_VERSION: u8 = 2;
pub const USB_DIAG_FLAG_MOUNTED: u8 = 1 << 0;
pub const USB_DIAG_FLAG_SUSPENDED: u8 = 1 << 1;
pub const USB_DIAG_ACTIVITY_QUEUED: u8 = 1 << 0;
pub const USB_DIAG_ACTIVITY_SENT: u8 = 1 << 1;
pub const USB_DIAG_ACTIVITY_OUT: u8 = 1 << 2;
pub const USB_DIAG_ACTIVITY_PEER: u8 = 1 << 3;
pub const USB_DIAG_ACTIVITY_PARSEC: u8 = 1 << 4;
pub const USB_DIAG_IN_BLOCKED_NONE: u8 = 0;
pub const USB_DIAG_IN_BLOCKED_NOT_MOUNTED: u8 = 1;
pub const USB_DIAG_IN_BLOCKED_NOT_READY: u8 = 2;
pub const USB_DIAG_IN_BLOCKED_SHORT_WRITE: u8 = 3;

pub const PICO_STATE_V1_WIRE_SIZE: usize = 104;
pub const PICO_STATE_V2_WIRE_SIZE: usize = 128;
pub const PICO_STATE_WIRE_SIZE: usize = 176;
pub const PICO_STATE_V1_VERSION: u8 = 1;
pub const PICO_STATE_V2_VERSION: u8 = 2;
pub const PICO_STATE_VERSION: u8 = 3;

pub const BT_HID_STATUS_STARTED: u8 = 1 << 0;
pub const BT_HID_STATUS_CONNECTED: u8 = 1 << 1;
pub const BT_HID_STATUS_SEND_REQUESTED: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbConfigurationState {
    NoHostTraffic,
    EnumerationStarted,
    ConfiguredThenUnmounted,
    ConfiguredThenUnmountedWithoutCallback,
    Suspended,
    Configured,
}

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

/// One USB HID boot-keyboard report: a modifier bitmap plus up to six
/// concurrently-held key usage codes (HID Keyboard/Keypad page 0x07).
/// The on-wire reserved byte (boot report byte 1) is always zero and is
/// not stored here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardReport {
    pub modifiers: u8,
    pub keys: [u8; 6],
}

/// Which output device a Pico presents in run mode. Discovered from the ACK
/// flags byte and persisted on the Pico in flash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Persona {
    #[default]
    Xinput,
    Keyboard,
    Maple,
    Ps3,
    Ps4,
    XboxOne,
    Debug,
    GenericHid,
    BluetoothHid,
    BluetoothXbox,
    BluetoothPlaystation,
}

impl Persona {
    /// Read the active persona from an ACK packet's flags byte.
    pub fn from_ack_flags(flags: u8) -> Self {
        const PERSONA_MASK: u8 = ACK_FLAG_KEYBOARD_PERSONA
            | ACK_FLAG_MAPLE_PERSONA
            | ACK_FLAG_DINPUT_PERSONA
            | ACK_FLAG_ALT_PERSONA;
        let persona_bits = flags & PERSONA_MASK;
        match persona_bits {
            ACK_FLAG_ALT_PERSONA => return Persona::BluetoothHid,
            bits if bits
                == (ACK_FLAG_DINPUT_PERSONA | ACK_FLAG_MAPLE_PERSONA | ACK_FLAG_ALT_PERSONA) =>
            {
                return Persona::BluetoothXbox
            }
            bits if bits
                == (ACK_FLAG_KEYBOARD_PERSONA
                    | ACK_FLAG_DINPUT_PERSONA
                    | ACK_FLAG_MAPLE_PERSONA
                    | ACK_FLAG_ALT_PERSONA) =>
            {
                return Persona::BluetoothPlaystation
            }
            _ => {}
        }

        if persona_bits & ACK_FLAG_KEYBOARD_PERSONA != 0 && persona_bits & ACK_FLAG_ALT_PERSONA != 0
        {
            Persona::Debug
        } else if persona_bits & ACK_FLAG_KEYBOARD_PERSONA != 0 {
            Persona::Keyboard
        } else if persona_bits & ACK_FLAG_MAPLE_PERSONA != 0
            && persona_bits & ACK_FLAG_ALT_PERSONA != 0
        {
            Persona::XboxOne
        } else if persona_bits & ACK_FLAG_DINPUT_PERSONA != 0
            && persona_bits & ACK_FLAG_ALT_PERSONA != 0
        {
            Persona::Ps4
        } else if persona_bits & ACK_FLAG_DINPUT_PERSONA != 0
            && persona_bits & ACK_FLAG_MAPLE_PERSONA != 0
        {
            Persona::GenericHid
        } else if persona_bits & ACK_FLAG_DINPUT_PERSONA != 0 {
            Persona::Ps3
        } else if persona_bits & ACK_FLAG_MAPLE_PERSONA != 0 {
            Persona::Maple
        } else {
            Persona::Xinput
        }
    }

    /// The byte the firmware stores in flash and accepts in a
    /// `TYPE_SET_PERSONA` request body.
    pub fn flash_byte(self) -> u8 {
        match self {
            Persona::Xinput => 0,
            Persona::Keyboard => 1,
            Persona::Maple => 2,
            Persona::Ps3 => 3,
            Persona::Ps4 => 4,
            Persona::XboxOne => 5,
            Persona::Debug => 6,
            Persona::GenericHid => 7,
            Persona::BluetoothHid => 8,
            Persona::BluetoothXbox => 9,
            Persona::BluetoothPlaystation => 10,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Persona::Xinput => "xinput",
            Persona::Keyboard => "keyboard",
            Persona::Maple => "maple",
            Persona::Ps3 => "ps3",
            Persona::Ps4 => "ps4",
            Persona::XboxOne => "xboxone",
            Persona::Debug => "debug",
            Persona::GenericHid => "generic-hid",
            Persona::BluetoothHid => "bluetooth",
            Persona::BluetoothXbox => "bluetooth-xbox",
            Persona::BluetoothPlaystation => "bluetooth-playstation",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Persona::Xinput => "Xbox 360 / XInput",
            Persona::Keyboard => "Keyboard",
            Persona::Maple => "Maple adapter",
            Persona::Ps3 => "PS3 / DualShock 3",
            Persona::Ps4 => "PS4 / DualShock 4",
            Persona::XboxOne => "Xbox One",
            Persona::Debug => "Debug packet capture",
            Persona::GenericHid => "Generic HID gamepad",
            Persona::BluetoothHid => "Bluetooth generic HID",
            Persona::BluetoothXbox => "Bluetooth Xbox Wireless Controller",
            Persona::BluetoothPlaystation => "Bluetooth DualShock 4",
        }
    }

    pub fn is_bluetooth(self) -> bool {
        matches!(
            self,
            Persona::BluetoothHid | Persona::BluetoothXbox | Persona::BluetoothPlaystation
        )
    }
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
    pub full_version: Option<FirmwareVersion>,
}

impl AckInfo {
    pub fn firmware_version(&self) -> FirmwareVersion {
        self.full_version.unwrap_or_else(|| {
            FirmwareVersion::from_triplet(self.fw_major, self.fw_minor, self.fw_patch)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: FirmwareVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    State(GamepadState),
    Heartbeat(GamepadState),
    KeyState(KeyboardReport),
    KeyHeartbeat(KeyboardReport),
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

    pub fn key_state(seq: u8, flags: u8, report: KeyboardReport) -> Self {
        Self {
            kind: PacketKind::KeyState(report),
            seq,
            flags,
        }
    }

    pub fn key_heartbeat(seq: u8, flags: u8, report: KeyboardReport) -> Self {
        Self {
            kind: PacketKind::KeyHeartbeat(report),
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
            PacketKind::KeyState(_) => TYPE_KEY_STATE,
            PacketKind::KeyHeartbeat(_) => TYPE_KEY_HEARTBEAT,
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
            PacketKind::KeyState(kr) | PacketKind::KeyHeartbeat(kr) => {
                write_key(body, &kr);
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
            TYPE_KEY_STATE => PacketKind::KeyState(read_key(body)),
            TYPE_KEY_HEARTBEAT => PacketKind::KeyHeartbeat(read_key(body)),
            TYPE_DISCOVER => PacketKind::Discover,
            TYPE_ACK => PacketKind::Ack(AckInfo {
                proto_version: body[0],
                fw_major: body[1],
                fw_minor: body[2],
                fw_patch: body[3],
                board_type: body[4],
                uptime_seconds: u32::from_le_bytes([body[5], body[6], body[7], 0]),
                unique_id_short: u32::from_le_bytes([body[8], body[9], body[10], body[11]]),
                full_version: None,
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

fn write_key(body: &mut [u8; 12], rep: &KeyboardReport) {
    body[0] = rep.modifiers;
    body[1] = 0; // HID boot report reserved byte
    body[2..8].copy_from_slice(&rep.keys);
    // body[8..12] stay zero
}

fn read_key(body: &[u8; 12]) -> KeyboardReport {
    let mut keys = [0u8; 6];
    keys.copy_from_slice(&body[2..8]);
    KeyboardReport {
        modifiers: body[0],
        keys,
    }
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

/// Build a `TYPE_SET_PERSONA` request datagram. The desired persona's
/// flash byte goes in `body[0]`; the rest of the body is reserved zero.
/// Same fixed shape as DISCOVER so run-mode firmware accepts it in the
/// existing UDP path.
pub fn encode_set_persona(seq: u8, persona: Persona) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_SET_PERSONA;
    buf[2] = seq;
    buf[3] = 0;
    buf[4] = persona.flash_byte();
    buf[16] = crc8(&buf[..16]);
    buf
}

/// Build a `TYPE_SET_USB_CAPTURE` request datagram. When `enabled` is true,
/// firmware persists the requested persona if needed, marks the next boot for
/// one-shot raw USB packet capture, and reboots. When false, firmware clears
/// capture for the current boot without changing persona.
pub fn encode_set_usb_capture(seq: u8, persona: Persona, enabled: bool) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_SET_USB_CAPTURE;
    buf[2] = seq;
    buf[3] = 0;
    buf[4] = persona.flash_byte();
    buf[5] = u8::from(enabled);
    buf[16] = crc8(&buf[..16]);
    buf
}

/// Build a `TYPE_GET_VERSION` request datagram. The response is decoded with
/// `VersionInfo::decode`.
pub fn encode_get_version(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_VERSION;
    buf[2] = seq;
    buf[16] = crc8(&buf[..16]);
    buf
}

/// Build a `TYPE_GET_PICO_STATE` request datagram. Same fixed shape as
/// discovery and the other diagnostic requests.
pub fn encode_get_pico_state(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_PICO_STATE;
    buf[2] = seq;
    buf[16] = crc8(&buf[..16]);
    buf
}

impl VersionInfo {
    pub fn encode(&self, seq: u8, flags: u8) -> [u8; PACKET_SIZE] {
        let mut buf = [0u8; PACKET_SIZE];
        buf[0] = MAGIC;
        buf[1] = TYPE_VERSION;
        buf[2] = seq;
        buf[3] = flags;
        if let FirmwareVersion::Release {
            year,
            month,
            day,
            revision: Some(revision),
            suffix,
        } = self.version
        {
            let body: &mut [u8; 12] = (&mut buf[4..16]).try_into().unwrap();
            body[0..2].copy_from_slice(&year.to_le_bytes());
            body[2] = month;
            body[3] = day;
            body[4] = revision;
            if let Some(suffix) = suffix {
                body[5] = 4;
                body[6..10].copy_from_slice(&suffix);
            }
        }
        buf[16] = crc8(&buf[..16]);
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::decode_with_header(buf)?.2)
    }

    pub fn decode_with_header(buf: &[u8]) -> Result<(u8, u8, Self), DecodeError> {
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
        if buf[1] != TYPE_VERSION {
            return Err(DecodeError::UnknownType(buf[1]));
        }
        let body: &[u8; 12] = buf[4..16].try_into().unwrap();
        let year = u16::from_le_bytes([body[0], body[1]]);
        let month = body[2];
        let day = body[3];
        let revision = body[4];
        if !(2020..=2099).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day)
        {
            return Err(DecodeError::UnknownType(TYPE_VERSION));
        }
        let suffix = if body[5] == 4 {
            let mut suffix = [0u8; 4];
            suffix.copy_from_slice(&body[6..10]);
            if suffix.iter().all(|b| b.is_ascii_alphanumeric()) {
                Some(suffix)
            } else {
                None
            }
        } else {
            None
        };
        Ok((
            buf[2],
            buf[3],
            Self {
                version: FirmwareVersion::Release {
                    year,
                    month,
                    day,
                    revision: Some(revision),
                    suffix,
                },
            },
        ))
    }
}

pub fn usb_in_blocked_reason_label(reason: u8) -> &'static str {
    match reason {
        USB_DIAG_IN_BLOCKED_NONE => "none",
        USB_DIAG_IN_BLOCKED_NOT_MOUNTED => "not_mounted",
        USB_DIAG_IN_BLOCKED_NOT_READY => "not_ready",
        USB_DIAG_IN_BLOCKED_SHORT_WRITE => "short_write",
        _ => "unknown",
    }
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

#[derive(Debug, PartialEq, Eq)]
pub enum PicoStateDecodeError {
    WrongSize { got: usize, want: usize },
    WrongMagic,
    WrongType(u8),
    UnsupportedVersion(u8),
    BadCrc { got: u16, want: u16 },
}

impl std::fmt::Display for PicoStateDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongSize { got, want } => {
                write!(f, "Pico state wrong size: got {got}, want {want}")
            }
            Self::WrongMagic => write!(f, "Pico state wrong magic"),
            Self::WrongType(t) => write!(f, "Pico state wrong type 0x{t:02X}"),
            Self::UnsupportedVersion(v) => write!(f, "Pico state unsupported version {v}"),
            Self::BadCrc { got, want } => {
                write!(
                    f,
                    "Pico state CRC-16 mismatch: got 0x{got:04X}, want 0x{want:04X}"
                )
            }
        }
    }
}

impl std::error::Error for PicoStateDecodeError {}

#[cfg(test)]
mod tests;
