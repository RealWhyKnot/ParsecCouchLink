//! Run-mode Pico USB diagnostics over Wi-Fi.

use std::fmt::Write as _;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tokio::time::interval;

use crate::tui::{input_text, select};
use crate::{cmd_run, pico_cache, protocol, support};

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_INTERVAL: Duration = Duration::from_millis(400);

pub async fn run(all: bool, ips: Vec<String>) -> Result<()> {
    let picos = resolve_targets(all, ips).await?;
    query_and_print(&picos).await
}

pub async fn run_for_targets(picos: &[cmd_run::PicoTarget]) -> Result<()> {
    query_and_print(picos).await
}

pub async fn run_interactive() -> Result<()> {
    loop {
        println!("Looking for running Pico boards on Wi-Fi...");
        let picos = cmd_run::discover_picos(DISCOVER_TIMEOUT).await?;
        if !picos.is_empty() {
            if let Err(e) = query_and_print(&picos).await {
                println!("USB diagnostic could not complete: {e:#}");
            }
            return Ok(());
        }

        support::print_no_pico_wifi_help(DISCOVER_TIMEOUT.as_secs());
        println!();
        println!(
            "If the Pico joined Wi-Fi but broadcast discovery is blocked, enter its IP manually."
        );
        let choices = vec!["Try discovery again", "Enter Pico IP manually", "Back"];
        match select("USB diagnostic", &choices, 0).await? {
            0 => continue,
            1 => {
                let Some(pico) = prompt_manual_pico_ip().await? else {
                    continue;
                };
                if let Err(e) = query_and_print(&[pico]).await {
                    println!("USB diagnostic could not complete: {e:#}");
                }
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
}

async fn resolve_targets(all: bool, ips: Vec<String>) -> Result<Vec<cmd_run::PicoTarget>> {
    if !ips.is_empty() {
        let mut targets = Vec::new();
        for text in ips {
            let ip = parse_ip_arg(&text)?;
            targets.push(cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await?);
        }
        return Ok(targets);
    }

    let mut picos = cmd_run::discover_picos(DISCOVER_TIMEOUT).await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(DISCOVER_TIMEOUT.as_secs()));
    }
    if !all && picos.len() > 1 {
        println!(
            "{} Pico boards replied; checking the first one. Use `--all` to check every Pico.",
            picos.len()
        );
        picos.truncate(1);
    }
    Ok(picos)
}

async fn prompt_manual_pico_ip() -> Result<Option<cmd_run::PicoTarget>> {
    let text = input_text("Pico IP address (blank to cancel)").await?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let ip = match parse_ip_arg(&text) {
        Ok(ip) => ip,
        Err(e) => {
            println!("Invalid IP address: {e:#}");
            return Ok(None);
        }
    };
    println!("Probing {ip}:{} directly...", protocol::PORT);
    match cmd_run::probe_pico_ip(ip, Duration::from_secs(8)).await {
        Ok(pico) => Ok(Some(pico)),
        Err(e) => {
            println!("No Pico replied at {ip}: {e:#}");
            Ok(None)
        }
    }
}

fn parse_ip_arg(text: &str) -> Result<IpAddr> {
    cmd_run::parse_ip_selector(text)
        .ok_or_else(|| anyhow!("`{}` is not a valid IP address", text.trim()))
}

async fn query_and_print(picos: &[cmd_run::PicoTarget]) -> Result<()> {
    let mut failures = 0usize;
    for pico in picos {
        println!();
        println!("{}", pico.detail_label());
        match query_usb_diag(pico, PROBE_TIMEOUT).await {
            Ok(diag) => print_usb_diag(&diag, pico.persona),
            Err(e) => {
                failures += 1;
                println!("  FAIL  USB diagnostic did not reply: {e:#}");
                println!("  Update Pico firmware, then run this check again.");
            }
        }
    }
    if failures > 0 {
        bail!("{failures} Pico USB diagnostic query failed");
    }
    Ok(())
}

pub async fn query_usb_diag(
    pico: &cmd_run::PicoTarget,
    timeout: Duration,
) -> Result<protocol::UsbDiag> {
    let socket = crate::net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP USB diagnostic socket")?;
    let mut seq = 0xD1u8;
    let started = Instant::now();
    let deadline = started + timeout;
    let mut tick = interval(PROBE_INTERVAL);
    let mut buf = [0u8; 256];

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let req = protocol::encode_get_usb_diag(seq);
                seq = seq.wrapping_add(1);
                socket
                    .send_to(&req, pico.peer)
                    .await
                    .with_context(|| format!("sending USB diagnostic request to {}", pico.peer))?;
            }
            r = socket.recv_from(&mut buf) => {
                let (n, from) = r.context("receiving USB diagnostic reply")?;
                if from.ip() != pico.peer.ip() {
                    continue;
                }
                let diag = protocol::UsbDiag::decode(&buf[..n])
                    .context("decoding USB diagnostic reply")?;
                pico_cache::record(
                    pico_cache::PicoStateSnapshot::from_target("usb-diag", pico)
                        .with_usb_diag(&diag, pico.persona)
                        .with_outcome("usb_diag_captured"),
                );
                return Ok(diag);
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                bail!(
                    "no USB diagnostic reply from {} within {} s",
                    pico.peer,
                    timeout.as_secs()
                );
            }
        }
    }
}

fn print_usb_diag(diag: &protocol::UsbDiag, persona: protocol::Persona) {
    print!("{}", format_usb_diag(diag, persona));
}

pub fn format_usb_diag(diag: &protocol::UsbDiag, persona: protocol::Persona) -> String {
    let device_label = match persona {
        protocol::Persona::Xinput => "XInput",
        protocol::Persona::Keyboard => "HID keyboard",
        protocol::Persona::Maple => "XInput (Maple mode)",
        protocol::Persona::Ps3 => "PS3 HID gamepad",
        protocol::Persona::Ps4 => "PS4 HID gamepad",
        protocol::Persona::XboxOne => "Xbox One XGIP",
        protocol::Persona::Debug => "Debug XInput packet capture",
    };
    let mut out = String::new();
    let _ = writeln!(out, "  {}", usb_verdict(diag, device_label));
    let _ = writeln!(
        out,
        "  USB: {}{}  mounts={} unmounts={} suspends={} resumes={}",
        if diag.mounted() {
            "configured"
        } else {
            "not configured"
        },
        if diag.suspended() { " / suspended" } else { "" },
        diag.mount_count,
        diag.umount_count,
        diag.suspend_count,
        diag.resume_count,
    );
    let _ = writeln!(
        out,
        "  descriptors: device={} configuration={}",
        diag.device_desc_count, diag.config_desc_count
    );
    let _ = writeln!(
        out,
        "  {device_label}: queued_reports={} host_accepted_reports={} host_out_reports={}",
        diag.xinput_in_queued_count, diag.xinput_in_sent_count, diag.xinput_out_count
    );
    let _ = writeln!(
        out,
        "  IN report blocks: not_mounted={} not_ready={} short_write={} idle_suppressed={} last={} want={} got={}",
        diag.xinput_in_blocked_not_mounted_count,
        diag.xinput_in_blocked_not_ready_count,
        diag.xinput_in_blocked_short_write_count,
        diag.xinput_in_idle_suppressed_count,
        protocol::usb_in_blocked_reason_label(diag.last_in_blocked_reason),
        diag.last_in_blocked_want,
        diag.last_in_blocked_got,
    );
    let _ = writeln!(
        out,
        "  recent: mount={} in={} blocked={} out={} bridge_packet={}",
        age_label(diag, diag.last_mount_ms),
        age_label(diag, diag.last_in_sent_ms),
        age_label(diag, diag.last_in_blocked_ms),
        age_label(diag, diag.last_out_ms),
        age_label(diag, diag.last_bridge_packet_ms),
    );
    if diag.last_out_len > 0 {
        let _ = writeln!(
            out,
            "  last host OUT: len={} first_bytes={:02X} {:02X}",
            diag.last_out_len, diag.last_out_byte0, diag.last_out_byte1
        );
    }
    let _ = writeln!(
        out,
        "  stream state: bridge_peer={} parsec_connected={}",
        yes_no(diag.bridge_peer_latched()),
        yes_no(diag.parsec_connected()),
    );
    out
}

fn usb_verdict(diag: &protocol::UsbDiag, device_label: &str) -> String {
    if !diag.mounted() {
        if diag.device_desc_count > 0 || diag.config_desc_count > 0 {
            format!("FAIL  USB host started enumeration but did not configure the {device_label} device.")
        } else {
            "FAIL  Pico sees no USB host enumeration traffic.".to_string()
        }
    } else if !diag.xinput_report_sent() {
        if diag.in_blocked_total() > 0 && diag.last_in_blocked_reason != 0 {
            format!(
                "WARN  USB is configured, but the latest {device_label} report was blocked: {} (want={} got={}).",
                protocol::usb_in_blocked_reason_label(diag.last_in_blocked_reason),
                diag.last_in_blocked_want,
                diag.last_in_blocked_got,
            )
        } else {
            format!(
                "WARN  USB is configured, but the host has not accepted a {device_label} report yet."
            )
        }
    } else if diag.xinput_out_seen() {
        format!("PASS  USB host is polling and has sent {device_label} OUT traffic.")
    } else {
        format!("PASS  USB host is polling the {device_label} endpoint. No OUT traffic seen yet.")
    }
}

fn age_label(diag: &protocol::UsbDiag, timestamp_ms: u32) -> String {
    match diag.age_ms(timestamp_ms) {
        Some(ms) if ms < 1000 => format!("{ms} ms ago"),
        Some(ms) => format!("{} s ago", ms / 1000),
        None => "never".to_string(),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(mounted: bool, sent: bool, out: bool, desc: bool) -> protocol::UsbDiag {
        protocol::UsbDiag {
            seq: 1,
            flags: 0,
            version: protocol::USB_DIAG_VERSION,
            usb_flags: if mounted {
                protocol::USB_DIAG_FLAG_MOUNTED
            } else {
                0
            },
            activity_flags: (if sent {
                protocol::USB_DIAG_ACTIVITY_SENT
            } else {
                0
            }) | (if out {
                protocol::USB_DIAG_ACTIVITY_OUT
            } else {
                0
            }),
            last_out_len: 0,
            now_ms: 10_000,
            last_bridge_packet_ms: 0,
            mount_count: if mounted { 1 } else { 0 },
            umount_count: 0,
            suspend_count: 0,
            resume_count: 0,
            device_desc_count: if desc { 1 } else { 0 },
            config_desc_count: if desc { 1 } else { 0 },
            xinput_in_queued_count: if sent { 1 } else { 0 },
            xinput_in_sent_count: if sent { 1 } else { 0 },
            xinput_out_count: if out { 1 } else { 0 },
            xinput_in_blocked_not_mounted_count: 0,
            xinput_in_blocked_not_ready_count: 0,
            xinput_in_blocked_short_write_count: 0,
            xinput_in_idle_suppressed_count: 0,
            last_mount_ms: 9000,
            last_umount_ms: 0,
            last_in_queued_ms: 0,
            last_in_sent_ms: if sent { 9500 } else { 0 },
            last_out_ms: if out { 9600 } else { 0 },
            last_in_blocked_ms: 0,
            last_in_blocked_reason: 0,
            last_in_blocked_want: 0,
            last_in_blocked_got: 0,
            last_out_byte0: 0,
            last_out_byte1: 0,
        }
    }

    #[test]
    fn verdict_identifies_usb_enumeration_shapes() {
        assert!(usb_verdict(&diag(false, false, false, false), "XInput").starts_with("FAIL"));
        assert!(
            usb_verdict(&diag(false, false, false, true), "XInput").contains("started enumeration")
        );
        assert!(usb_verdict(&diag(true, false, false, true), "XInput").starts_with("WARN"));
        assert!(usb_verdict(&diag(true, true, false, true), "XInput").starts_with("PASS"));
        assert!(usb_verdict(&diag(true, true, true, true), "XInput").contains("OUT traffic"));
    }

    #[test]
    fn verdict_uses_persona_label() {
        let warn = usb_verdict(&diag(true, false, false, true), "HID keyboard");
        assert!(warn.contains("HID keyboard report"));
    }

    #[test]
    fn verdict_expects_xinput_reports_for_maple_mode() {
        let verdict = usb_verdict(&diag(true, false, false, true), "XInput (Maple mode)");
        assert!(verdict.contains("XInput (Maple mode) report"));
    }

    #[test]
    fn verdict_uses_playstation_label() {
        let verdict = usb_verdict(&diag(true, false, false, true), "PS3 HID gamepad");
        assert!(verdict.contains("PS3 HID gamepad report"));
    }

    #[test]
    fn format_usb_diag_contains_persona_and_counters() {
        let text = format_usb_diag(&diag(true, true, true, true), protocol::Persona::Ps4);
        assert!(text.contains("PS4 HID gamepad"));
        assert!(text.contains("mounts=1"));
        assert!(text.contains("host_accepted_reports=1"));
        assert!(text.contains("IN report blocks:"));
        assert!(text.contains("stream state:"));
    }

    #[test]
    fn verdict_reports_block_reason_when_configured_without_sent_report() {
        let mut d = diag(true, false, false, true);
        d.xinput_in_blocked_not_ready_count = 3;
        d.last_in_blocked_reason = protocol::USB_DIAG_IN_BLOCKED_NOT_READY;
        d.last_in_blocked_want = 20;
        d.last_in_blocked_got = 0;

        let verdict = usb_verdict(&d, "XInput");
        assert!(verdict.contains("blocked: not_ready"));
        assert!(verdict.contains("want=20 got=0"));
    }

    #[test]
    fn age_label_formats_never_and_seconds() {
        let d = diag(true, true, false, true);
        assert_eq!(age_label(&d, 0), "never");
        assert_eq!(age_label(&d, 9500), "500 ms ago");
        assert_eq!(age_label(&d, 8000), "2 s ago");
    }
}
