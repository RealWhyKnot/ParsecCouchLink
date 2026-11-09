use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::{cmd_debug, cmd_lab, cmd_run};

#[derive(Parser)]
#[command(
    name = "couchlink",
    version = crate::VERSION,
    about = "Parsec to retro-console bridge. Run with no arguments for the guided menu. \
             Run `couchlink setup` to walk through first-time setup. \
             Run `couchlink bundle` to gather diagnostics."
)]
pub(crate) struct Cli {
    #[arg(help = "Verbosity. -v for debug, -vv for trace")]
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub(crate) verbose: u8,

    #[arg(help = "Skip the rolling log file; log to terminal only")]
    #[arg(long, global = true)]
    pub(crate) no_log_file: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    #[command(
        about = "Run the bridge directly. With no saved layout, this streams one controller to \
                 one Pico"
    )]
    Run(RunArgs),
    #[command(
        about = "Switch a Pico to USB keyboard mode and stream the host keyboard to it. For \
                 console games that need a keyboard (e.g. Typing of the Dead on Dreamcast)"
    )]
    Keyboard(SwitchArgs),
    #[command(
        about = "Switch a Pico to debug input mode and stream XInput while capturing raw USB \
                 packets"
    )]
    #[command(hide = true)]
    DebugInput(SwitchArgs),
    #[command(about = "Automatically select a gamepad USB mode accepted by the console adapter")]
    Auto(AutoArgs),
    #[command(about = "Switch a Pico to Xbox 360 / XInput mode (the default persona)")]
    Xinput(SwitchArgs),
    #[command(about = "Try Xbox 360 and Xbox One modes and keep whichever one the adapter polls")]
    Xbox(XboxArgs),
    #[command(about = "Switch a Pico to Xbox 360 / XInput mode")]
    Xbox360(SwitchArgs),
    #[command(about = "Switch a Pico to Xbox One-compatible USB mode")]
    Xboxone(SwitchArgs),
    #[command(about = "Hidden compatibility alias for older shortcuts")]
    #[command(name = "controller", hide = true)]
    Controller(SwitchArgs),
    #[command(about = "Switch a Pico to Xbox-compatible mode for Dreamcast Maple adapters")]
    Maple(SwitchArgs),
    #[command(about = "Try PS3, generic HID, and PS4 modes for USB4MAPLE-style adapters")]
    Dinput(SwitchArgs),
    #[command(about = "Switch a Pico to PS3 / DualShock 3 HID mode")]
    Ps3(SwitchArgs),
    #[command(about = "Switch a Pico to PS4 / DualShock 4 HID mode")]
    Ps4(SwitchArgs),
    #[command(about = "Switch a Pico to generic HID gamepad mode")]
    #[command(name = "generic-hid", alias = "generic")]
    GenericHid(SwitchArgs),
    #[command(about = "Switch a Pico to Bluetooth mode")]
    #[command(
        name = "bluetooth",
        alias = "bluetooth-hid",
        alias = "bt",
        alias = "bt-hid",
        alias = "blueretro",
        alias = "n64-bluetooth"
    )]
    BluetoothHid(SwitchArgs),
    #[command(about = "Switch a Pico to Bluetooth as an Xbox Wireless Controller")]
    #[command(name = "bluetooth-xbox", alias = "bt-xbox", alias = "blueretro-xbox")]
    BluetoothXbox(SwitchArgs),
    #[command(about = "Switch a Pico to Bluetooth as a DualShock 4 Wireless Controller")]
    #[command(
        name = "bluetooth-playstation",
        alias = "bt-playstation",
        alias = "bt-ps",
        alias = "blueretro-playstation"
    )]
    BluetoothPlaystation(SwitchArgs),
    #[command(
        about = "First-time setup wizard: flash the Pico, provision Wi-Fi, and check LAN \
                 discovery"
    )]
    Setup {
        #[arg(help = "Path to the .uf2 firmware to flash")]
        #[arg(long)]
        uf2: Option<PathBuf>,
    },
    #[command(
        about = "Run every diagnostic check; report PASS/WARN/FAIL/SKIP with hints. Exit codes: \
                 0 clean, 1 warnings only, 2 hard fail, 3 setup incomplete"
    )]
    #[command(hide = true)]
    Doctor,
    #[command(
        about = "Find a Pico in BOOTSEL mode and copy a UF2 onto it. With no --uf2, the \
                 matching couchlink-pico2w.uf2 / couchlink-picow.uf2 is picked from next to the \
                 executable (or its `dist/` folder)"
    )]
    Flash {
        #[arg(help = "Path to a .uf2 file, or a directory containing one. Optional")]
        #[arg(short, long)]
        uf2: Option<PathBuf>,

        #[arg(help = "Flash every Pico currently visible in BOOTSEL mode")]
        #[arg(long)]
        all: bool,

        #[arg(help = "Ask setup-mode USB-CDC firmware to reboot into BOOTSEL before flashing")]
        #[arg(long)]
        from_usb: bool,
    },
    #[command(about = "Re-push Wi-Fi credentials to a Pico in setup mode via USB-CDC")]
    ConfigureWifi,
    #[command(
        about = "Recover Picos for streaming by checking Wi-Fi, setup USB, and BOOTSEL states"
    )]
    #[command(hide = true)]
    Recover,
    #[command(about = "Reboot a setup-mode USB Pico into BOOTSEL firmware mode")]
    #[command(hide = true)]
    Bootsel {
        #[arg(help = "Reboot every setup-mode USB Pico into BOOTSEL")]
        #[arg(long)]
        all: bool,

        #[arg(help = "Select a setup-mode USB Pico by COM port. Repeat for more than one")]
        #[arg(long = "port")]
        ports: Vec<String>,
    },
    #[command(
        about = "Guided Pico debug/recovery menu. Can also switch between Wi-Fi, USB debug, and \
                 BOOTSEL modes"
    )]
    #[command(hide = true)]
    Debug(DebugArgs),
    #[command(
        about = "Run a single diagnostic test by name",
        long_about = "Run a single diagnostic test by name.\n\n\
                      Names: paths, xinput, startup, firewall, wifi-band, cdc, discover, usb \
                      (aliases: wifi = wifi-band, adapter = usb)."
    )]
    #[command(hide = true)]
    Test {
        which: String,

        #[arg(help = "For supported tests, probe every matching device instead of the first one")]
        #[arg(long)]
        all: bool,

        #[arg(
            help = "For `test cdc`, reboot setup-mode Pico(s) into Wi-Fi run mode after USB checks \
                    pass"
        )]
        #[arg(long)]
        reboot_to_run: bool,

        #[arg(help = "For `test discover` and `test usb`, probe a Pico by manual IP address")]
        #[arg(long = "ip")]
        ips: Vec<String>,
    },
    #[command(about = "Run unattended Pico hardware bench scenarios")]
    #[command(hide = true)]
    Lab(LabArgs),
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
    #[command(about = "Print where logs live, or tail the active log file")]
    #[command(hide = true)]
    Logs {
        #[arg(help = "Tail the current log file instead of printing its path")]
        #[arg(long)]
        tail: bool,
    },
    #[command(about = "Produce a support-bundle ZIP that's safe to send with a bug report")]
    Bundle {
        #[arg(help = "Output path. Default: a timestamped ZIP in the current directory")]
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
pub(crate) struct SwitchArgs {
    #[arg(help = "Select a Pico by UID, IP, or board name. Repeat to select more than one")]
    #[arg(long = "pico")]
    pub(crate) picos: Vec<String>,

    #[arg(help = "Switch every Pico currently visible on Wi-Fi")]
    #[arg(long)]
    pub(crate) all: bool,

    #[arg(help = "Switch the persona but don't start streaming afterwards")]
    #[arg(long)]
    pub(crate) no_stream: bool,
}

#[derive(Debug, Args)]
pub(crate) struct AutoArgs {
    #[arg(help = "Select a Pico by UID, IP, or board name. Repeat to select more than one")]
    #[arg(long = "pico")]
    pub(crate) picos: Vec<String>,

    #[arg(help = "Check every Pico currently visible on Wi-Fi")]
    #[arg(long)]
    pub(crate) all: bool,

    #[arg(help = "Select the persona but don't start streaming afterwards")]
    #[arg(long)]
    pub(crate) no_stream: bool,
}

#[derive(Debug, Args)]
pub(crate) struct XboxArgs {
    #[arg(help = "Select a Pico by UID, IP, or board name. Repeat to select more than one")]
    #[arg(long = "pico")]
    pub(crate) picos: Vec<String>,

    #[arg(help = "Switch every Pico currently visible on Wi-Fi")]
    #[arg(long)]
    pub(crate) all: bool,

    #[arg(help = "Select the persona but don't start streaming afterwards")]
    #[arg(long)]
    pub(crate) no_stream: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    #[arg(help = "Discover every running Pico and map Controller 1, 2, ... in order")]
    #[arg(long)]
    pub(crate) all: bool,

    #[arg(
        help = "Select one Pico by UID, IP, or board name. Repeat to select more than one. If \
                an IP is given and broadcast discovery misses it, the bridge probes that IP \
                directly"
    )]
    #[arg(long = "pico")]
    pub(crate) picos: Vec<String>,

    #[arg(
        help = "Explicit route in the form 1=07D37EB6 or 2=192.168.50.4. Repeat for more \
                routes. IP targets are probed directly if broadcast discovery misses them"
    )]
    #[arg(long = "route")]
    pub(crate) routes: Vec<String>,

    #[arg(help = "Use the routing layout saved by the guided menu")]
    #[arg(long)]
    pub(crate) use_saved: bool,

    #[arg(help = "Seconds between visible traffic status updates")]
    #[arg(long, default_value_t = 2)]
    pub(crate) status_seconds: u64,

    #[arg(help = "Seconds to collect Pico discovery replies before routing")]
    #[arg(long, default_value_t = 5)]
    pub(crate) discover_seconds: u64,

    #[arg(help = "Suppress live traffic status output")]
    #[arg(long)]
    pub(crate) quiet: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DebugArgs {
    #[arg(help = "Show current Pico mode status and exit")]
    #[arg(long)]
    pub(crate) status: bool,

    #[command(flatten)]
    pub(crate) reboot: DebugRebootArgs,

    #[arg(help = "Read setup-mode USB debug logs")]
    #[arg(long)]
    pub(crate) logs: bool,

    #[arg(help = "Apply the selected action to every matching Pico")]
    #[arg(long)]
    pub(crate) all: bool,

    #[arg(help = "Select a Wi-Fi Pico by IP when using --to-usb-debug")]
    #[arg(long = "ip")]
    pub(crate) ips: Vec<String>,

    #[arg(
        help = "Select a setup-mode USB debug Pico by COM port for --to-wifi, --to-bootsel, or \
                --logs"
    )]
    #[arg(long = "port")]
    pub(crate) ports: Vec<String>,
}

#[derive(Debug, Args)]
pub(crate) struct DebugRebootArgs {
    #[arg(help = "Ask a Wi-Fi Pico to reboot into USB debug mode")]
    #[arg(long = "to-usb-debug")]
    pub(crate) to_usb_debug: bool,

    #[arg(help = "Ask a USB debug Pico to reboot into Wi-Fi/input mode")]
    #[arg(long = "to-wifi")]
    pub(crate) to_wifi: bool,

    #[arg(help = "Ask a USB debug Pico to reboot into BOOTSEL firmware mode")]
    #[arg(long = "to-bootsel")]
    pub(crate) to_bootsel: bool,
}

#[derive(Debug, Args)]
pub(crate) struct LabArgs {
    #[arg(
        help = "Probe every visible Pico. This is also the default when no --pico selector is \
                provided"
    )]
    #[arg(long)]
    pub(crate) all: bool,

    #[arg(help = "Select a Pico by UID, IP, or board name. Repeat to select more than one")]
    #[arg(long = "pico")]
    pub(crate) picos: Vec<String>,

    #[arg(help = "Hardware scenario to run")]
    #[arg(long, value_enum, default_value = "full")]
    pub(crate) scenario: cmd_lab::LabScenario,

    #[arg(help = "Number of times to repeat the selected scenario")]
    #[arg(long, default_value_t = 1)]
    pub(crate) cycles: u32,

    #[arg(
        help = "Power-cycle method. Auto uses external power only when a configured probe \
                passes"
    )]
    #[arg(long, value_enum, default_value = "auto")]
    pub(crate) power: cmd_lab::LabPower,

    #[arg(help = "Path to a .uf2 file, or a directory containing board-specific UF2 files")]
    #[arg(long)]
    pub(crate) uf2: Option<PathBuf>,

    #[arg(help = "Write a machine-readable JSON report")]
    #[arg(long)]
    pub(crate) json: Option<PathBuf>,

    #[arg(help = "Skip the BOOTSEL flash leg")]
    #[arg(long)]
    pub(crate) no_flash: bool,
}

impl From<RunArgs> for cmd_run::RunOptions {
    fn from(args: RunArgs) -> Self {
        Self {
            all: args.all,
            picos: args.picos,
            routes: args.routes,
            use_saved: args.use_saved,
            status_seconds: args.status_seconds,
            discover_seconds: args.discover_seconds,
            quiet: args.quiet,
        }
    }
}

impl From<DebugArgs> for cmd_debug::DebugOptions {
    fn from(args: DebugArgs) -> Self {
        Self {
            actions: cmd_debug::DebugAction::chosen(&[
                (args.status, cmd_debug::DebugAction::Status),
                (args.reboot.to_usb_debug, cmd_debug::DebugAction::ToUsbDebug),
                (args.reboot.to_wifi, cmd_debug::DebugAction::ToWifi),
                (args.reboot.to_bootsel, cmd_debug::DebugAction::ToBootsel),
                (args.logs, cmd_debug::DebugAction::Logs),
            ]),
            all: args.all,
            ips: args.ips,
            ports: args.ports,
        }
    }
}

impl From<LabArgs> for cmd_lab::LabOptions {
    fn from(args: LabArgs) -> Self {
        Self {
            all: args.all || args.picos.is_empty(),
            picos: args.picos,
            scenario: args.scenario,
            cycles: args.cycles,
            power: args.power,
            uf2: args.uf2,
            json: args.json,
            no_flash: args.no_flash,
        }
    }
}
