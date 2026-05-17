//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! crash files, Pico diag log, and a manifest.json with non-sensitive system
//! info. Intended to be attached to a bug report.
//!
//! NEVER include Wi-Fi credentials. The Pico stores them and the bridge
//! never reads them. SSID is also omitted by default to be safe.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Local;
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::{cdc, config};

pub async fn run(output: Option<PathBuf>) -> Result<()> {
    let pico_diag = try_capture_pico_diag().await;
    let pico_diag_captured = pico_diag.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

    let crash_files = collect_crash_file_names();
    let setup_transcripts = collect_setup_transcript_names();

    let manifest = build_manifest(pico_diag_captured, &crash_files, &setup_transcripts).await?;
    let manifest_json = serde_json::to_string_pretty(&manifest)?;

    let doctor_text = run_doctor_silently().await;

    let out_path = output.unwrap_or_else(|| {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        PathBuf::from(format!("couchlink-bundle-{stamp}.zip"))
    });
    let f = std::fs::File::create(&out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    let mut zip = ZipWriter::new(f);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("manifest.json", opts)?;
    zip.write_all(manifest_json.as_bytes())?;

    zip.start_file("doctor.txt", opts)?;
    zip.write_all(doctor_text.as_bytes())?;

    // Always write pico-diag.txt. If CDC capture failed (no port, port
    // open failed, GET_LOG_BUFFER timed out, or the buffer was empty),
    // the file contains a diagnostic stub explaining which branch took
    // that path. An absent file would be ambiguous; an explicit stub is
    // self-narrating.
    let pico_diag_body = match pico_diag.as_deref() {
        Some(text) if !text.is_empty() => text.to_string(),
        Some(_) => "(firmware diagnostic ring buffer was empty when bundle ran -- \
                    this means the Pico was reachable over CDC but had no log entries. \
                    Usually the Pico rebooted between the failure and bundle. If the \
                    failure was at boot, re-run the failing command and immediately run \
                    bundle while the Pico is still in setup mode.)"
            .to_string(),
        None => "(firmware diagnostic could not be captured over USB CDC -- the Pico \
                 either wasn't enumerated as a setup-mode serial device, didn't \
                 respond to GET_LOG_BUFFER, or rebooted before bundle could reach \
                 it. Re-plug the Pico, hold it in BOOTSEL, flash with `flash.ps1`, \
                 wait for it to come back as a setup-mode serial device, then run \
                 bundle. See the bridge log (logs/couchlink-*.log) for the specific \
                 CDC failure.)"
            .to_string(),
    };
    zip.start_file("pico-diag.txt", opts)?;
    zip.write_all(pico_diag_body.as_bytes())?;

    // Crash files from crash_dir().
    if let Ok(crash_dir) = config::crash_dir() {
        if crash_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&crash_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let p = entry.path();
                    if p.is_file() {
                        if let Some(name) = p.file_name() {
                            if let Ok(bytes) = std::fs::read(&p) {
                                zip.start_file(
                                    format!("crashes/{}", name.to_string_lossy()),
                                    opts,
                                )?;
                                zip.write_all(&bytes)?;
                            }
                        }
                    }
                }
            }
        }
    }

    // Logs: last 3 couchlink.*.log (bridge, written by tracing-appender's
    // daily rotation as couchlink.YYYY-MM-DD.log) and last 3 setup-*.log
    // (PowerShell transcripts from setup.ps1 / the wrapper scripts).
    // The bridge prefix was previously "couchlink-" which never matched
    // tracing-appender's actual filename format and silently produced
    // bundles with zero bridge logs.
    if let Ok(log_dir) = config::log_dir() {
        bundle_log_prefix(&log_dir, "couchlink.", &mut zip, opts)?;
        bundle_log_prefix(&log_dir, "setup-", &mut zip, opts)?;
    }

    zip.finish()?;

    let issue_url = crate::support::issue_url();
    println!("Wrote {}", out_path.display());
    println!("  manifest.json + doctor.txt + bridge logs");
    if pico_diag_captured {
        println!("  pico-diag.txt: captured");
    } else {
        println!("  pico-diag.txt: not captured -- Pico not in setup mode");
    }
    if crash_files.is_empty() {
        println!("  crashes/: none");
    } else {
        println!("  crashes/: {} files", crash_files.len());
    }
    if setup_transcripts.is_empty() {
        println!("  setup transcripts: none");
    } else {
        println!("  setup transcripts: {} files", setup_transcripts.len());
    }
    println!();
    println!("Wi-Fi password and SSID are not included. Safe to share.");
    println!();
    println!("Report this bundle at: {issue_url}");

    // Offer to open the issues page if stdin is a terminal.
    if console::Term::stdout().is_term() {
        let url = issue_url.clone();
        let open_it = tokio::task::spawn_blocking(move || {
            dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Open the issues page in your browser now?")
                .default(false)
                .interact()
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);

        if open_it {
            #[cfg(windows)]
            {
                let _ = tokio::process::Command::new("cmd")
                    .args(["/C", "start", "", url.as_str()])
                    .status()
                    .await;
            }
            #[cfg(not(windows))]
            let _ = url;
        }
    }

    Ok(())
}

fn bundle_log_prefix(
    log_dir: &std::path::Path,
    prefix: &str,
    zip: &mut ZipWriter<std::fs::File>,
    opts: SimpleFileOptions,
) -> Result<()> {
    if let Ok(entries) = std::fs::read_dir(log_dir) {
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(prefix) && n.ends_with(".log"))
                        .unwrap_or(false)
            })
            .collect();
        paths.sort();
        let take = 3.min(paths.len());
        let recent = &paths[paths.len() - take..];
        for p in recent {
            if let Some(name) = p.file_name() {
                if let Ok(bytes) = std::fs::read(p) {
                    zip.start_file(format!("logs/{}", name.to_string_lossy()), opts)?;
                    zip.write_all(&bytes)?;
                }
            }
        }
    }
    Ok(())
}

fn collect_crash_file_names() -> Vec<String> {
    let Ok(crash_dir) = config::crash_dir() else {
        return vec![];
    };
    if !crash_dir.is_dir() {
        return vec![];
    }
    let Ok(entries) = std::fs::read_dir(&crash_dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

fn collect_setup_transcript_names() -> Vec<String> {
    let Ok(log_dir) = config::log_dir() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&log_dir) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("setup-") && n.ends_with(".log"))
        .collect();
    names.sort();
    let take = 3.min(names.len());
    names[names.len() - take..].to_vec()
}

#[derive(Serialize)]
struct Manifest {
    bridge_version: &'static str,
    protocol_version: u8,
    cdc_protocol_version: u8,
    generated_at: String,
    os: String,
    windows_version: Option<String>,
    last_pico: Option<config::PicoIdentity>,
    setup_complete: bool,
    config_path: String,
    log_dir: String,
    report_url: String,
    pico_diag_captured: bool,
    crash_files: Vec<String>,
    setup_transcripts: Vec<String>,
    notes: Vec<&'static str>,
}

async fn build_manifest(
    pico_diag_captured: bool,
    crash_files: &[String],
    setup_transcripts: &[String],
) -> Result<Manifest> {
    let cfg = config::load().unwrap_or_default();
    Ok(Manifest {
        bridge_version: env!("CARGO_PKG_VERSION"),
        protocol_version: crate::protocol::PROTO_VERSION,
        cdc_protocol_version: crate::cdc::PROTO_VERSION,
        generated_at: Local::now().to_rfc3339(),
        os: std::env::consts::OS.to_string(),
        windows_version: windows_version().await,
        last_pico: cfg.last_pico,
        setup_complete: cfg.setup_complete,
        config_path: config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        log_dir: config::log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        report_url: crate::support::issue_url(),
        pico_diag_captured,
        crash_files: crash_files.to_vec(),
        setup_transcripts: setup_transcripts.to_vec(),
        notes: vec![
            "Wi-Fi credentials are NOT included.",
            "SSID is NOT included.",
            "Logs are filtered to the last 3 rotated files per prefix.",
        ],
    })
}

#[cfg(windows)]
async fn windows_version() -> Option<String> {
    let out = tokio::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(not(windows))]
async fn windows_version() -> Option<String> {
    None
}

// CDC access blocks; run it off the async runtime.
async fn try_capture_pico_diag() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let port = cdc::find_setup_port().ok()?;
        let mut p = cdc::PicoSetup::open_named(&port).ok()?;
        p.get_log_buffer().ok()
    })
    .await
    .ok()
    .flatten()
}

async fn run_doctor_silently() -> String {
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
