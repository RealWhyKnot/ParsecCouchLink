//! system-info.txt and the bundled (plain-text) doctor re-run.

use chrono::Local;

use crate::config;

use super::manifest::windows_version;

/// Always-present body for `bundle/system-info.txt`. Captures things
/// that can be checked without opening the Pico, so an issue reporter
/// has provenance even when the Pico is gone (lost cable, dead
/// firmware, hardware swap).
pub(super) async fn build_system_info() -> String {
    let cfg = config::load().unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!("couchlink {}\n", env!("CARGO_PKG_VERSION"),));
    out.push_str(&format!("generated  {}\n", Local::now().to_rfc3339()));
    out.push_str(&format!("os         {}\n", std::env::consts::OS));
    if let Some(v) = windows_version().await {
        out.push_str(&format!("windows    {v}\n"));
    }
    out.push_str(&format!(
        "hostname   {}\n",
        hostname_short().unwrap_or_else(|| "(unknown)".into())
    ));
    out.push_str(&format!("setup-done {}\n", cfg.setup_complete));
    match &cfg.last_pico {
        Some(p) => {
            out.push_str(&format!(
                "last-pico  fw={} board=0x{:02X} unique-id-short=0x{:08X}\n",
                crate::firmware_version::FirmwareVersion::from_triplet(
                    p.fw_major, p.fw_minor, p.fw_patch,
                ),
                p.board_type,
                p.unique_id_short,
            ));
            if let Some(ip) = p.last_ip.as_deref() {
                out.push_str(&format!("           last-ip={ip}\n"));
            }
            if let Some(n) = p.device_name.as_deref() {
                out.push_str(&format!("           device-name={n}\n"));
            }
        }
        None => out.push_str("last-pico  (none)\n"),
    }
    out.push_str(&format!(
        "config     {}\n",
        config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(unknown)".into())
    ));
    out.push_str(&format!(
        "logs       {}\n",
        config::log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(unknown)".into())
    ));
    out
}

fn hostname_short() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())?;
    Some(raw.split('.').next().unwrap_or(&raw).to_string())
}

pub(super) async fn run_doctor_silently() -> String {
    // Doctor prints to stdout via println!; we don't capture that here.
    // For the bundle we instead run each check and format the result
    // ourselves into a plain-text report.
    use crate::cmd_doctor::*;
    let checks: Vec<(&str, BoxedCheck)> = vec![
        ("paths", Box::pin(check_paths())),
        ("xinput", Box::pin(check_xinput())),
        ("startup", Box::pin(check_startup_shortcut())),
        ("firewall", Box::pin(check_firewall())),
        ("wifi-band", Box::pin(check_24ghz_warning())),
        ("cdc", Box::pin(check_cdc())),
        ("discover", Box::pin(check_discover())),
    ];
    let mut out = String::new();
    out.push_str(&format!(
        "couchlink doctor (bundled, no terminal styling)\n  bridge v{}\n  time {}\n\n",
        env!("CARGO_PKG_VERSION"),
        chrono::Local::now().to_rfc3339(),
    ));
    for (name, fut) in checks {
        let res = fut.await;
        out.push_str(&format!("  [{:<10}] {}\n", name, format_result(&res)));
    }
    out
}

fn format_result(r: &crate::cmd_doctor::CheckResult) -> String {
    use crate::cmd_doctor::CheckResult::*;
    match r {
        Pass(m) => format!("PASS  {m}"),
        Warn(m) => format!("WARN  {m}"),
        Skip(m) => format!("SKIP  {m}"),
        Fail(m, h) => format!("FAIL  {m}\n              hint: {h}"),
    }
}
