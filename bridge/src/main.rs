mod cdc;
mod cmd_bundle;
mod cmd_configure_wifi;
mod cmd_doctor;
mod cmd_flash;
mod cmd_home;
mod cmd_logs;
mod cmd_run;
mod cmd_setup;
mod cmd_test;
mod config;
mod diag_usb;
mod discovery;
mod firmware_version;
mod journal;
mod known_folders;
mod logfile;
mod protocol;
mod support;
mod xinput;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "couchlink",
    version,
    about = "Parsec to retro-console bridge. Run with no arguments for the guided menu. \
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
    /// Run the bridge directly. With no saved layout, this streams one controller to one Pico.
    Run {
        /// Discover every running Pico and map Controller 1, 2, ... in order.
        #[arg(long)]
        all: bool,

        /// Select one Pico by UID, IP, or board name. Repeat to select more than one.
        /// If an IP is given and broadcast discovery misses it, the bridge probes that IP directly.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Explicit route in the form 1=07D37EB6 or 2=192.168.50.4. Repeat for more routes.
        /// IP targets are probed directly if broadcast discovery misses them.
        #[arg(long = "route")]
        routes: Vec<String>,

        /// Use the routing layout saved by the guided menu.
        #[arg(long)]
        use_saved: bool,

        /// Seconds between visible traffic status updates.
        #[arg(long, default_value_t = 2)]
        status_seconds: u64,

        /// Seconds to collect Pico discovery replies before routing.
        #[arg(long, default_value_t = 5)]
        discover_seconds: u64,

        /// Suppress live traffic status output.
        #[arg(long)]
        quiet: bool,
    },
    /// First-time setup wizard: flash the Pico, provision Wi-Fi, and check LAN discovery.
    Setup {
        /// Path to the .uf2 firmware to flash.
        #[arg(long)]
        uf2: Option<PathBuf>,
    },
    /// Run every diagnostic check; report PASS/WARN/FAIL/SKIP with hints.
    /// Exit codes: 0 clean, 1 warnings only, 2 hard fail, 3 setup incomplete.
    Doctor,
    /// Find a Pico in BOOTSEL mode and copy a UF2 onto it. With no --uf2,
    /// the matching couchlink-pico2w.uf2 / couchlink-picow.uf2 is picked
    /// from next to the executable (or its `dist/` folder).
    Flash {
        /// Path to a .uf2 file, or a directory containing one. Optional.
        #[arg(short, long)]
        uf2: Option<PathBuf>,

        /// Flash every Pico currently visible in BOOTSEL mode.
        #[arg(long)]
        all: bool,

        /// Ask setup-mode USB-CDC firmware to reboot into BOOTSEL before flashing.
        #[arg(long)]
        from_usb: bool,
    },
    /// Re-push Wi-Fi credentials to a Pico in setup mode via USB-CDC.
    ConfigureWifi,
    /// Run a single diagnostic test by name.
    ///
    /// Names: xinput, paths, firewall, startup, discover, cdc, ack-identity
    Test {
        which: String,

        /// For supported tests, probe every matching device instead of the first one.
        #[arg(long)]
        all: bool,

        /// For `test cdc`, reboot setup-mode Pico(s) into Wi-Fi run mode after USB checks pass.
        #[arg(long)]
        reboot_to_run: bool,

        /// For `test discover`, probe a Pico by manual IP address.
        #[arg(long = "ip")]
        ips: Vec<String>,
    },
    /// Print where logs live, or tail the active log file.
    Logs {
        /// Tail the current log file instead of printing its path.
        #[arg(long)]
        tail: bool,
    },
    /// Produce a support-bundle ZIP that's safe to send with a bug report.
    Bundle {
        /// Output path. Default: a timestamped ZIP in the current directory.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    let log_guard = match logfile::init(cli.verbose, !cli.no_log_file) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("failed to initialize logging: {e:#}");
            std::process::exit(2);
        }
    };
    journal::init();
    install_panic_hook();

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("tokio runtime build failed: {e:#}");
            drop(log_guard);
            support::print_help_footer();
            std::process::exit(2);
        }
    };

    let result: anyhow::Result<()> = rt.block_on(async move {
        match cli.command {
            None => cmd_home::run().await,
            Some(Command::Run {
                all,
                picos,
                routes,
                use_saved,
                status_seconds,
                discover_seconds,
                quiet,
            }) => {
                cmd_run::run(cmd_run::RunOptions {
                    all,
                    picos,
                    routes,
                    use_saved,
                    status_seconds,
                    discover_seconds,
                    quiet,
                })
                .await
            }
            Some(Command::Setup { uf2 }) => cmd_setup::run(uf2).await,
            Some(Command::Doctor) => cmd_doctor::run().await,
            Some(Command::Flash { uf2, all, from_usb }) => cmd_flash::run(uf2, all, from_usb).await,
            Some(Command::ConfigureWifi) => cmd_configure_wifi::run().await,
            Some(Command::Test {
                which,
                all,
                reboot_to_run,
                ips,
            }) => cmd_test::run(&which, all, reboot_to_run, ips).await,
            Some(Command::Logs { tail }) => cmd_logs::run(tail).await,
            Some(Command::Bundle { output }) => cmd_bundle::run(output).await,
        }
    });

    drop(log_guard);

    if let Err(e) = result {
        eprintln!();
        eprintln!("couchlink exited with an error:");
        eprintln!("  {e:#}");
        support::print_help_footer();
        std::process::exit(1);
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    // Chain the default hook so the user still sees the panic message on stderr,
    // then append our footer so they know where to find logs and how to report.
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_crash_file(info);
        default_hook(info);
        support::print_help_footer();
    }));
}

fn write_crash_file(info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    use std::io::Write;
    let dir = crate::config::crash_dir().map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("couchlink-{stamp}.txt"));
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "couchlink panic")?;
    writeln!(f, "version: {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(f, "time (UTC): {}", chrono::Utc::now().to_rfc3339())?;
    if let Some(loc) = info.location() {
        writeln!(
            f,
            "location: {}:{}:{}",
            loc.file(),
            loc.line(),
            loc.column()
        )?;
    }
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .map(String::from)
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string());
    writeln!(f, "message: {payload}")?;
    writeln!(f, "---- backtrace ----")?;
    writeln!(f, "{}", std::backtrace::Backtrace::force_capture())?;
    Ok(())
}
