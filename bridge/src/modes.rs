#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PicoScope {
    Selected,
    All,
}

impl PicoScope {
    pub(crate) const fn from_all_flag(all: bool) -> Self {
        if all { Self::All } else { Self::Selected }
    }

    pub(crate) const fn is_all(self) -> bool {
        matches!(self, Self::All)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterSwitch {
    Stream,
    Stop,
}

impl AfterSwitch {
    pub(crate) const fn from_no_stream_flag(no_stream: bool) -> Self {
        if no_stream { Self::Stop } else { Self::Stream }
    }

    pub(crate) const fn streams(self) -> bool {
        matches!(self, Self::Stream)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootselEntry {
    OverUsb,
    Manual,
}

impl BootselEntry {
    pub(crate) const fn from_usb_flag(from_usb: bool) -> Self {
        if from_usb {
            Self::OverUsb
        } else {
            Self::Manual
        }
    }
}
