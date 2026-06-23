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

mod wire;

pub use wire::crc8;
use wire::{put_u16_le, put_u32_le, read_u16_le, read_u32_le};

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

/// Set in a `TYPE_LOG_CHUNK` datagram's `flags` byte to mark the final
/// chunk in the reply sequence.
pub const LOG_CHUNK_FLAG_LAST: u8 = 1 << 0;

/// Maximum log-chunk payload size in bytes. With a 16 KiB diag ring on
/// the firmware, a complete snapshot fits in 64 chunks. Comfortably
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
mod tests;
