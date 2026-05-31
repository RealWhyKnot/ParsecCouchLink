//! USB enumeration capture (pnputil / serialport / event log) plus the
//! topology-aware diag stubs that depend on what Windows currently sees
//! on the bus.

/// Generate the pico-diag.txt body for `DiagOutcome::VendorNotFound`,
/// gated on what the pnputil snapshot shows for 0x2E8A:0xCAF0. The three
/// branches match `PicoEnumState` (NotEnumerated, EnumeratedRunMode, and
/// the setup-mode-shaped but unclaimable case). The generic static text
/// in the `VendorNotFound` match arm is replaced at write time by this
/// function so the instructions match what Windows actually sees.
pub(super) fn vendor_not_found_stub_text(state: &PicoEnumState) -> String {
    match state {
        PicoEnumState::NotEnumerated => stub_failure(
            "No Pico (VID_2E8A:PID_CAF0) is currently enumerated on USB.",
            &[
                "Try a different micro-USB DATA cable -- charge-only cables enumerate \
                 USB power but carry no data.",
                "Try a different USB port on the PC (prefer a port on the motherboard, \
                 not a hub).",
                "Hold BOOTSEL for 5+ seconds while replugging to wipe creds and force \
                 setup mode.",
            ],
            &[("looking_for_vid_pid", "0x2E8A:0xCAF0")],
        ),
        PicoEnumState::EnumeratedRunMode => stub_failure(
            "The Pico is in run mode. Run-mode firmware does not expose a USB diag \
             interface -- the vendor interface exists only in setup mode.",
            &[
                "Wait ~30 s for the Wi-Fi association watchdog to auto-bounce the Pico \
                 back to setup mode if Wi-Fi association is failing, then run \
                 `couchlink bundle` again.",
                "Hold BOOTSEL briefly (under 2 s) while replugging to force setup mode \
                 without wiping creds.",
                "Hold BOOTSEL for 3+ s to force setup mode AND wipe creds.",
            ],
            &[(
                "looking_for_vid_pid",
                "0x2E8A:0xCAF0 (run mode, no vendor interface)",
            )],
        ),
        PicoEnumState::EnumeratedParentOnly => parent_only_stub_text(),
        PicoEnumState::EnumeratedSetupMode
        | PicoEnumState::EnumeratedButInterfaceUnclaimable { .. } => stub_failure(
            "Found a Pico with a diag-vendor interface but could not claim it via WinUSB.",
            &[
                "Another process may be holding the diag interface. Close any running \
                 couchlink instances and re-run bundle.",
                "If Windows shows the diag interface as 'driver not loaded' in Device \
                 Manager, the MS OS 2.0 descriptor binding may have failed. Unplug and \
                 replug the Pico; Windows re-evaluates WinUSB binding on enumeration.",
            ],
            &[("looking_for_vid_pid", "0x2E8A:0xCAF0 + vendor interface")],
        ),
    }
}

pub(super) fn parent_only_stub_text() -> String {
    stub_failure(
        "Windows sees the Pico USB parent device, but no usable CDC, WinUSB, or \
         XInput child interface is currently bound.",
        &[
            "Unplug and replug the Pico, then re-run `couchlink bundle`; Windows may \
             complete child-interface binding on a clean enumeration.",
            "If this happened immediately after a UF2 flash, flash firmware that forces \
             setup mode on UF2 reflash so old saved credentials cannot bypass CDC setup.",
            "If the parent-only state persists, collect Windows Device Manager details \
             for the Pico entry; the configuration descriptor or driver bind failed \
             before any command path could reach firmware diagnostics.",
        ],
        &[("enumerated_device", "USB\\VID_2E8A&PID_CAF0 parent only")],
    )
}

/// Format a self-diagnosing stub body. Leads with a one-sentence root
/// cause, then a numbered "Try this" list, then a `Diagnostic details`
/// block with the captured fields verbatim.
pub(super) fn stub_failure(root_cause: &str, steps: &[&str], fields: &[(&str, &str)]) -> String {
    let mut out = String::new();
    out.push_str("=== Suggested next step ===\n");
    out.push_str(root_cause);
    out.push('\n');
    out.push('\n');
    out.push_str("Try this (in order):\n");
    for (i, s) in steps.iter().enumerate() {
        out.push_str(&format!("  {}. {}\n", i + 1, soft_wrap(s, 78, "     ")));
    }
    if !fields.is_empty() {
        out.push('\n');
        out.push_str("=== Diagnostic details ===\n");
        for (k, v) in fields {
            out.push_str(&format!("  {k}: {v}\n"));
        }
    }
    out
}

/// Wraps `text` so each line stays under `width` columns; continuation
/// lines are indented by `indent`. Whitespace-only sequences in the
/// input become single spaces.
fn soft_wrap(text: &str, width: usize, indent: &str) -> String {
    let mut out = String::new();
    let mut line_len = 0;
    for word in text.split_whitespace() {
        if line_len == 0 {
            out.push_str(word);
            line_len = word.len();
            continue;
        }
        if line_len + 1 + word.len() > width {
            out.push('\n');
            out.push_str(indent);
            out.push_str(word);
            line_len = indent.len() + word.len();
        } else {
            out.push(' ');
            out.push_str(word);
            line_len += 1 + word.len();
        }
    }
    out
}

/// Map a HELLO-probe failure shape to root cause + remediation list.
/// Most of the diagnostic value of the new bundle is concentrated
/// here: when the operator opens pico-diag.txt this is what they read
/// first.
pub(super) fn setup_probe_failed_diagnosis(
    step: &str,
    bytes_received: usize,
) -> (&'static str, &'static [&'static str]) {
    static WRITE: &[&str] = &[
        "Unplug the Pico and plug it back in (no BOOTSEL).",
        "If the COM port re-appears but the bridge still fails, the USB \
         serial driver (usbser.sys) may be in a bad state. Reboot Windows.",
        "If it still fails, try a different USB port -- preferably one on \
         the motherboard rather than through a hub.",
    ];
    static READ_NO_BYTES: &[&str] = &[
        "The firmware enumerated USB (Windows sees the COM port) but is not \
         responding to commands. The most common cause is a fault during \
         firmware init -- the CDC stack is up enough to enumerate, but the \
         main poll loop never started.",
        "Hold BOOTSEL while plugging the Pico in. Run `flash.ps1` to write \
         a fresh UF2. Re-run setup. The new firmware writes a `boot: \
         reset-reason=fault` line on the next boot if it crashed; the \
         bundle then captures WHY it crashed via the new fault context \
         (PC, LR, xPSR, R0-R3, R12, SP, CFSR on RP2350).",
        "If a fresh flash still fails: try a different micro-USB DATA cable \
         (charge-only cables enumerate USB but fail data transfers), or a \
         different USB port.",
        "If still failing after cable + port + reflash, this is worth a bug \
         report. Attach this bundle.",
    ];
    static READ_SOME_BYTES: &[&str] = &[
        "The firmware is alive and writing bytes on the wire, but those \
         bytes are not a valid HELLO_ACK frame. The hex preview above \
         shows what it actually said. The most common cause is a version \
         mismatch between the bridge and the firmware UF2.",
        "Reflash the Pico with the couchlink-*.uf2 from the SAME release \
         as the couchlink.exe you are running. Mixing v2026.5.16.x \
         firmware with v2026.5.16.y bridge is the canonical cause of this \
         exact shape.",
        "If the hex preview looks like ASCII text (e.g. starts with `54 \
         75` = \"Tu...\"), the firmware may be writing diag log lines \
         directly to CDC instead of framed responses. That is a firmware \
         bug; attach this bundle.",
    ];
    static DECODE: &[&str] = &[
        "Bytes arrived but the frame header is malformed -- wrong magic, \
         wrong CRC, or wrong opcode. Almost always a protocol-version \
         mismatch.",
        "Make sure the bridge .exe and the firmware .uf2 came from the \
         same release zip. Re-download the release if unsure.",
    ];
    static GET_LOG: &[&str] = &[
        "HELLO succeeded but the follow-up GET_LOG_BUFFER call failed. The \
         firmware is responding to commands but not to this one \
         specifically.",
        "Most likely the Pico rebooted between HELLO and GET_LOG_BUFFER. \
         Re-run bundle; it should retry against the post-reboot state.",
        "If it persists, this is worth a bug report.",
    ];
    if step == "write" {
        (
            "Bridge could not write the HELLO frame to the firmware.",
            WRITE,
        )
    } else if step == "read" && bytes_received == 0 {
        (
            "Firmware enumerated USB but did not write a single byte back \
             during the 10-second probe.",
            READ_NO_BYTES,
        )
    } else if step == "read" {
        (
            "Firmware is alive on the wire but its bytes did not parse as a \
             HELLO_ACK frame.",
            READ_SOME_BYTES,
        )
    } else if step == "decode" {
        (
            "Bytes arrived but did not decode as a valid framed response.",
            DECODE,
        )
    } else if step == "get_log_buffer" {
        (
            "HELLO succeeded but the diag-log fetch did not complete.",
            GET_LOG,
        )
    } else {
        (
            "HELLO probe failed at an unexpected step.",
            &["This shape was not anticipated by the self-diagnosis. \
                 Attach this bundle to a bug report; the captured fields \
                 below carry enough detail to diagnose offline."],
        )
    }
}

/// Classification of the Pico's current USB enumeration state, derived
/// from pnputil output. Used to gate `VendorNotFound` stub text on what
/// Windows actually sees on the bus.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PicoEnumState {
    /// No entry with VID_2E8A&PID_CAF0 in pnputil output.
    NotEnumerated,
    /// VID_2E8A&PID_CAF0 present with both MI_00 (CDC) and MI_02 (vendor).
    /// The vendor interface should be WinUSB-bound in setup mode.
    EnumeratedSetupMode,
    /// VID_2E8A&PID_CAF0 parent is present, but Windows has not exposed any
    /// child interface. This is neither healthy setup mode nor healthy run mode.
    EnumeratedParentOnly,
    /// VID_2E8A&PID_CAF0 present but no MI_02 / vendor interface found.
    /// Run-mode firmware presents only the XInput composite without a
    /// vendor interface; xusb22.sys being bound is a secondary indicator.
    EnumeratedRunMode,
    /// Setup-mode-shaped device found but the diag_usb open call failed.
    #[allow(dead_code)]
    EnumeratedButInterfaceUnclaimable { reason: String },
}

/// Parse pnputil /enum-devices text to determine how the Pico is
/// currently enumerated. The function does not require all blocks to be
/// present; it looks for specific Instance ID patterns.
///
/// Setup mode: the composite parent VID_2E8A&PID_CAF0\... plus a child
/// with &MI_02 (the WinUSB vendor interface). Older run-mode builds used
/// the same parent with non-diag child interfaces. Parent-only means Windows
/// saw the device descriptor but did not expose any interface child, so it is
/// tracked separately from a healthy run-mode shape.
pub(super) fn classify_pico_enum(pnputil_text: &str) -> PicoEnumState {
    // Check for the parent device.
    let has_parent = pnputil_text
        .lines()
        .any(|l| l.contains("VID_2E8A") && l.contains("PID_CAF0") && !l.contains("&MI_"));

    if !has_parent {
        return PicoEnumState::NotEnumerated;
    }

    // Check for the MI_02 vendor interface (setup mode only).
    let has_vendor_itf = pnputil_text
        .lines()
        .any(|l| l.contains("VID_2E8A") && l.contains("PID_CAF0") && l.contains("&MI_02"));

    if has_vendor_itf {
        PicoEnumState::EnumeratedSetupMode
    } else if !pnputil_text
        .lines()
        .any(|l| l.contains("VID_2E8A") && l.contains("PID_CAF0") && l.contains("&MI_"))
    {
        PicoEnumState::EnumeratedParentOnly
    } else {
        // Parent and at least one child present, but no vendor interface --
        // old run-mode firmware shape.
        PicoEnumState::EnumeratedRunMode
    }
}

/// Capture a USB device enumeration. On Windows we first try
/// `pnputil /enum-devices /class USB /connected` (Win10 1903+); on
/// older Windows or non-Windows hosts we fall back to a serialport
/// list dump that at least names every USB serial device with VID,
/// PID, manufacturer, and serial number. Returns `(text, method)`
/// or `None` if both paths failed.
pub(super) async fn capture_usb_devices() -> Option<(String, &'static str)> {
    #[cfg(windows)]
    {
        if let Some(text) = pnputil_enum_usb().await {
            return Some((text, "pnputil"));
        }
        tracing::debug!("bundle: pnputil enum failed, falling back to serialport list");
    }
    let text = tokio::task::spawn_blocking(serialport_list_dump)
        .await
        .ok()??;
    Some((text, "serialport-fallback"))
}

/// Last 15 minutes of OS-level USB events from the Windows event log.
/// Catches driver bind failures, surprise removals, and descriptor
/// request timeouts -- none of which surface in pnputil's snapshot.
/// Best-effort: a long-running event log query, a missing PowerShell,
/// or a permissions denial all return `None` and the bundle records
/// that the capture failed in the manifest.
#[cfg(windows)]
pub(super) async fn capture_windows_usb_events() -> Option<String> {
    // Get-WinEvent's `-FilterHashtable` is documented to fail with an
    // unhelpful "No events were found" message when the filter matches
    // nothing -- which is normal on a quiet system. Catch that branch
    // and return an empty string rather than `None` so the bundle
    // header makes the "uneventful" case obvious to the operator.
    //
    // The query is split: System log gets the usbhub / usbser drivers,
    // and the Kernel-PnP/Configuration log catches the higher-level
    // bind events. PS 5.1 syntax: no `&&` chaining, no ternary.
    let ps_cmd = r#"
$ErrorActionPreference = 'SilentlyContinue'
$start = (Get-Date).AddMinutes(-15)
$events = @()
$sys = Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=$start} -MaxEvents 200 2>$null
if ($sys) {
    $events += $sys | Where-Object { $_.ProviderName -match '(?i)usb|usbser|usbhub|pnp' }
}
$pnp = Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Kernel-PnP/Configuration'; StartTime=$start} -MaxEvents 100 2>$null
if ($pnp) { $events += $pnp }
if (-not $events -or $events.Count -eq 0) {
    Write-Output '(no matching events in the last 15 minutes)'
    exit 0
}
$events |
    Sort-Object TimeCreated |
    ForEach-Object {
        $msg = if ($_.Message) { $_.Message.Trim() } else { '' }
        Write-Output ('[' + $_.TimeCreated.ToString('yyyy-MM-ddTHH:mm:ss.fff') + '] ' + $_.LevelDisplayName + ' ' + $_.ProviderName + '/' + $_.Id)
        Write-Output ('  ' + ($msg -replace "`r?`n", "`n  "))
        Write-Output ''
    }
"#;
    // 30-second cap: Get-WinEvent against System on a busy machine can
    // be slow, and we'd rather skip than hang the bundle indefinitely.
    let fut = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_cmd])
        .output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            tracing::debug!("bundle: powershell spawn for usb events failed: {e}");
            return None;
        }
        Err(_) => {
            tracing::debug!("bundle: usb events query timed out after 30 s");
            return None;
        }
    };
    if !out.status.success() {
        tracing::debug!(
            "bundle: powershell exit {} for usb events",
            out.status.code().unwrap_or(-1)
        );
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
pub(super) async fn capture_windows_usb_events() -> Option<String> {
    None
}

#[cfg(windows)]
async fn pnputil_enum_usb() -> Option<String> {
    let out = tokio::process::Command::new("pnputil.exe")
        .args(["/enum-devices", "/class", "USB", "/connected"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        // Some Win10 1903-1909 builds reject /connected unelevated.
        // Try the looser /enum-devices /class USB (no /connected) as a
        // last resort before declaring the path unusable.
        let fallback = tokio::process::Command::new("pnputil.exe")
            .args(["/enum-devices", "/class", "USB"])
            .output()
            .await
            .ok()?;
        if !fallback.status.success() {
            tracing::debug!(
                "bundle: pnputil returned non-zero (status={:?})",
                fallback.status.code()
            );
            return None;
        }
        return Some(String::from_utf8_lossy(&fallback.stdout).into_owned());
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn serialport_list_dump() -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    let mut out = String::new();
    out.push_str("# serialport::available_ports() fallback dump\n\n");
    if ports.is_empty() {
        out.push_str("(no serial ports found)\n");
    }
    for p in &ports {
        out.push_str(&format!("- {}\n", p.port_name));
        if let serialport::SerialPortType::UsbPort(info) = &p.port_type {
            out.push_str(&format!(
                "    VID=0x{:04X} PID=0x{:04X}\n",
                info.vid, info.pid,
            ));
            if let Some(s) = info.serial_number.as_deref() {
                out.push_str(&format!("    serial={s}\n"));
            }
            if let Some(s) = info.manufacturer.as_deref() {
                out.push_str(&format!("    manufacturer={s}\n"));
            }
            if let Some(s) = info.product.as_deref() {
                out.push_str(&format!("    product={s}\n"));
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical pnputil snippet for a Pico in setup mode (VID_2E8A:PID_CAF0
    // composite parent + MI_00 CDC child + MI_02 vendor child). Derived from
    // a real bundle capture where the Pico was in setup mode and WinUSB was
    // bound to the vendor interface.
    const PNPUTIL_SETUP_MODE: &str = "\
Instance ID:                USB\\VID_2E8A&PID_CAF0\\E0C9125B0D9B
Device Description:         USB Composite Device
Class Name:                 USB
Status:                     Started
Driver Name:                usb.inf

Instance ID:                USB\\VID_2E8A&PID_CAF0&MI_00\\8&22cf742d&0&0000
Device Description:         USB Serial Device
Class Name:                 Ports
Status:                     Started
Driver Name:                usbser.inf

Instance ID:                USB\\VID_2E8A&PID_CAF0&MI_02\\8&22cf742d&0&0002
Device Description:         Pico Diag
Class Name:                 USBDevice
Status:                     Started
Driver Name:                winusb.inf
";

    // Canonical pnputil snippet for a Pico in run mode (VID_2E8A:PID_CAF0
    // composite parent + XInput child only, no MI_02). The run-mode firmware
    // presents only the XInput HID interface.
    const PNPUTIL_RUN_MODE: &str = "\
Instance ID:                USB\\VID_2E8A&PID_CAF0\\E0C9125B0D9B
Device Description:         USB Composite Device
Class Name:                 USB
Status:                     Started
Driver Name:                usb.inf

Instance ID:                USB\\VID_2E8A&PID_CAF0&MI_00\\8&33aa123&0&0000
Device Description:         Xbox 360 Controller
Class Name:                 XboxController
Status:                     Started
Driver Name:                xusb22.inf
";

    #[test]
    fn classify_pico_enum_not_enumerated() {
        // Bundle from the first customer (Pico 2 W in run mode with Wi-Fi
        // failed): no 2E8A:CAF0 entries at all.
        let text = "Instance ID: USB\\VID_28DE&PID_2102\\07F8359478\nStatus: Started\n";
        assert_eq!(classify_pico_enum(text), PicoEnumState::NotEnumerated);
    }

    #[test]
    fn classify_pico_enum_setup_mode() {
        assert_eq!(
            classify_pico_enum(PNPUTIL_SETUP_MODE),
            PicoEnumState::EnumeratedSetupMode,
        );
    }

    #[test]
    fn classify_pico_enum_run_mode() {
        assert_eq!(
            classify_pico_enum(PNPUTIL_RUN_MODE),
            PicoEnumState::EnumeratedRunMode,
        );
    }

    #[test]
    fn classify_pico_enum_parent_only_is_parent_only() {
        // Parent with no children at all is an incomplete Windows binding
        // state, not a healthy run-mode shape.
        let text = "Instance ID: USB\\VID_2E8A&PID_CAF0\\E0C9125B0D9B\nStatus: Started\n";
        assert_eq!(
            classify_pico_enum(text),
            PicoEnumState::EnumeratedParentOnly
        );
    }

    #[test]
    fn vendor_not_found_stub_not_enumerated_names_cable() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::NotEnumerated);
        assert!(stub.contains("0x2E8A:0xCAF0"), "missing VID/PID: {stub}");
        assert!(stub.contains("DATA cable"), "missing cable tip: {stub}");
    }

    #[test]
    fn vendor_not_found_stub_run_mode_names_watchdog() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::EnumeratedRunMode);
        assert!(
            stub.contains("association watchdog"),
            "missing watchdog tip: {stub}"
        );
        assert!(stub.contains("BOOTSEL"), "missing BOOTSEL tip: {stub}");
    }

    #[test]
    fn vendor_not_found_stub_parent_only_names_binding_failure() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::EnumeratedParentOnly);
        assert!(
            stub.contains("parent device"),
            "missing parent-only diagnosis: {stub}"
        );
        assert!(
            stub.contains("UF2 flash"),
            "missing flash-specific hint: {stub}"
        );
    }

    #[test]
    fn vendor_not_found_stub_setup_mode_names_winusb() {
        let stub = vendor_not_found_stub_text(&PicoEnumState::EnumeratedSetupMode);
        assert!(stub.contains("WinUSB"), "missing WinUSB tip: {stub}");
    }
}
