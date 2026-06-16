//! `couchlink doctor` -- run every diagnostic check and report
//! PASS/WARN/FAIL/SKIP with a one-line hint per failure. Exit codes:
//! 0 clean, 1 warnings, 2 hard fail, 3 setup not complete.
//!
//! Each check is also exported so `cmd_test::run` can invoke it
//! individually.

use std::time::{Duration, Instant};

use anyhow::Result;
use console::style;

use crate::{cdc, config, discovery, support};

#[derive(Debug)]
pub enum CheckResult {
    Pass(String),
    Warn(String),
    Skip(String),
    Fail(String, String),
}

pub type BoxedCheck = std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult>>>;

#[derive(Debug, Default)]
pub struct DoctorSummary {
    pub warns: usize,
    pub fails: usize,
    pub setup_complete: bool,
}

pub async fn run() -> Result<()> {
    let summary = run_checks().await?;
    if !summary.setup_complete && summary.fails == 0 {
        println!("(note: config marks setup as incomplete; run `couchlink setup`)");
        std::process::exit(3);
    }
    if summary.fails > 0 {
        tracing::error!("doctor: {} fail(s), suggesting bundle", summary.fails);
        support::print_help_footer();
        std::process::exit(2);
    }
    if summary.warns > 0 {
        std::process::exit(1);
    }
    Ok(())
}

async fn run_checks() -> Result<DoctorSummary> {
    tracing::info!("doctor: starting, bridge v{}", env!("CARGO_PKG_VERSION"));
    println!("couchlink doctor v{}", env!("CARGO_PKG_VERSION"));
    println!();

    let mut passes = 0;
    let mut warns = 0;
    let mut fails = 0;
    let mut skips = 0;

    let checks: Vec<(&str, BoxedCheck)> = vec![
        ("paths", Box::pin(check_paths())),
        ("xinput", Box::pin(check_xinput())),
        ("startup", Box::pin(check_startup_shortcut())),
        ("firewall", Box::pin(check_firewall())),
        ("wifi-band", Box::pin(check_24ghz_warning())),
        ("cdc", Box::pin(check_cdc())),
        ("discover", Box::pin(check_discover())),
    ];

    for (name, fut) in checks {
        print!("  {:<14} ", name);
        let t0 = Instant::now();
        let res = fut.await;
        let ms = t0.elapsed().as_millis();
        match &res {
            CheckResult::Pass(m) => {
                println!("{}  ({:>5} ms)  {}", style("PASS").green(), ms, m);
                tracing::debug!("doctor: {} -> PASS ({}ms)", name, ms);
                passes += 1;
            }
            CheckResult::Warn(m) => {
                println!("{}  ({:>5} ms)  {}", style("WARN").yellow(), ms, m);
                tracing::debug!("doctor: {} -> WARN ({}ms)", name, ms);
                warns += 1;
            }
            CheckResult::Skip(m) => {
                println!("{}  ({:>5} ms)  {}", style("SKIP").dim(), ms, m);
                tracing::debug!("doctor: {} -> SKIP ({}ms)", name, ms);
                skips += 1;
            }
            CheckResult::Fail(m, hint) => {
                println!("{}  ({:>5} ms)  {}", style("FAIL").red(), ms, m);
                println!("                hint: {}", hint);
                tracing::debug!("doctor: {} -> FAIL ({}ms)", name, ms);
                fails += 1;
            }
        }
    }

    println!();
    println!(
        "summary: {} pass, {} warn, {} fail, {} skip",
        passes, warns, fails, skips,
    );

    let setup_complete = config::load().map(|c| c.setup_complete).unwrap_or(false);
    Ok(DoctorSummary {
        warns,
        fails,
        setup_complete,
    })
}

pub async fn check_paths() -> CheckResult {
    let cfg_dir = match config::config_dir() {
        Ok(p) => p,
        Err(e) => {
            return CheckResult::Fail(
                format!("can't resolve config dir: {e}"),
                "ProjectDirs returned None; very unusual.".into(),
            );
        }
    };
    let log_dir = config::log_dir().ok().unwrap_or_default();
    if let Err(e) = config::ensure_dirs() {
        return CheckResult::Fail(
            format!("can't create {}: {e}", cfg_dir.display()),
            "Check %APPDATA% permissions and disk space.".into(),
        );
    }
    CheckResult::Pass(format!(
        "config={} logs={}",
        cfg_dir.display(),
        log_dir.display(),
    ))
}

pub async fn check_xinput() -> CheckResult {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::XboxController::{
            XInputGetState, XINPUT_STATE, XUSER_MAX_COUNT,
        };
        let mut found = Vec::new();
        for s in 0..XUSER_MAX_COUNT {
            let mut state = XINPUT_STATE::default();
            let r = unsafe { XInputGetState(s, &mut state) };
            if r == 0 {
                found.push(s);
            }
        }
        if found.is_empty() {
            return CheckResult::Warn(
                "no XInput controllers detected. Parsec must be running with \
                 a guest who has a gamepad. A real wired Xbox pad also works \
                 here for bench testing."
                    .into(),
            );
        }
        CheckResult::Pass(format!("XInput slots connected: {:?}", found))
    }
    #[cfg(not(windows))]
    {
        CheckResult::Skip("XInput is Windows-only".into())
    }
}

pub async fn check_startup_shortcut() -> CheckResult {
    #[cfg(windows)]
    {
        match crate::known_folders::shortcut_path_for("Parsec CouchLink") {
            Ok(p) if p.exists() => CheckResult::Pass(format!("{}", p.display())),
            Ok(p) => CheckResult::Warn(format!(
                "not installed at {}. Run `couchlink setup` to install autostart.",
                p.display(),
            )),
            Err(e) => CheckResult::Fail(
                format!("can't resolve startup folder: {e}"),
                "SHGetKnownFolderPath failed; rare. Reboot and retry.".into(),
            ),
        }
    }
    #[cfg(not(windows))]
    {
        CheckResult::Skip("startup shortcut is Windows-only".into())
    }
}

pub async fn check_firewall() -> CheckResult {
    #[cfg(windows)]
    {
        match tokio::process::Command::new("netsh")
            .args(["advfirewall", "show", "currentprofile", "state"])
            .output()
            .await
        {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
                let on = text.contains("state") && text.contains("on");
                if on {
                    CheckResult::Warn(
                        "Windows Firewall is on for the current profile. UDP/4242 \
                         may be blocked. If discovery fails, allow it with: \
                         `netsh advfirewall firewall add rule name=couchlink dir=in \
                         action=allow protocol=UDP localport=4242` (needs admin)."
                            .into(),
                    )
                } else {
                    CheckResult::Pass("firewall reports state OFF for the current profile".into())
                }
            }
            Ok(o) => CheckResult::Warn(format!(
                "netsh advfirewall exit {:?}, can't determine firewall state",
                o.status.code(),
            )),
            Err(e) => CheckResult::Warn(format!("netsh not runnable: {e}")),
        }
    }
    #[cfg(not(windows))]
    {
        CheckResult::Skip("firewall check is Windows-only".into())
    }
}

pub async fn check_24ghz_warning() -> CheckResult {
    CheckResult::Warn(
        "Pico 2 W is 2.4 GHz only. If your home Wi-Fi runs 5 GHz exclusively, \
         the Pico cannot join. Enable a 2.4 GHz SSID on the router and use \
         that during setup."
            .into(),
    )
}

pub async fn check_cdc() -> CheckResult {
    // Three-way classify so a stuck-at-stage-4 bundle says exactly
    // which branch fired:
    //   - no port  -> Skip (this is normal post-setup)
    //   - port found but HELLO bombs -> Fail with the specific reason
    //   - port found and HELLO succeeds -> Pass with firmware details
    let port = match cdc::find_setup_port() {
        Ok(p) => p,
        Err(_) => {
            return CheckResult::Skip(
                "no Pico in setup mode plugged in (this is normal once setup is finished)".into(),
            );
        }
    };
    let probe = tokio::task::spawn_blocking(move || -> Result<cdc::HelloAck> {
        let mut p = cdc::PicoSetup::open_named(&port)?;
        p.hello()
    })
    .await;
    match probe {
        Ok(Ok(ack)) => CheckResult::Pass(format!(
            "HELLO ok proto v{} fw v{} board=0x{:02X} creds={}",
            ack.proto_version,
            ack.firmware_version(),
            ack.board_type,
            if ack.creds_present() {
                "present"
            } else {
                "absent"
            },
        )),
        Ok(Err(e)) => CheckResult::Fail(
            format!("setup-mode CDC port opened but HELLO failed: {e:#}"),
            "If a COM port is visible in Device Manager but HELLO times out, \
             unplug + replug the Pico (hold BOOTSEL during plug-in if you also \
             want to re-flash). If it still fails, run `couchlink bundle` -- \
             pico-diag.txt will show whether the firmware saw the request."
                .into(),
        ),
        Err(e) => CheckResult::Fail(
            format!("CDC probe task failed: {e}"),
            "Internal scheduling error -- run again. If it recurs, attach a bundle.".into(),
        ),
    }
}

pub async fn check_discover() -> CheckResult {
    let socket = match crate::net::bind_udp("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            return CheckResult::Fail(
                format!("UDP bind failed: {e}"),
                "Another process owns the ephemeral port, or Windows Firewall \
                 is blocking the bind. Re-run, or open an inbound rule with \
                 `New-NetFirewallRule -DisplayName couchlink -Direction Inbound \
                 -Protocol UDP -LocalPort 4242 -Action Allow` (admin)."
                    .into(),
            );
        }
    };

    match discovery::collect(&socket, Duration::from_secs(3)).await {
        Ok(replies) if !replies.is_empty() => CheckResult::Pass(format_discover_pass(&replies)),
        Ok(_) => CheckResult::Fail(
            "no Pico replied within 3 s on UDP/4242".into(),
            support::no_pico_wifi_short_hint().into(),
        ),
        Err(e) => CheckResult::Fail(
            format!("UDP discovery failed: {e}"),
            "Likely a firewall rule, a network adapter problem, or a route selection issue. \
             Run `couchlink test discover --all`; if that fails too, run `couchlink bundle`."
                .into(),
        ),
    }
}

fn format_discover_pass(replies: &[discovery::DiscoveryReply]) -> String {
    let first = &replies[0];
    let count_prefix = if replies.len() == 1 {
        "1 Pico reply".to_string()
    } else {
        format!("{} Pico replies", replies.len())
    };
    format!(
        "{count_prefix}; first ack from {} proto v{} fw v{} board=0x{:02X} uid=0x{:08X} uptime={}s",
        first.peer,
        first.info.proto_version,
        first.info.firmware_version(),
        first.info.board_type,
        first.info.unique_id_short,
        first.info.uptime_seconds,
    )
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use crate::protocol::{self, AckInfo, Persona};

    use super::*;

    #[test]
    fn discover_pass_message_reports_count_and_first_ack() {
        let replies = vec![
            discovery::DiscoveryReply {
                peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 50, 226)), 4242),
                info: AckInfo {
                    proto_version: 1,
                    fw_major: 26,
                    fw_minor: 6,
                    fw_patch: 16,
                    board_type: protocol::BOARD_PICO_2_W,
                    uptime_seconds: 42,
                    unique_id_short: 0x07D37EB6,
                    full_version: None,
                },
                persona: Persona::Ps4,
                flags: 0,
            },
            discovery::DiscoveryReply {
                peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 50, 4)), 4242),
                info: AckInfo {
                    proto_version: 1,
                    fw_major: 26,
                    fw_minor: 6,
                    fw_patch: 16,
                    board_type: protocol::BOARD_PICO_W_RP2040,
                    uptime_seconds: 41,
                    unique_id_short: 0x523861E6,
                    full_version: None,
                },
                persona: Persona::Ps4,
                flags: 0,
            },
        ];

        let message = format_discover_pass(&replies);
        assert!(message.contains("2 Pico replies"));
        assert!(message.contains("first ack from 192.168.50.226:4242"));
        assert!(message.contains("board=0x01"));
        assert!(message.contains("uid=0x07D37EB6"));
    }
}
