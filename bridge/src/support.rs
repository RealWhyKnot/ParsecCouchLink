pub(crate) fn issue_url() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY");
    format!("{}/issues", repo.trim_end_matches('/'))
}

pub(crate) fn print_help_footer() {
    let log_dir = crate::config::log_dir().map_or_else(
        |_| "(unknown -- check %LOCALAPPDATA%\\ParsecCouchLink\\data\\logs)".to_string(),
        |p| p.display().to_string(),
    );
    errln!();
    errln!("If this looks wrong:");
    errln!("  logs:   {}", log_dir);
    errln!("  bundle: couchlink bundle");
    errln!("  report: {}", issue_url());
}

pub(crate) fn no_pico_wifi_help(timeout_seconds: u64) -> String {
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

pub(crate) fn print_no_pico_wifi_help(timeout_seconds: u64) {
    outln!("{}", no_pico_wifi_help(timeout_seconds));
}

pub(crate) fn no_pico_wifi_short_hint() -> &'static str {
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
