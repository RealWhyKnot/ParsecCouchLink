mod cdc;
mod cmd_auto;
mod cmd_bundle;
mod cmd_configure_wifi;
mod cmd_debug;
mod cmd_doctor;
mod cmd_flash;
mod cmd_home;
mod cmd_lab;
mod cmd_logs;
mod cmd_persona;
mod cmd_run;
mod cmd_setup;
mod cmd_test;
mod cmd_usb_diag;
mod config;
mod debug_packets;
mod diag_usb;
mod discovery;
mod firmware_version;
mod journal;
mod keyboard;
mod known_folders;
mod logfile;
mod net;
mod pico_cache;
mod pico_mode;
mod pico_state;
mod protocol;
mod support;
mod tui;
mod xinput;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "couchlink",
    version,
    about = "Parsec to retro-console bridge. Run with no arguments for the guided menu. \
             Run `couchlink setup` to walk through first-time setup. \
             Run `couchlink bundle` to gather diagnostics."
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

#[derive(Debug, Subcommand)]
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
    /// Switch a Pico to USB keyboard mode and stream the host keyboard to it.
    /// For console games that need a keyboard (e.g. Typing of the Dead on Dreamcast).
    Keyboard {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to debug input mode and stream XInput while capturing raw USB packets.
    #[command(hide = true)]
    DebugInput {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Automatically select a gamepad USB mode accepted by the console adapter.
    Auto {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Check every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Select the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Xbox 360 / XInput mode (the default persona).
    Xinput {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Try Xbox 360 and Xbox One modes and keep whichever one the adapter polls.
    Xbox {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Select the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Xbox 360 / XInput mode.
    Xbox360 {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Xbox One-compatible USB mode.
    Xboxone {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Hidden compatibility alias for older shortcuts.
    #[command(name = "controller", hide = true)]
    Controller {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Xbox-compatible mode for Dreamcast Maple adapters.
    Maple {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Try PS3, generic HID, and PS4 modes for USB4MAPLE-style adapters.
    Dinput {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to PS3 / DualShock 3 HID mode.
    Ps3 {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to PS4 / DualShock 4 HID mode.
    Ps4 {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to generic HID gamepad mode.
    #[command(name = "generic-hid", alias = "generic")]
    GenericHid {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Bluetooth HID gamepad mode.
    #[command(
        name = "bluetooth-hid",
        alias = "bluetooth",
        alias = "bt-hid",
        alias = "blueretro",
        alias = "n64-bluetooth"
    )]
    BluetoothHid {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Bluetooth HID with Xbox button ordering.
    #[command(name = "bluetooth-xbox", alias = "bt-xbox", alias = "blueretro-xbox")]
    BluetoothXbox {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// Switch a Pico to Bluetooth HID with PlayStation button ordering.
    #[command(
        name = "bluetooth-playstation",
        alias = "bt-playstation",
        alias = "bt-ps",
        alias = "blueretro-playstation"
    )]
    BluetoothPlaystation {
        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Switch every Pico currently visible on Wi-Fi.
        #[arg(long)]
        all: bool,

        /// Switch the persona but don't start streaming afterwards.
        #[arg(long)]
        no_stream: bool,
    },
    /// First-time setup wizard: flash the Pico, provision Wi-Fi, and check LAN discovery.
    Setup {
        /// Path to the .uf2 firmware to flash.
        #[arg(long)]
        uf2: Option<PathBuf>,
    },
    /// Run every diagnostic check; report PASS/WARN/FAIL/SKIP with hints.
    /// Exit codes: 0 clean, 1 warnings only, 2 hard fail, 3 setup incomplete.
    #[command(hide = true)]
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
    /// Recover Picos for streaming by checking Wi-Fi, setup USB, and BOOTSEL states.
    #[command(hide = true)]
    Recover,
    /// Reboot a setup-mode USB Pico into BOOTSEL firmware mode.
    #[command(hide = true)]
    Bootsel {
        /// Reboot every setup-mode USB Pico into BOOTSEL.
        #[arg(long)]
        all: bool,

        /// Select a setup-mode USB Pico by COM port. Repeat for more than one.
        #[arg(long = "port")]
        ports: Vec<String>,
    },
    /// Guided Pico debug/recovery menu. Can also switch between Wi-Fi, USB debug, and BOOTSEL modes.
    #[command(hide = true)]
    Debug {
        /// Show current Pico mode status and exit.
        #[arg(long)]
        status: bool,

        /// Ask a Wi-Fi Pico to reboot into USB debug mode.
        #[arg(long = "to-usb-debug")]
        to_usb_debug: bool,

        /// Ask a USB debug Pico to reboot into Wi-Fi/input mode.
        #[arg(long = "to-wifi")]
        to_wifi: bool,

        /// Ask a USB debug Pico to reboot into BOOTSEL firmware mode.
        #[arg(long = "to-bootsel")]
        to_bootsel: bool,

        /// Read setup-mode USB debug logs.
        #[arg(long)]
        logs: bool,

        /// Apply the selected action to every matching Pico.
        #[arg(long)]
        all: bool,

        /// Select a Wi-Fi Pico by IP when using --to-usb-debug.
        #[arg(long = "ip")]
        ips: Vec<String>,

        /// Select a setup-mode USB debug Pico by COM port for --to-wifi, --to-bootsel, or --logs.
        #[arg(long = "port")]
        ports: Vec<String>,
    },
    /// Run a single diagnostic test by name.
    ///
    /// Names: paths, xinput, startup, firewall, wifi-band, cdc, discover, usb
    /// (aliases: wifi = wifi-band, adapter = usb).
    #[command(hide = true)]
    Test {
        which: String,

        /// For supported tests, probe every matching device instead of the first one.
        #[arg(long)]
        all: bool,

        /// For `test cdc`, reboot setup-mode Pico(s) into Wi-Fi run mode after USB checks pass.
        #[arg(long)]
        reboot_to_run: bool,

        /// For `test discover` and `test usb`, probe a Pico by manual IP address.
        #[arg(long = "ip")]
        ips: Vec<String>,
    },
    /// Run unattended Pico hardware bench scenarios.
    #[command(hide = true)]
    Lab {
        /// Probe every visible Pico. This is also the default when no --pico selector is provided.
        #[arg(long)]
        all: bool,

        /// Select a Pico by UID, IP, or board name. Repeat to select more than one.
        #[arg(long = "pico")]
        picos: Vec<String>,

        /// Hardware scenario to run.
        #[arg(long, value_enum, default_value = "full")]
        scenario: cmd_lab::LabScenario,

        /// Number of times to repeat the selected scenario.
        #[arg(long, default_value_t = 1)]
        cycles: u32,

        /// Power-cycle method. Auto uses external power only when a configured probe passes.
        #[arg(long, value_enum, default_value = "auto")]
        power: cmd_lab::LabPower,

        /// Path to a .uf2 file, or a directory containing board-specific UF2 files.
        #[arg(long)]
        uf2: Option<PathBuf>,

        /// Write a machine-readable JSON report.
        #[arg(long)]
        json: Option<PathBuf>,

        /// Skip the BOOTSEL flash leg.
        #[arg(long)]
        no_flash: bool,
    },
    #[command(name = "lab-pnp-helper", hide = true)]
    LabPnpHelper {
        #[arg(long, value_enum, default_value = "disable-enable")]
        action: cmd_lab::PnpHelperAction,

        #[arg(long = "instance-id", required = true)]
        instance_ids: Vec<String>,

        #[arg(long, default_value_t = 2)]
        hold_seconds: u64,

        #[arg(long)]
        result_file: Option<PathBuf>,
    },
    /// Print where logs live, or tail the active log file.
    #[command(hide = true)]
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
            Some(Command::Keyboard {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Keyboard, picos, all, !no_stream).await,
            Some(Command::DebugInput {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Debug, picos, all, !no_stream).await,
            Some(Command::Auto {
                picos,
                all,
                no_stream,
            }) => cmd_auto::run(picos, all, !no_stream).await,
            Some(Command::Xinput {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Xinput, picos, all, !no_stream).await,
            Some(Command::Xbox {
                picos,
                all,
                no_stream,
            }) => cmd_auto::run_family(picos, all, !no_stream, cmd_auto::XBOX_FAMILY, "Xbox").await,
            Some(Command::Xbox360 {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Xinput, picos, all, !no_stream).await,
            Some(Command::Xboxone {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::XboxOne, picos, all, !no_stream).await,
            Some(Command::Controller {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Xinput, picos, all, !no_stream).await,
            Some(Command::Maple {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Maple, picos, all, !no_stream).await,
            Some(Command::Dinput {
                picos,
                all,
                no_stream,
            }) => {
                cmd_auto::run_family(
                    picos,
                    all,
                    !no_stream,
                    cmd_auto::PLAYSTATION_FAMILY,
                    "DInput / PlayStation",
                )
                .await
            }
            Some(Command::Ps3 {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Ps3, picos, all, !no_stream).await,
            Some(Command::Ps4 {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::Ps4, picos, all, !no_stream).await,
            Some(Command::GenericHid {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::GenericHid, picos, all, !no_stream).await,
            Some(Command::BluetoothHid {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::BluetoothHid, picos, all, !no_stream).await,
            Some(Command::BluetoothXbox {
                picos,
                all,
                no_stream,
            }) => cmd_persona::run(protocol::Persona::BluetoothXbox, picos, all, !no_stream).await,
            Some(Command::BluetoothPlaystation {
                picos,
                all,
                no_stream,
            }) => {
                cmd_persona::run(
                    protocol::Persona::BluetoothPlaystation,
                    picos,
                    all,
                    !no_stream,
                )
                .await
            }
            Some(Command::Setup { uf2 }) => cmd_setup::run(uf2).await,
            Some(Command::Doctor) => cmd_doctor::run().await,
            Some(Command::Flash { uf2, all, from_usb }) => cmd_flash::run(uf2, all, from_usb).await,
            Some(Command::ConfigureWifi) => cmd_configure_wifi::run().await,
            Some(Command::Recover) => cmd_run::run_recover_command().await,
            Some(Command::Bootsel { all, ports }) => {
                cmd_debug::run(cmd_debug::DebugOptions {
                    to_bootsel: true,
                    all,
                    ports,
                    ..cmd_debug::DebugOptions::default()
                })
                .await
            }
            Some(Command::Debug {
                status,
                to_usb_debug,
                to_wifi,
                to_bootsel,
                logs,
                all,
                ips,
                ports,
            }) => {
                cmd_debug::run(cmd_debug::DebugOptions {
                    status,
                    to_usb_debug,
                    to_wifi,
                    to_bootsel,
                    logs,
                    all,
                    ips,
                    ports,
                })
                .await
            }
            Some(Command::Test {
                which,
                all,
                reboot_to_run,
                ips,
            }) => cmd_test::run(&which, all, reboot_to_run, ips).await,
            Some(Command::Lab {
                all,
                picos,
                scenario,
                cycles,
                power,
                uf2,
                json,
                no_flash,
            }) => {
                let all = all || picos.is_empty();
                cmd_lab::run(cmd_lab::LabOptions {
                    all,
                    picos,
                    scenario,
                    cycles,
                    power,
                    uf2,
                    json,
                    no_flash,
                })
                .await
            }
            Some(Command::LabPnpHelper {
                action,
                instance_ids,
                hold_seconds,
                result_file,
            }) => cmd_lab::run_pnp_helper(action, instance_ids, hold_seconds, result_file),
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn lower_level_diagnostic_commands_are_hidden_from_help() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();

        assert!(help_lists_command(&help, "bundle"));
        for hidden in [
            "debug-input",
            "doctor",
            "recover",
            "bootsel",
            "debug",
            "test",
            "lab",
            "logs",
        ] {
            assert!(
                !help_lists_command(&help, hidden),
                "{hidden} should stay hidden from top-level help:\n{help}"
            );
        }
    }

    fn help_lists_command(help: &str, command: &str) -> bool {
        let command_with_space = format!("{command} ");
        help.lines().any(|line| {
            let line = line.trim_start();
            line == command || line.starts_with(&command_with_space)
        })
    }

    #[test]
    fn lab_command_parses_defaults_and_overrides() {
        let cli = Cli::try_parse_from([
            "couchlink",
            "lab",
            "--scenario",
            "power-cycle",
            "--cycles",
            "3",
            "--power",
            "pnp-restart",
            "--pico",
            "07D37EB6",
            "--no-flash",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Lab {
                all,
                picos,
                scenario,
                cycles,
                power,
                no_flash,
                ..
            }) => {
                assert!(!all);
                assert_eq!(picos, vec!["07D37EB6"]);
                assert_eq!(scenario, cmd_lab::LabScenario::PowerCycle);
                assert_eq!(cycles, 3);
                assert_eq!(power, cmd_lab::LabPower::PnpRestart);
                assert!(no_flash);
            }
            other => panic!("expected lab command, got {other:?}"),
        }
    }

    #[test]
    fn lab_command_parses_pnp_remove_power() {
        let cli = Cli::try_parse_from(["couchlink", "lab", "--power", "pnp-remove"]).unwrap();

        match cli.command {
            Some(Command::Lab { power, .. }) => {
                assert_eq!(power, cmd_lab::LabPower::PnpRemove);
            }
            other => panic!("expected lab command, got {other:?}"),
        }
    }

    #[test]
    fn lab_pnp_helper_command_parses_instance_ids() {
        let cli = Cli::try_parse_from([
            "couchlink",
            "lab-pnp-helper",
            "--action",
            "remove-rescan",
            "--hold-seconds",
            "3",
            "--instance-id",
            r"USB\VID_045E&PID_028E\E6613852837C242C",
        ])
        .unwrap();

        match cli.command {
            Some(Command::LabPnpHelper {
                action,
                instance_ids,
                hold_seconds,
                result_file,
            }) => {
                assert_eq!(action, cmd_lab::PnpHelperAction::RemoveRescan);
                assert_eq!(hold_seconds, 3);
                assert_eq!(result_file, None);
                assert_eq!(
                    instance_ids,
                    vec![r"USB\VID_045E&PID_028E\E6613852837C242C"]
                );
            }
            other => panic!("expected lab pnp helper command, got {other:?}"),
        }
    }

    #[test]
    fn maple_command_parses_target_and_no_stream() {
        let cli = Cli::try_parse_from(["couchlink", "maple", "--pico", "07D37EB6", "--no-stream"])
            .unwrap();

        match cli.command {
            Some(Command::Maple {
                picos,
                all,
                no_stream,
            }) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(!all);
                assert!(no_stream);
            }
            other => panic!("expected maple command, got {other:?}"),
        }
    }

    #[test]
    fn auto_command_parses_target_all_and_no_stream() {
        let cli = Cli::try_parse_from([
            "couchlink",
            "auto",
            "--pico",
            "07D37EB6",
            "--all",
            "--no-stream",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Auto {
                picos,
                all,
                no_stream,
            }) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(all);
                assert!(no_stream);
            }
            other => panic!("expected auto command, got {other:?}"),
        }
    }

    #[test]
    fn debug_input_command_parses_target_and_no_stream() {
        let cli = Cli::try_parse_from([
            "couchlink",
            "debug-input",
            "--pico",
            "07D37EB6",
            "--no-stream",
        ])
        .unwrap();

        match cli.command {
            Some(Command::DebugInput {
                picos,
                all,
                no_stream,
            }) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(!all);
                assert!(no_stream);
            }
            other => panic!("expected debug-input command, got {other:?}"),
        }
    }

    #[test]
    fn xinput_command_parses_target_and_no_stream() {
        let cli = Cli::try_parse_from(["couchlink", "xinput", "--pico", "07D37EB6", "--no-stream"])
            .unwrap();

        match cli.command {
            Some(Command::Xinput {
                picos,
                all,
                no_stream,
            }) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(!all);
                assert!(no_stream);
            }
            other => panic!("expected xinput command, got {other:?}"),
        }
    }

    #[test]
    fn controller_command_still_parses_as_hidden_alias() {
        let cli = Cli::try_parse_from([
            "couchlink",
            "controller",
            "--pico",
            "07D37EB6",
            "--no-stream",
        ])
        .unwrap();

        match cli.command {
            Some(Command::Controller {
                picos,
                all,
                no_stream,
            }) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(!all);
                assert!(no_stream);
            }
            other => panic!("expected controller alias command, got {other:?}"),
        }
    }

    #[test]
    fn dinput_command_parses_target_and_no_stream() {
        let cli = Cli::try_parse_from(["couchlink", "dinput", "--pico", "07D37EB6", "--no-stream"])
            .unwrap();

        match cli.command {
            Some(Command::Dinput {
                picos,
                all,
                no_stream,
            }) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(!all);
                assert!(no_stream);
            }
            other => panic!("expected dinput command, got {other:?}"),
        }
    }

    #[test]
    fn specific_gamepad_persona_commands_parse_target_and_no_stream() {
        for command in [
            "xbox",
            "xbox360",
            "xboxone",
            "ps3",
            "ps4",
            "generic-hid",
            "bluetooth-hid",
            "bluetooth-xbox",
            "bluetooth-playstation",
        ] {
            let cli =
                Cli::try_parse_from(["couchlink", command, "--pico", "07D37EB6", "--no-stream"])
                    .unwrap();
            match (command, cli.command) {
                (
                    "xbox",
                    Some(Command::Xbox {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "xbox360",
                    Some(Command::Xbox360 {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "xboxone",
                    Some(Command::Xboxone {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "ps3",
                    Some(Command::Ps3 {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "ps4",
                    Some(Command::Ps4 {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "generic-hid",
                    Some(Command::GenericHid {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "bluetooth-hid",
                    Some(Command::BluetoothHid {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "bluetooth-xbox",
                    Some(Command::BluetoothXbox {
                        picos,
                        all,
                        no_stream,
                    }),
                )
                | (
                    "bluetooth-playstation",
                    Some(Command::BluetoothPlaystation {
                        picos,
                        all,
                        no_stream,
                    }),
                ) => {
                    assert_eq!(picos, vec!["07D37EB6"]);
                    assert!(!all);
                    assert!(no_stream);
                }
                other => panic!("unexpected parse result: {other:?}"),
            }
        }
    }
}
