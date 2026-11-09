use serde::{Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Capture {
    Captured,
    #[default]
    Missing,
}

impl Capture {
    pub(super) const fn is_captured(self) -> bool {
        matches!(self, Self::Captured)
    }
}

impl From<bool> for Capture {
    fn from(captured: bool) -> Self {
        if captured {
            Self::Captured
        } else {
            Self::Missing
        }
    }
}

impl Serialize for Capture {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(self.is_captured())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Inclusion {
    Included,
    Omitted,
}

impl Inclusion {
    pub(super) const fn is_included(self) -> bool {
        matches!(self, Self::Included)
    }
}

impl From<bool> for Inclusion {
    fn from(included: bool) -> Self {
        if included {
            Self::Included
        } else {
            Self::Omitted
        }
    }
}

impl Serialize for Inclusion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(self.is_included())
    }
}
