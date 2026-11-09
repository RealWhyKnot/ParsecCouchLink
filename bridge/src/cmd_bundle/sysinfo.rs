use chrono::Local;
use std::fmt::Write as _;

use crate::config;

use super::manifest::windows_version;

pub(super) async fn build_system_info() -> String {
    let cfg = config::load().unwrap_or_default();
    let mut out = String::new();
    let _ = writeln!(out, "couchlink {}", crate::VERSION);
    let _ = writeln!(out, "generated  {}", Local::now().to_rfc3339());
    let _ = writeln!(out, "os         {}", std::env::consts::OS);
    if let Some(v) = windows_version().await {
        let _ = writeln!(out, "windows    {v}");
    }
    let _ = writeln!(
        out,
        "hostname   {}",
        hostname_short().unwrap_or_else(|| "(unknown)".into())
    );
    let _ = writeln!(out, "setup-done {}", cfg.setup_complete);
    match &cfg.last_pico {
        Some(p) => {
            let _ = writeln!(
                out,
                "last-pico  fw={} board=0x{:02X} unique-id-short=0x{:08X}",
                crate::firmware_version::FirmwareVersion::from_triplet(
                    p.fw_major, p.fw_minor, p.fw_patch,
                ),
                p.board_type,
                p.unique_id_short
            );
            if let Some(ip) = p.last_ip.as_deref() {
                let _ = writeln!(out, "           last-ip={ip}");
            }
            if let Some(n) = p.device_name.as_deref() {
                let _ = writeln!(out, "           device-name={n}");
            }
        }
        None => out.push_str("last-pico  (none)\n"),
    }
    let _ = writeln!(
        out,
        "config     {}",
        config::config_path().map_or_else(|_| "(unknown)".into(), |p| p.display().to_string())
    );
    let _ = writeln!(
        out,
        "logs       {}",
        config::log_dir().map_or_else(|_| "(unknown)".into(), |p| p.display().to_string())
    );
    out
}

fn hostname_short() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())?;
    Some(raw.split('.').next().unwrap_or(&raw).to_string())
}

pub(super) async fn run_doctor_silently() -> String {
    use crate::cmd_doctor::{
        BoxedCheck, check_24ghz_warning, check_cdc, check_discover, check_firewall, check_paths,
        check_startup_shortcut, check_xinput,
    };
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
    let _ = write!(
        out,
        "couchlink doctor (bundled, no terminal styling)\n  bridge v{}\n  time {}\n\n",
        crate::VERSION,
        chrono::Local::now().to_rfc3339()
    );
    for (name, fut) in checks {
        let res = fut.await;
        let _ = writeln!(out, "  [{:<10}] {}", name, format_result(&res));
    }
    out
}

fn format_result(r: &crate::cmd_doctor::CheckResult) -> String {
    use crate::cmd_doctor::CheckResult::{Fail, Pass, Skip, Warn};
    match r {
        Pass(m) => format!("PASS  {m}"),
        Warn(m) => format!("WARN  {m}"),
        Skip(m) => format!("SKIP  {m}"),
        Fail(m, h) => format!("FAIL  {m}\n              hint: {h}"),
    }
}
