use super::{MAGIC, PACKET_SIZE};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DecodeError {
    WrongSize,
    WrongMagic,
    BadCrc,
    UnknownType(u8),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongSize => write!(f, "wrong size: expected {PACKET_SIZE} bytes"),
            Self::WrongMagic => write!(f, "wrong magic: expected 0x{MAGIC:02X}"),
            Self::BadCrc => write!(f, "CRC-8 mismatch"),
            Self::UnknownType(t) => write!(f, "unknown packet type 0x{t:02X}"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UsbDiagDecodeError {
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
pub(crate) enum PicoStateDecodeError {
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
