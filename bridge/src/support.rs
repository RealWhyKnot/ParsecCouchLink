//! Shared helpers for end-user error/exit messaging.

/// GitHub issues URL for this binary. Pulled from Cargo.toml's
/// `repository` field at compile time.
pub fn issue_url() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY");
    format!("{}/issues", repo.trim_end_matches('/'))
}

/// Print the standard "things went wrong, here's what to do" footer
/// to stderr. Three lines: log dir, bundle command, issue URL.
pub fn print_help_footer() {
    let log_dir = crate::config::log_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| {
            "(unknown -- check %LOCALAPPDATA%\\ParsecCouchLink\\data\\logs)".to_string()
        });
    eprintln!();
    eprintln!("If this looks wrong:");
    eprintln!("  logs:   {}", log_dir);
    eprintln!("  bundle: couchlink bundle");
    eprintln!("  report: {}", issue_url());
}

pub fn no_pico_wifi_help(timeout_seconds: u64) -> String {
    format!(
        "No Pico replied on Wi-Fi within {timeout_seconds} s.\n\
         This is a Pico/network discovery problem, not a controller problem.\n\
         Try this in order:\n\
           1. Run `couchlink debug --status` to see whether the Pico is on Wi-Fi, USB debug, or BOOTSEL.\n\
           2. Run `couchlink test discover --all` to retry discovery.\n\
           3. If your router shows the Pico's IP, choose `Enter Pico IP manually` or run `couchlink test discover --ip <ip>`.\n\
           4. If the router, SSID, or password changed, run `couchlink configure-wifi`.\n\
           5. If the Pico is on Wi-Fi but needs USB debug mode, run `couchlink debug --to-usb-debug`.\n\
           6. Run `couchlink doctor`; check Windows Firewall, router client isolation, and whether the PC and Pico are on the same LAN.\n\
           7. Run `couchlink bundle` and send the zip if it still fails."
    )
}

pub fn print_no_pico_wifi_help(timeout_seconds: u64) {
    println!("{}", no_pico_wifi_help(timeout_seconds));
}

pub fn no_pico_wifi_short_hint() -> &'static str {
    "Run `couchlink debug --status` to see whether the Pico is on Wi-Fi, USB debug, or BOOTSEL, then run `couchlink test discover --all`. If the router shows its IP, run `couchlink test discover --ip <ip>`. If Wi-Fi changed or the Pico is in USB debug mode, run `couchlink configure-wifi`. If it still fails, run `couchlink doctor` and `couchlink bundle`."
}

#[cfg(test)]
mod tests {
    use super::{no_pico_wifi_help, no_pico_wifi_short_hint};

    #[test]
    fn no_pico_wifi_help_points_to_recovery_commands() {
        let help = no_pico_wifi_help(5);
        assert!(help.contains("No Pico replied on Wi-Fi within 5 s."));
        assert!(help.contains("not a controller problem"));
        assert!(help.contains("couchlink debug --status"));
        assert!(help.contains("Enter Pico IP manually"));
        assert!(help.contains("couchlink test discover --ip <ip>"));
        assert!(help.contains("couchlink configure-wifi"));
        assert!(help.contains("couchlink debug --to-usb-debug"));
        assert!(help.contains("couchlink doctor"));
        assert!(help.contains("couchlink bundle"));
    }

    #[test]
    fn no_pico_wifi_short_hint_is_actionable() {
        let hint = no_pico_wifi_short_hint();
        assert!(hint.contains("couchlink debug --status"));
        assert!(hint.contains("couchlink test discover --all"));
    }
}
