use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FirmwareVersion {
    Release {
        year: u16,
        month: u8,
        day: u8,
        revision: Option<u8>,
        suffix: Option<[u8; 4]>,
    },
    Legacy {
        major: u8,
        minor: u8,
        patch: u8,
    },
}

impl FirmwareVersion {
    pub(crate) fn from_triplet(major: u8, minor: u8, patch: u8) -> Self {
        if (20..=99).contains(&major) && (1..=12).contains(&minor) && (1..=31).contains(&patch) {
            Self::Release {
                year: 2000 + u16::from(major),
                month: minor,
                day: patch,
                revision: None,
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

    pub(crate) fn from_hello_payload(payload: &[u8]) -> Self {
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
                    revision: Some(revision),
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
        if suffix.iter().all(u8::is_ascii_alphanumeric) {
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
                match revision {
                    Some(revision) => {
                        write!(f, "{year}.{month}.{day}.{revision}")?;
                        if let Some(suffix) = suffix {
                            let Ok(suffix) = std::str::from_utf8(&suffix) else {
                                return Err(fmt::Error);
                            };
                            write!(f, "-{suffix}")?;
                        }
                    }
                    None => write!(f, "{year}.{month}.{day}.x")?,
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

    #[test]
    fn from_triplet_gates_release_vs_legacy() {
        assert_eq!(
            FirmwareVersion::from_triplet(26, 5, 30).to_string(),
            "2026.5.30.x"
        );
        assert_eq!(
            FirmwareVersion::from_triplet(19, 5, 30).to_string(),
            "19.5.30"
        );
        assert_eq!(
            FirmwareVersion::from_triplet(26, 13, 1).to_string(),
            "26.13.1"
        );
        assert_eq!(
            FirmwareVersion::from_triplet(26, 5, 32).to_string(),
            "26.5.32"
        );
    }

    #[test]
    fn release_display_includes_revision_and_suffix() {
        let v = FirmwareVersion::Release {
            year: 2026,
            month: 5,
            day: 30,
            revision: Some(7),
            suffix: Some(*b"D69A"),
        };
        assert_eq!(v.to_string(), "2026.5.30.7-D69A");

        let no_suffix = FirmwareVersion::Release {
            year: 2026,
            month: 5,
            day: 30,
            revision: Some(0),
            suffix: None,
        };
        assert_eq!(no_suffix.to_string(), "2026.5.30.0");

        let unknown_build = FirmwareVersion::Release {
            year: 2026,
            month: 5,
            day: 30,
            revision: None,
            suffix: None,
        };
        assert_eq!(unknown_build.to_string(), "2026.5.30.x");
    }
}
