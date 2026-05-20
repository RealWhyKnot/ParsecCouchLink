//! `couchlink tunnel ...` subcommand.
//!
//! Mints / shows / disables a remote-debug tunnel session pair on the
//! tunnel server, stashed in `config.toml` under `[telemetry]`. The actual
//! run-time WS connection happens from `cmd_run` when the bridge starts.

use anyhow::{Context, Result};
use console::style;

use crate::config::{self, TelemetryConfig};
use crate::telemetry;

const DEFAULT_SERVER: &str = "https://couchlink.whyknot.dev";

pub async fn start(server: Option<String>) -> Result<()> {
    let server = server
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
        .trim_end_matches('/')
        .to_string();
    eprintln!("tunnel: minting a new session on {server} ...");

    let mint = telemetry::mint_session(&server)
        .await
        .with_context(|| format!("contacting tunnel at {server}"))?;

    let mut cfg = config::load().unwrap_or_default();
    cfg.telemetry = Some(TelemetryConfig {
        server: server.clone(),
        write_token: mint.write_token,
        view_token: mint.view_token,
    });
    config::save(&cfg).context("saving session into config.toml")?;

    eprintln!();
    eprintln!("{}", style("tunnel session ready.").bold().green());
    eprintln!("  server   : {server}");
    eprintln!("  view url : {}", mint.view_url);
    eprintln!();
    eprintln!("Share the view URL with whoever is helping you debug. Anyone holding it can");
    eprintln!("run the allowlisted commands listed in USAGE.md against this bridge.");
    eprintln!("Restart the bridge (or run `couchlink tunnel disable`) to revoke the URL.");
    Ok(())
}

pub async fn show() -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    match cfg.telemetry {
        Some(t) if !t.write_token.is_empty() => {
            eprintln!("server   : {}", t.server);
            eprintln!("view url : {}", t.view_url());
            Ok(())
        }
        _ => {
            eprintln!("no tunnel session configured. Run `couchlink tunnel start` to mint one.");
            Ok(())
        }
    }
}

pub async fn disable() -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    if cfg.telemetry.is_none() {
        eprintln!("no tunnel session was active.");
        return Ok(());
    }
    cfg.telemetry = None;
    config::save(&cfg)?;
    eprintln!("tunnel session removed from config. Restart the bridge to apply.");
    Ok(())
}
