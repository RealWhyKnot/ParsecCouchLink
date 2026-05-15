//! `couchlink test <which>` -- run a single check from the doctor ladder.

use anyhow::{anyhow, Result};

use crate::cmd_doctor::{
    check_24ghz_warning, check_cdc, check_discover, check_firewall, check_paths,
    check_startup_shortcut, check_xinput, CheckResult,
};

pub async fn run(which: &str) -> Result<()> {
    let result = match which {
        "paths" => check_paths().await,
        "xinput" => check_xinput().await,
        "startup" => check_startup_shortcut().await,
        "firewall" => check_firewall().await,
        "wifi-band" | "wifi" => check_24ghz_warning().await,
        "cdc" => check_cdc().await,
        "discover" => check_discover().await,
        other => {
            return Err(anyhow!(
                "unknown test: {other}. Available: paths, xinput, startup, firewall, wifi-band, cdc, discover"
            ));
        }
    };
    match result {
        CheckResult::Pass(m) => {
            println!("PASS  {}", m);
            Ok(())
        }
        CheckResult::Warn(m) => {
            println!("WARN  {}", m);
            std::process::exit(1);
        }
        CheckResult::Skip(m) => {
            println!("SKIP  {}", m);
            Ok(())
        }
        CheckResult::Fail(m, hint) => {
            println!("FAIL  {}", m);
            println!("hint: {}", hint);
            std::process::exit(2);
        }
    }
}
