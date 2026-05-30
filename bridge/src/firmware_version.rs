use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirmwareVersion {
    Release {
        year: u16,
        month: u8,
        day: u8,
        revision: u8,
        suffix: Option<[u8; 4]>,
    },
    Legacy {
        major: u8,
        minor: u8,
        patch: u8,
    },
}

impl FirmwareVersion {
    pub fn from_triplet(major: u8, minor: u8, patch: u8) -> Self {
        if (20..=99).contains(&major) && (1..=12).contains(&minor) && (1..=31).contains(&patch) {
            Self::Release {
                year: 2000 + major as u16,
                month: minor,
                day: patch,
                revision: 0,
                suffix: None,
            }
        } else {
            Self::Legacy {
                major,
                minor,
                patch,
            }
        }
    }

    pub fn from_hello_payload(payload: &[u8]) -> Self {
        if payload.len() >= 11 {
            let year = u16::from_le_bytes([payload[6], payload[7]]);
            let month = payload[8];
            let day = payload[9];
            let revision = payload[10];
            if (2020..=2099).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
            {
                let suffix = Self::payload_suffix(payload);
                return Self::Release {
                    year,
                    month,
                    day,
                    revision,
                    suffix,
                };
            }
        }
        if payload.len() >= 4 {
            Self::from_triplet(payload[1], payload[2], payload[3])
        } else {
            Self::Legacy {
                major: 0,
                minor: 0,
                patch: 0,
            }
        }
    }

    fn payload_suffix(payload: &[u8]) -> Option<[u8; 4]> {
        if payload.len() < 16 || payload[11] != 4 {
            return None;
        }
        let mut suffix = [0u8; 4];
        suffix.copy_from_slice(&payload[12..16]);
        if suffix.iter().all(|b| b.is_ascii_alphanumeric()) {
            Some(suffix)
        } else {
            None
        }
    }
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Release {
                year,
                month,
                day,
                revision,
                suffix,
            } => {
                write!(f, "{year}.{month}.{day}.{revision}")?;
                if let Some(suffix) = suffix {
                    let suffix = std::str::from_utf8(&suffix).map_err(|_| fmt::Error)?;
                    write!(f, "-{suffix}")?;
                }
                Ok(())
            }
            Self::Legacy {
                major,
                minor,
                patch,
            } => write!(f, "{major}.{minor}.{patch}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FirmwareVersion;

    #[test]
    fn parses_release_hello_with_suffix() {
        let payload = [
            1, 26, 5, 30, 1, 0, 0xEA, 0x07, 5, 30, 7, 4, b'D', b'6', b'9', b'A',
        ];

        assert_eq!(
            FirmwareVersion::from_hello_payload(&payload).to_string(),
            "2026.5.30.7-D69A"
        );
    }

    #[test]
    fn parses_release_hello_without_suffix() {
        let payload = [1, 26, 5, 30, 1, 0, 0xEA, 0x07, 5, 30, 0, 0];

        assert_eq!(
            FirmwareVersion::from_hello_payload(&payload).to_string(),
            "2026.5.30.0"
        );
    }

    #[test]
    fn keeps_legacy_triplets_for_non_date_versions() {
        let payload = [1, 1, 2, 3, 1, 0];

        assert_eq!(
            FirmwareVersion::from_hello_payload(&payload).to_string(),
            "1.2.3"
        );
    }
}
