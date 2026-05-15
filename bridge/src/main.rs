mod cdc;
mod cmd_bundle;
mod cmd_configure_wifi;
mod cmd_doctor;
mod cmd_flash;
mod cmd_logs;
mod cmd_run;
mod cmd_setup;
mod cmd_test;
mod config;
mod discovery;
mod known_folders;
mod logfile;
mod network;
mod protocol;
mod xinput;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "couchlink",
    version,
    about = "Parsec to retro-console bridge. Run with no arguments to stream. \
             Run `couchlink setup` to walk through first-time setup. \
             Run `couchlink doctor` to diagnose problems."
)]
struct Cli {
    /// Verbosity. -v for debug, -vv for trace.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Skip the rolling log file; log to terminal only.
    #[arg(long, global = true)]
    no_log_file: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the bridge. This is the default.
    Run,
    /// First-time setup wizard: flash the Pico, provision Wi-Fi, smoke test.
    Setup {
        /// Path to the .uf2 firmware to flash.
        #[arg(long)]
        uf2: Option<PathBuf>,
    },
    /// Run every diagnostic check; report PASS/WARN/FAIL/SKIP with hints.
    /// Exit codes: 0 clean, 1 warnings only, 2 hard fail, 3 setup incomplete.
    Doctor,
    /// Find a Pico in BOOTSEL mode and copy a UF2 onto it.
    Flash {
        /// Path to the .uf2 firmware to flash.
        #[arg(short, long)]
        uf2: PathBuf,
    },
    /// Re-push Wi-Fi credentials to a Pico in setup mode via USB-CDC.
    ConfigureWifi,
    /// Run a single diagnostic test by name.
    ///
    /// Names: xinput, paths, firewall, startup, discover, cdc, ack-identity
    Test { which: String },
    /// Print where logs live, or tail the active log file.
    Logs {
        /// Tail the current log file instead of printing its path.
        #[arg(long)]
        tail: bool,
    },
    /// Produce a support-bundle ZIP that's safe to send for remote debugging.
    Bundle {
        /// Output path. Default: a timestamped ZIP in the current directory.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let _log_guard = logfile::init(cli.verbose, !cli.no_log_file)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async move {
        match cli.command.unwrap_or(Command::Run) {
            Command::Run => cmd_run::run().await,
            Command::Setup { uf2 } => cmd_setup::run(uf2).await,
            Command::Doctor => cmd_doctor::run().await,
            Command::Flash { uf2 } => cmd_flash::run(uf2).await,
            Command::ConfigureWifi => cmd_configure_wifi::run().await,
            Command::Test { which } => cmd_test::run(&which).await,
            Command::Logs { tail } => cmd_logs::run(tail).await,
            Command::Bundle { output } => cmd_bundle::run(output).await,
        }
    })
}
