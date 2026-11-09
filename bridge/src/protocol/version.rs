use super::wire::crc8;
use super::{DecodeError, MAGIC, PACKET_SIZE, TYPE_VERSION};
use crate::firmware_version::FirmwareVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VersionInfo {
    pub version: FirmwareVersion,
}

impl VersionInfo {
    #[cfg(test)]
    pub(crate) fn encode(&self, seq: u8, flags: u8) -> [u8; PACKET_SIZE] {
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
            let body: &mut [u8; 12] = (&mut buf[4..16])
                .try_into()
                .expect("bytes 4..16 of a packet are 12 bytes");
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

    #[cfg(test)]
    pub(crate) fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        Ok(Self::decode_with_header(buf)?.2)
    }

    pub(crate) fn decode_with_header(buf: &[u8]) -> Result<(u8, u8, Self), DecodeError> {
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
        let body: &[u8; 12] = buf[4..16]
            .try_into()
            .expect("bytes 4..16 of a packet are 12 bytes");
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
            if suffix.iter().all(u8::is_ascii_alphanumeric) {
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
