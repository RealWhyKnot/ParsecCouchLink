use super::DiagSource;
use crate::cmd_bundle::usb_enum::{setup_probe_failed_diagnosis, stub_failure};
use std::net::SocketAddr;

pub(super) fn captured_stub(text: &str, lost: u32, source: &DiagSource) -> String {
    let prefix = format!("--- captured via {}", source.describe());
    let prefix = if lost > 0 {
        format!("{prefix}; {lost} byte(s) dropped from the ring before this snapshot ---\n")
    } else {
        format!("{prefix} ---\n")
    };
    format!("{prefix}{text}")
}

pub(super) fn empty_stub(source: &DiagSource) -> String {
    stub_failure(
        "Pico answered, but its diag ring was empty.",
        &[
            "Re-run the failing command and immediately run bundle while \
             the Pico is still in the same state. If the failure was at \
             boot, the reboot between the failure and bundle wiped the \
             in-RAM ring.",
            "If this is reproducible, attach a bug report -- an \
             answering-but-empty Pico is unusual.",
        ],
        &[("source", &source.describe())],
    )
}

pub(super) fn no_setup_port_stub() -> String {
    stub_failure(
        "No setup-mode Pico found, and no last-known run-mode Pico to \
         fall back to.",
        &[
            "Unplug the Pico. Hold BOOTSEL while plugging it back in. \
             Wait until Windows shows a RPI-RP2 or RP2350 drive in File \
             Explorer.",
            "Run `couchlink.exe flash` to copy the matching UF2 onto \
             the drive.",
            "Once the Pico reboots into setup mode (it should appear as \
             a new COM port within ~5 seconds), re-run this bundle.",
            "If no COM port shows up at all, try a different micro-USB \
             DATA cable (charge-only cables fail) or a different USB \
             port on the PC.",
        ],
        &[("looking_for_vid_pid", "0x2E8A:0xCAF0")],
    )
}

pub(super) fn setup_open_failed_stub(error: &str) -> String {
    stub_failure(
        "Pico is enumerated, but the bridge could not open its COM port.",
        &[
            "Another application is probably holding the port. Close any \
             open serial terminals (PuTTY, Tera Term, Arduino Serial \
             Monitor, screen, minicom) and re-run.",
            "If no app is open, unplug + replug the Pico and re-run.",
            "If the error mentions ACCESS_DENIED specifically, a Windows \
             driver may have failed to bind; check Device Manager for a \
             yellow exclamation mark on the Pico's entry.",
        ],
        &[("error", error)],
    )
}

pub(super) fn setup_probe_failed_stub(
    port: &str,
    step: &str,
    elapsed_ms: u128,
    bytes_received: usize,
    rx_first_32_hex: &str,
    error: &str,
) -> String {
    let (root, steps) = setup_probe_failed_diagnosis(step, bytes_received);
    stub_failure(
        root,
        steps,
        &[
            ("port", port),
            ("step", step),
            ("elapsed_ms", &elapsed_ms.to_string()),
            ("bytes_received", &bytes_received.to_string()),
            ("rx_first_32_hex", rx_first_32_hex),
            ("error", error),
        ],
    )
}

pub(super) fn no_last_pico_in_config_stub() -> String {
    stub_failure(
        "No setup-mode Pico found, and no run-mode Pico has ever been \
         seen by this bridge installation.",
        &[
            "Run `couchlink setup` to provision a Pico (flash + Wi-Fi).",
            "Or, if a Pico is already running on your LAN, run \
             `couchlink bundle` from the bridge PC and choose \
             `Enter Pico IP manually` from the menu if broadcast \
             discovery is blocked.",
        ],
        &[],
    )
}

pub(super) fn vendor_not_found_stub() -> String {
    stub_failure(
        "No Pico with a diag-vendor interface (WinUSB-bound) is \
         currently enumerated. Either the Pico is in run mode (no \
         diag interface present) or its firmware predates the \
         WinUSB diag channel.",
        &[
            "If the Pico is in run mode, retrieval falls through to \
             UDP automatically; this stub means UDP also did not \
             succeed -- see the UDP entries for diagnostics.",
            "If the Pico is in setup mode but no diag interface is \
             visible, the firmware predates the diag-vendor \
             interface. Reflash with the matching couchlink-*.uf2.",
        ],
        &[("looking_for_vid_pid", "0x2E8A:0xCAF0 + vendor interface")],
    )
}

pub(super) fn vendor_open_failed_stub(error: &str) -> String {
    stub_failure(
        "Found a Pico with a diag-vendor interface but could not \
         claim it via WinUSB.",
        &[
            "Another process may be holding the diag interface. Close \
             any running couchlink instances and re-run bundle.",
            "If Windows shows the diag interface as 'driver not \
             loaded' in Device Manager, the MS OS 2.0 descriptor \
             binding may have failed. Unplug and replug the Pico; \
             Windows re-evaluates WinUSB binding on enumeration.",
        ],
        &[("error", error)],
    )
}

pub(super) fn vendor_transfer_failed_stub(
    step: &str,
    bytes_received: usize,
    error: &str,
) -> String {
    stub_failure(
        "Vendor control transfer to retrieve the diag log failed.",
        &[
            "Re-run bundle. Control transfers occasionally fail under \
             bus glitches; a retry usually succeeds.",
            "If the error mentions PIPE or STALL, the firmware did \
             not recognise the vendor request -- the bridge and \
             firmware may be on mismatched protocol versions. \
             Reflash with the matching couchlink-*.uf2.",
        ],
        &[
            ("step", step),
            ("bytes_received", &bytes_received.to_string()),
            ("error", error),
        ],
    )
}

pub(super) fn udp_discovery_failed_stub(reason: &str) -> String {
    stub_failure(
        "Bundle tried a run-mode UDP probe but the Pico did not answer.",
        &[
            "Confirm the Pico is powered on, plugged into the USB4MAPLE \
             (or equivalent), and within Wi-Fi range of the AP it was \
             provisioned against.",
            "If you have changed Wi-Fi networks since setup, the saved \
             credentials are now stale. Hold BOOTSEL for 3+ seconds \
             during plug-in to wipe the saved creds, then re-run \
             `couchlink setup`.",
            "If you have multiple network adapters, make sure the bridge \
             is allowed through Windows Firewall on the active profile. \
             `couchlink bundle` includes the firewall and network snapshot.",
        ],
        &[("error", reason)],
    )
}

pub(super) fn udp_probe_failed_stub(
    peer: SocketAddr,
    step: &str,
    elapsed_ms: u128,
    chunks_received: u16,
    error: &str,
) -> String {
    stub_failure(
        "Pico answered initial discovery but the CMD_GET_LOG exchange \
         did not complete.",
        &[
            "The Pico is alive on the LAN -- it answered discovery -- \
             but either the request did not reach it or its reply did \
             not reach us. Run `couchlink bundle` again; transient \
             packet loss is the most likely cause.",
            "If it persists across multiple bundle attempts, check \
             whether anything on the network is doing aggressive packet \
             inspection (some corporate firewalls drop unknown UDP \
             types).",
        ],
        &[
            ("peer", &peer.to_string()),
            ("step", step),
            ("elapsed_ms", &elapsed_ms.to_string()),
            ("chunks_received", &chunks_received.to_string()),
            ("error", error),
        ],
    )
}

pub(super) fn udp_unsupported_stub(peer: SocketAddr) -> String {
    stub_failure(
        "Pico is reachable on the LAN but is running pre-LogChunk \
         firmware.",
        &[
            "The Pico answered discovery, but its ACK does not advertise \
             the LogChunk capability bit. The firmware predates the \
             run-mode diag pull.",
            "Hold BOOTSEL while plugging the Pico into this PC, then \
             flash with `couchlink.exe flash`. The new firmware \
             advertises the bit and the next bundle will UDP-pull diag \
             automatically.",
            "If you cannot reflash right now, the bridge log at \
             %LOCALAPPDATA%\\ParsecCouchLink\\data\\logs has \
             bridge-side observations that do not depend on the firmware.",
        ],
        &[("peer", &peer.to_string())],
    )
}
