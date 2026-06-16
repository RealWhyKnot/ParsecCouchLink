//! WinUSB vendor-control transfer for retrieving the firmware diag log.
//!
//! The Pico's setup-mode composite carries a vendor-class interface
//! (DIAG_ITF_NUM = 2) that Windows binds to WinUSB via MS OS 2.0
//! descriptors. The diag log ring is exposed via a vendor IN control
//! transfer on EP0, independent of the CDC bulk endpoint state -- so
//! this path works even when the CDC FIFO is wedged.
//!
//! Response payload matches the CDC CMD_GET_LOG_BUFFER wire format:
//! `[lost_le32][raw_log_bytes]`.

use std::time::Duration;

// Constants must match `pico-bridge/src/usb_descriptors.c`.
const PICO_VID: u16 = 0x2E8A;
const PICO_PID: u16 = 0xCAF0;
const DIAG_ITF_NUM: u8 = 2;
const DIAG_GET_LOG_REQ: u8 = 0x01;
const MAX_RESPONSE_BYTES: usize = 4 + 16384;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);

/// Result of a vendor-control diag-log fetch. Translated to
/// `DiagOutcome` by the caller in `cmd_bundle.rs`.
#[derive(Debug)]
pub enum VendorDiagOutcome {
    Captured {
        text: String,
        lost: u32,
    },
    Empty,
    NotFound,
    OpenFailed {
        error: String,
    },
    TransferFailed {
        step: &'static str,
        bytes_received: usize,
        error: String,
    },
}

/// Blocking implementation; intended to be called inside
/// `tokio::task::spawn_blocking`.
pub fn capture_diag_blocking() -> VendorDiagOutcome {
    let info = match find_pico_diag_device() {
        Ok(Some(i)) => i,
        Ok(None) => return VendorDiagOutcome::NotFound,
        Err(e) => {
            return VendorDiagOutcome::OpenFailed {
                error: format!("list_devices: {e}"),
            };
        }
    };

    let device = match info.open() {
        Ok(d) => d,
        Err(e) => {
            return VendorDiagOutcome::OpenFailed {
                error: format!("open: {e}"),
            };
        }
    };

    let iface = match device.claim_interface(DIAG_ITF_NUM) {
        Ok(i) => i,
        Err(e) => {
            return VendorDiagOutcome::OpenFailed {
                error: format!("claim_interface({DIAG_ITF_NUM}): {e}"),
            };
        }
    };

    let control = nusb::transfer::Control {
        control_type: nusb::transfer::ControlType::Vendor,
        recipient: nusb::transfer::Recipient::Interface,
        request: DIAG_GET_LOG_REQ,
        value: 0,
        index: u16::from(DIAG_ITF_NUM),
    };

    let mut buf = vec![0u8; MAX_RESPONSE_BYTES];
    let n = match iface.control_in_blocking(control, &mut buf, TRANSFER_TIMEOUT) {
        Ok(n) => n,
        Err(e) => {
            return VendorDiagOutcome::TransferFailed {
                step: "control_in",
                bytes_received: 0,
                error: format!("{e}"),
            };
        }
    };

    if n < 4 {
        return VendorDiagOutcome::TransferFailed {
            step: "parse",
            bytes_received: n,
            error: format!("response too short: {n} bytes (need >= 4 for lost-bytes header)"),
        };
    }

    let lost = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let log_bytes = &buf[4..n];
    let text = String::from_utf8_lossy(log_bytes).to_string();

    if text.is_empty() {
        VendorDiagOutcome::Empty
    } else {
        VendorDiagOutcome::Captured { text, lost }
    }
}

fn find_pico_diag_device() -> Result<Option<nusb::DeviceInfo>, std::io::Error> {
    let mut found = None;
    for info in nusb::list_devices()? {
        if info.vendor_id() == PICO_VID && info.product_id() == PICO_PID {
            found = Some(info);
            break;
        }
    }
    Ok(found)
}
