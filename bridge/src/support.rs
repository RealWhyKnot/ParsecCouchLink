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
        .unwrap_or_else(|_| "(unknown -- check %LOCALAPPDATA%\\ParsecToDreamcast)".to_string());
    eprintln!();
    eprintln!("If this looks wrong:");
    eprintln!("  logs:   {}", log_dir);
    eprintln!("  bundle: couchlink bundle");
    eprintln!("  report: {}", issue_url());
}
