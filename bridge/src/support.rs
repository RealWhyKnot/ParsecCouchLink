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
           1. Run `couchlink` and choose `Start streaming` again; CouchLink automatically checks setup-mode USB and saved-Wi-Fi Picos.\n\
           2. If your router shows the Pico's IP, choose `Enter Pico IP manually` from the menu.\n\
           3. If the router, SSID, or password changed, run `couchlink configure-wifi`.\n\
           4. Run `couchlink bundle` and send the zip if it still fails."
    )
}

pub fn print_no_pico_wifi_help(timeout_seconds: u64) {
    println!("{}", no_pico_wifi_help(timeout_seconds));
}

pub fn no_pico_wifi_short_hint() -> &'static str {
    "Run `couchlink` and choose `Start streaming` again so CouchLink can check setup-mode USB Picos with saved Wi-Fi. If the router shows its IP, choose `Enter Pico IP manually`. If Wi-Fi changed, run `couchlink configure-wifi`. If it still fails, run `couchlink bundle`."
}

#[cfg(test)]
mod tests {
    use super::{no_pico_wifi_help, no_pico_wifi_short_hint};

    #[test]
    fn no_pico_wifi_help_points_to_bundle() {
        let help = no_pico_wifi_help(5);
        assert!(help.contains("No Pico replied on Wi-Fi within 5 s."));
        assert!(help.contains("not a controller problem"));
        assert!(help.contains("automatically checks setup-mode USB"));
        assert!(help.contains("Enter Pico IP manually"));
        assert!(help.contains("couchlink configure-wifi"));
        assert!(help.contains("couchlink bundle"));
        assert!(!help.contains("couchlink recover"));
        assert!(!help.contains("couchlink test"));
        assert!(!help.contains("couchlink debug"));
        assert!(!help.contains("couchlink doctor"));
    }

    #[test]
    fn no_pico_wifi_short_hint_is_actionable() {
        let hint = no_pico_wifi_short_hint();
        assert!(hint.contains("setup-mode USB"));
        assert!(hint.contains("Enter Pico IP manually"));
        assert!(hint.contains("couchlink bundle"));
        assert!(!hint.contains("couchlink recover"));
        assert!(!hint.contains("couchlink test"));
        assert!(!hint.contains("couchlink doctor"));
    }
}
