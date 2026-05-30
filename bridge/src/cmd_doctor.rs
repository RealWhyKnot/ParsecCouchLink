//! `couchlink doctor` -- run every diagnostic check and report
//! PASS/WARN/FAIL/SKIP with a one-line hint per failure. Exit codes:
//! 0 clean, 1 warnings, 2 hard fail, 3 setup not complete.
//!
//! Each check is also exported so `cmd_test::run` can invoke it
//! individually.

use std::time::{Duration, Instant};

use anyhow::Result;
use console::style;
use tokio::net::UdpSocket;

use crate::{cdc, config, protocol, support};

#[derive(Debug)]
pub enum CheckResult {
    Pass(String),
    Warn(String),
    Skip(String),
    Fail(String, String),
}

pub type BoxedCheck = std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult>>>;

pub async fn run() -> Result<()> {
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
    if !setup_complete && fails == 0 {
        println!("(note: config marks setup as incomplete; run `couchlink setup`)");
        std::process::exit(3);
    }
    if fails > 0 {
        tracing::error!("doctor: {} fail(s), suggesting bundle", fails);
        support::print_help_footer();
        std::process::exit(2);
    }
    if warns > 0 {
        std::process::exit(1);
    }
    Ok(())
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
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
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
    if let Err(e) = socket.set_broadcast(true) {
        return CheckResult::Fail(
            format!("set_broadcast failed on UDP socket: {e}"),
            "Network adapter may not support broadcast (unusual). Try a \
             different NIC, or disable the wrong-side adapter on a multi-homed PC."
                .into(),
        );
    }
    let discover = protocol::Packet::discover(0).encode();
    if let Err(e) = socket.send_to(&discover, "255.255.255.255:4242").await {
        return CheckResult::Fail(
            format!("UDP broadcast send failed: {e}"),
            "Likely a firewall rule blocking outbound UDP, or `255.255.255.255` \
             is routed to the wrong NIC on this multi-homed PC. Run \
             `couchlink test firewall`."
                .into(),
        );
    }

    let mut buf = [0u8; 64];
    let recv = tokio::time::timeout(Duration::from_secs(3), socket.recv_from(&mut buf)).await;
    match recv {
        Ok(Ok((n, from))) => match protocol::Packet::decode(&buf[..n]) {
            Ok(pkt) => match pkt.kind {
                protocol::PacketKind::Ack(info) => CheckResult::Pass(format!(
                    "ack from {from} proto v{} fw v{} board=0x{:02X} uid=0x{:08X} uptime={}s",
                    info.proto_version,
                    info.firmware_version(),
                    info.board_type,
                    info.unique_id_short,
                    info.uptime_seconds,
                )),
                other => {
                    let head: Vec<u8> = buf[..n.min(16)].to_vec();
                    tracing::debug!(
                        "doctor: discovery got non-ack packet from {from}: kind={:?} first {} bytes = {:02X?}",
                        other,
                        head.len(),
                        head,
                    );
                    CheckResult::Warn(format!("got non-ack packet from {from}: {other:?}"))
                }
            },
            Err(e) => {
                let head: Vec<u8> = buf[..n.min(32)].to_vec();
                tracing::debug!(
                    "doctor: discovery got {} bytes from {from} that did not parse: {e}; first {} = {:02X?}",
                    n,
                    head.len(),
                    head,
                );
                CheckResult::Fail(
                    format!("got {n} bytes from {from} but it did not parse as a Pico ack: {e}"),
                    "Another device on the LAN is replying on UDP/4242, or the \
                     Pico is running mismatched firmware. Re-flash via \
                     `flash.ps1` and try again."
                        .into(),
                )
            }
        },
        Ok(Err(e)) => CheckResult::Fail(
            format!("UDP recv error: {e}"),
            "Network adapter dropped mid-test; rare. Re-run.".into(),
        ),
        Err(_) => CheckResult::Fail(
            "no Pico replied within 3 s on UDP/4242".into(),
            "Confirm the Pico is powered, joined your Wi-Fi, and on the same \
             LAN as this PC. Common causes: AP isolation enabled on the router \
             (see wiki/Troubleshooting.md), Pico on a different SSID, or this \
             PC is multi-homed and the broadcast went out the wrong NIC. If \
             unsure, run `couchlink configure-wifi` to re-provision."
                .into(),
        ),
    }
}
