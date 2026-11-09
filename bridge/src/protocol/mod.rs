use crate::firmware_version::FirmwareVersion;

mod decode_error;
mod log_chunk;
mod pico_state;
mod requests;
pub(crate) mod usb_diag;
mod version;
mod wire;

pub(crate) use decode_error::{DecodeError, PicoStateDecodeError, UsbDiagDecodeError};
pub(crate) use log_chunk::LogChunk;
#[cfg(test)]
pub(crate) use log_chunk::{
    LOG_CHUNK_FLAG_LAST, LOG_CHUNK_HEADER_LEN, LOG_CHUNK_MAX_PAYLOAD, LogChunkDecodeError,
};
pub(crate) use pico_state::{PicoStateDiag, bt_hid_target_label};
pub(crate) use requests::{
    encode_get_log, encode_get_pico_state, encode_get_usb_diag, encode_get_version,
    encode_identify, encode_reboot_to_setup, encode_set_persona, encode_set_usb_capture,
};
pub(crate) use usb_diag::UsbDiag;
pub(crate) use version::VersionInfo;
pub(crate) use wire::crc8;
#[cfg(test)]
use wire::put_u32_le;

pub(crate) const PORT: u16 = 4242;
pub(crate) const PACKET_SIZE: usize = 17;
pub(crate) const MAGIC: u8 = 0xA5;

pub(crate) const TYPE_STATE: u8 = 0x01;
pub(crate) const TYPE_HEARTBEAT: u8 = 0x02;
pub(crate) const TYPE_DISCOVER: u8 = 0x03;
pub(crate) const TYPE_ACK: u8 = 0x04;
pub(crate) const TYPE_GET_LOG: u8 = 0x05;
pub(crate) const TYPE_GET_USB_DIAG: u8 = 0x06;
pub(crate) const TYPE_REBOOT_TO_SETUP: u8 = 0x07;
pub(crate) const TYPE_KEY_STATE: u8 = 0x08;
pub(crate) const TYPE_KEY_HEARTBEAT: u8 = 0x09;
pub(crate) const TYPE_SET_PERSONA: u8 = 0x0A;
pub(crate) const TYPE_GET_VERSION: u8 = 0x0B;
pub(crate) const TYPE_GET_PICO_STATE: u8 = 0x0C;
pub(crate) const TYPE_SET_USB_CAPTURE: u8 = 0x0D;
pub(crate) const TYPE_IDENTIFY: u8 = 0x0E;
pub(crate) const TYPE_LOG_CHUNK: u8 = 0x85;
pub(crate) const TYPE_USB_DIAG: u8 = 0x86;
pub(crate) const TYPE_VERSION: u8 = 0x87;
pub(crate) const TYPE_PICO_STATE: u8 = 0x88;

pub(crate) const FLAG_PARSEC_CONNECTED: u8 = 1 << 0;
pub(crate) const ACK_FLAG_LOG_CHUNK_SUPPORTED: u8 = 1 << 0;
#[cfg(test)]
pub(crate) const ACK_FLAG_USB_DIAG_SUPPORTED: u8 = 1 << 1;
pub(crate) const ACK_FLAG_KEYBOARD_PERSONA: u8 = 1 << 3;
pub(crate) const ACK_FLAG_FULL_VERSION_SUPPORTED: u8 = 1 << 4;
pub(crate) const ACK_FLAG_MAPLE_PERSONA: u8 = 1 << 5;
pub(crate) const ACK_FLAG_DINPUT_PERSONA: u8 = 1 << 6;
pub(crate) const ACK_FLAG_ALT_PERSONA: u8 = 1 << 7;

pub(crate) const USB_DIAG_V1_WIRE_SIZE: usize = 78;
pub(crate) const USB_DIAG_WIRE_SIZE: usize = 104;
pub(crate) const USB_DIAG_V1_VERSION: u8 = 1;
pub(crate) const USB_DIAG_VERSION: u8 = 2;
pub(crate) const USB_DIAG_FLAG_MOUNTED: u8 = 1 << 0;
pub(crate) const USB_DIAG_FLAG_SUSPENDED: u8 = 1 << 1;
pub(crate) const USB_DIAG_ACTIVITY_SENT: u8 = 1 << 1;
pub(crate) const USB_DIAG_ACTIVITY_OUT: u8 = 1 << 2;
pub(crate) const USB_DIAG_ACTIVITY_PEER: u8 = 1 << 3;
pub(crate) const USB_DIAG_ACTIVITY_PARSEC: u8 = 1 << 4;
pub(crate) const USB_DIAG_IN_BLOCKED_NONE: u8 = 0;
pub(crate) const USB_DIAG_IN_BLOCKED_NOT_MOUNTED: u8 = 1;
pub(crate) const USB_DIAG_IN_BLOCKED_NOT_READY: u8 = 2;
pub(crate) const USB_DIAG_IN_BLOCKED_SHORT_WRITE: u8 = 3;

pub(crate) const PICO_STATE_V1_WIRE_SIZE: usize = 104;
pub(crate) const PICO_STATE_V2_WIRE_SIZE: usize = 128;
pub(crate) const PICO_STATE_WIRE_SIZE: usize = 176;
pub(crate) const PICO_STATE_V1_VERSION: u8 = 1;
pub(crate) const PICO_STATE_V2_VERSION: u8 = 2;
pub(crate) const PICO_STATE_VERSION: u8 = 3;

pub(crate) const BT_HID_STATUS_STARTED: u8 = 1 << 0;
pub(crate) const BT_HID_STATUS_CONNECTED: u8 = 1 << 1;
pub(crate) const BT_HID_STATUS_SEND_REQUESTED: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UsbConfigurationState {
    NoHostTraffic,
    EnumerationStarted,
    ConfiguredThenUnmounted,
    ConfiguredThenUnmountedWithoutCallback,
    Suspended,
    Configured,
}

pub(crate) const PROTO_VERSION: u8 = 2;
pub(crate) const PROTO_VERSION_OPEN: u8 = 1;

pub(crate) fn proto_supported(version: u8) -> bool {
    version == PROTO_VERSION || version == PROTO_VERSION_OPEN
}

pub(crate) const BOARD_PICO_2_W: u8 = 0x01;
pub(crate) const BOARD_PICO_W_RP2040: u8 = 0x02;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GamepadState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyboardReport {
    pub modifiers: u8,
    pub keys: [u8; 6],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Persona {
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
    pub(crate) fn from_ack_flags(flags: u8) -> Self {
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
                return Persona::BluetoothXbox;
            }
            bits if bits
                == (ACK_FLAG_KEYBOARD_PERSONA
                    | ACK_FLAG_DINPUT_PERSONA
                    | ACK_FLAG_MAPLE_PERSONA
                    | ACK_FLAG_ALT_PERSONA) =>
            {
                return Persona::BluetoothPlaystation;
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

    pub(crate) fn flash_byte(self) -> u8 {
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

    pub(crate) fn label(self) -> &'static str {
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

    pub(crate) fn console_label(self) -> &'static str {
        match self {
            Persona::Xinput => "Xbox 360",
            Persona::Keyboard => "Keyboard",
            Persona::Maple => "Dreamcast",
            Persona::Ps3 => "PS3",
            Persona::Ps4 => "PS4",
            Persona::XboxOne => "Xbox One",
            Persona::Debug => "Debug",
            Persona::GenericHid => "Generic HID",
            Persona::BluetoothHid => "Bluetooth",
            Persona::BluetoothXbox => "Bluetooth Xbox",
            Persona::BluetoothPlaystation => "Bluetooth PS4",
        }
    }

    pub(crate) fn is_bluetooth(self) -> bool {
        matches!(
            self,
            Persona::BluetoothHid | Persona::BluetoothXbox | Persona::BluetoothPlaystation
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AckInfo {
    pub proto_version: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub board_type: u8,
    pub uptime_seconds: u32,
    pub unique_id_short: u32,
    pub full_version: Option<FirmwareVersion>,
}

impl AckInfo {
    pub(crate) fn firmware_version(&self) -> FirmwareVersion {
        self.full_version.unwrap_or_else(|| {
            FirmwareVersion::from_triplet(self.fw_major, self.fw_minor, self.fw_patch)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PacketKind {
    State(GamepadState),
    Heartbeat(GamepadState),
    KeyState(KeyboardReport),
    KeyHeartbeat(KeyboardReport),
    Discover,
    Ack(AckInfo),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Packet {
    pub kind: PacketKind,
    pub seq: u8,
    pub flags: u8,
}

impl Packet {
    pub(crate) fn state(seq: u8, flags: u8, state: GamepadState) -> Self {
        Self {
            kind: PacketKind::State(state),
            seq,
            flags,
        }
    }

    pub(crate) fn heartbeat(seq: u8, flags: u8, state: GamepadState) -> Self {
        Self {
            kind: PacketKind::Heartbeat(state),
            seq,
            flags,
        }
    }

    pub(crate) fn key_state(seq: u8, flags: u8, report: KeyboardReport) -> Self {
        Self {
            kind: PacketKind::KeyState(report),
            seq,
            flags,
        }
    }

    pub(crate) fn key_heartbeat(seq: u8, flags: u8, report: KeyboardReport) -> Self {
        Self {
            kind: PacketKind::KeyHeartbeat(report),
            seq,
            flags,
        }
    }

    pub(crate) fn discover(seq: u8) -> Self {
        Self {
            kind: PacketKind::Discover,
            seq,
            flags: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn ack(seq: u8, info: AckInfo) -> Self {
        Self {
            kind: PacketKind::Ack(info),
            seq,
            flags: 0,
        }
    }

    pub(crate) fn encode(&self) -> [u8; PACKET_SIZE] {
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
        let body: &mut [u8; 12] = (&mut buf[4..16])
            .try_into()
            .expect("bytes 4..16 of a packet are 12 bytes");
        match self.kind {
            PacketKind::State(st) | PacketKind::Heartbeat(st) => {
                write_state(body, &st);
            }
            PacketKind::KeyState(kr) | PacketKind::KeyHeartbeat(kr) => {
                write_key(body, kr);
            }
            PacketKind::Discover => {}
            PacketKind::Ack(info) => {
                body[0] = info.proto_version;
                body[1] = info.fw_major;
                body[2] = info.fw_minor;
                body[3] = info.fw_patch;
                body[4] = info.board_type;
                let up = info.uptime_seconds.to_le_bytes();
                body[5] = up[0];
                body[6] = up[1];
                body[7] = up[2];
                body[8..12].copy_from_slice(&info.unique_id_short.to_le_bytes());
            }
        }
        buf[16] = crc8(&buf[..16]);
        buf
    }

    pub(crate) fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
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
        let body: &[u8; 12] = buf[4..16]
            .try_into()
            .expect("bytes 4..16 of a packet are 12 bytes");
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

fn write_key(body: &mut [u8; 12], rep: KeyboardReport) {
    body[0] = rep.modifiers;
    body[1] = 0;
    body[2..8].copy_from_slice(&rep.keys);
}

fn read_key(body: &[u8; 12]) -> KeyboardReport {
    let mut keys = [0u8; 6];
    keys.copy_from_slice(&body[2..8]);
    KeyboardReport {
        modifiers: body[0],
        keys,
    }
}

#[cfg(test)]
pub(crate) fn seq_is_newer(new: u8, old: u8) -> bool {
    new.wrapping_sub(old) != 0 && new.wrapping_sub(old) < 128
}

pub(crate) fn usb_in_blocked_reason_label(reason: u8) -> &'static str {
    match reason {
        USB_DIAG_IN_BLOCKED_NONE => "none",
        USB_DIAG_IN_BLOCKED_NOT_MOUNTED => "not_mounted",
        USB_DIAG_IN_BLOCKED_NOT_READY => "not_ready",
        USB_DIAG_IN_BLOCKED_SHORT_WRITE => "short_write",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests;
