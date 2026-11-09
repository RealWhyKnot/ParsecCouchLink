use crate::{cdc, diag_usb, journal};
use std::time::Duration;

pub(super) async fn attempt_diag_recovery_after_lan_timeout() {
    outln!();
    outln!(
        "Pico did not answer on the LAN within 60 s. \
         Trying to retrieve the firmware diag log..."
    );
    journal!(
        "setup",
        "stage 5 timeout -- trying diag recovery (window 45 s)"
    );
    tracing::info!("setup: stage-5 timeout; waiting up to 45 s for setup-mode port to re-appear");

    let port = wait_for_setup_port().await;

    let Some(port) = port else {
        tracing::warn!("setup: diag recovery failed -- Pico did not re-appear in setup mode");
        journal!(
            "setup",
            "diag recovery failed -- no setup-mode port within 45 s"
        );
        outln!();
        outln!(
            "Could not retrieve diag log -- the Pico did not re-appear in setup mode. \
             This may mean the firmware predates the association watchdog (rebuild from \
             main and reflash), or the Pico has a more fundamental boot issue \
             (try `couchlink bundle` for a full system snapshot)."
        );
        print_lan_timeout_walkthrough();
        return;
    };

    tracing::info!("setup: setup-mode port re-appeared at {port}, pulling diag log via WinUSB");
    journal!(
        "setup",
        "diag recovery -- setup-mode port re-appeared at {port}"
    );
    outln!("  Setup-mode port re-appeared at {port}. Pulling diag log...");

    let outcome = tokio::task::spawn_blocking(diag_usb::capture_diag_blocking)
        .await
        .unwrap_or_else(|e| diag_usb::VendorDiagOutcome::TransferFailed {
            step: "spawn",
            bytes_received: 0,
            error: format!("blocking task panicked: {e}"),
        });

    match outcome {
        diag_usb::VendorDiagOutcome::Captured { text, lost } => {
            print_recovered_diag_log(&text, lost);
        }
        diag_usb::VendorDiagOutcome::Empty => {
            tracing::info!("setup: diag recovery -- vendor pull returned empty ring");
            journal!("setup", "diag recovery -- vendor pull empty");
            outln!("  Diag log was empty -- the Pico re-appeared but its log ring is clear.");
            print_lan_timeout_walkthrough();
        }
        other => {
            tracing::warn!("setup: diag recovery vendor pull failed: {other:?}");
            journal!("setup", "diag recovery failed -- vendor pull error");
            outln!(
                "  Pico re-appeared in setup mode but the vendor diag pull did not succeed \
                 ({other:?}). Run `couchlink bundle` for a full system snapshot."
            );
            print_lan_timeout_walkthrough();
        }
    }
}

pub(super) async fn wait_for_setup_port() -> Option<String> {
    const RECOVERY_WINDOW: Duration = Duration::from_secs(45);
    const POLL_INTERVAL: Duration = Duration::from_millis(500);

    let deadline = std::time::Instant::now() + RECOVERY_WINDOW;
    let mut found = None;
    let mut beat_at = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(p) = cdc::find_setup_port() {
            found = Some(p);
            break;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        if now >= beat_at {
            let elapsed = RECOVERY_WINDOW
                .saturating_sub(deadline.saturating_duration_since(now))
                .as_secs();
            outln!(
                "  ... still waiting for Pico to re-appear in setup mode ({}/45 s)",
                elapsed
            );
            beat_at = now + Duration::from_secs(10);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    found
}

pub(super) fn print_recovered_diag_log(text: &str, lost: u32) {
    tracing::info!(
        "setup: diag recovery succeeded -- {} bytes, {} lost",
        text.len(),
        lost,
    );
    journal!(
        "setup",
        "diag recovery succeeded -- {} bytes captured",
        text.len()
    );
    outln!();
    if lost > 0 {
        outln!("--- firmware diag log (last 50 lines; {lost} byte(s) dropped from ring) ---");
    } else {
        outln!("--- firmware diag log (last 50 lines) ---");
    }
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(50);
    for line in &lines[start..] {
        if line.contains("assoc_result") {
            outln!(">>> {line}");
        } else {
            outln!("    {line}");
        }
    }
    outln!("--- end of diag log ---");
    outln!();
    outln!(
        "If the diag log above names a Wi-Fi error \
         (BADAUTH / NONET / NOIP / timeout), that is the exact cause. \
         Otherwise, try the following in order:"
    );
    print_lan_timeout_walkthrough();
}

pub(super) fn print_lan_timeout_walkthrough() {
    outln!();
    outln!("  1. Confirm your SSID is a real 2.4 GHz network on your router");
    outln!("     (not a 5 GHz-only SSID -- many routers show only the 5 GHz SSID by default).");
    outln!("  2. Confirm the Pico is powered on and within Wi-Fi range.");
    outln!("  3. If you have changed Wi-Fi networks since setup, hold BOOTSEL for 3+ seconds");
    outln!("     during plug-in to wipe the saved creds, then re-run `couchlink setup`.");
    outln!("  4. If you have multiple network adapters, make sure the bridge is allowed");
    outln!("     through Windows Firewall on the active profile.");
    outln!("     `couchlink bundle` includes the firewall and network snapshot.");
    outln!("  5. Run `couchlink bundle` and attach the resulting zip to a bug report.");
}
