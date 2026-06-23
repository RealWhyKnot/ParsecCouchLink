use super::{MAGIC, TYPE_LOG_CHUNK};

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
        buf.push(0);
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
