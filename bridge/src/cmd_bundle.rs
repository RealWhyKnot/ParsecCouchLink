//! `couchlink bundle` -- produce a ZIP of recent logs, a doctor re-run,
//! and a manifest.json with non-sensitive system info. Intended to be
//! attached to a DM when something's not working.
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

use crate::config;

pub async fn run(output: Option<PathBuf>) -> Result<()> {
    let manifest = build_manifest().await?;
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

    let log_dir = config::log_dir().ok();
    if let Some(dir) = log_dir.as_deref() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            paths.sort();
            // Take only the most recent N to keep the bundle small.
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
    }

    zip.finish()?;
    println!("wrote {}", out_path.display());
    println!(
        "Bundle contents: manifest.json, doctor.txt, logs/ (last 3 files). \
         No Wi-Fi credentials, no SSID. Safe to send."
    );
    Ok(())
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
    notes: Vec<&'static str>,
}

async fn build_manifest() -> Result<Manifest> {
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
        notes: vec![
            "Wi-Fi credentials are NOT included.",
            "SSID is NOT included.",
            "Logs are filtered to the last 3 rotated files.",
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

async fn run_doctor_silently() -> String {
    // Doctor prints to stdout via println!; we don't capture that here.
    // For the bundle we instead run each check and format the result
    // ourselves into a plain-text report.
    use crate::cmd_doctor::*;
    let checks: Vec<(
        &str,
        std::pin::Pin<Box<dyn std::future::Future<Output = CheckResult>>>,
    )> = vec![
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
