#[macro_use]
mod console;
mod convert;
#[cfg(test)]
mod test_diag;

mod cdc;
mod cli;
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
mod modes;
mod net;
mod pico_cache;
mod pico_mode;
mod pico_state;
mod protocol;
mod support;
mod tui;
mod udp_auth;
mod xinput;

use clap::Parser;

use crate::cli::{Cli, Command, SwitchArgs};
use crate::modes::{AfterSwitch, BootselEntry, PicoScope};

pub(crate) const VERSION: &str = match option_env!("COUCHLINK_VERSION") {
    Some(stamp) => stamp,
    None => env!("CARGO_PKG_VERSION"),
};

fn main() {
    let cli = Cli::parse();
    let log_guard = match logfile::init(cli.verbose, !cli.no_log_file) {
        Ok(g) => g,
        Err(e) => {
            errln!("failed to initialize logging: {e:#}");
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
            Some(command) => dispatch(command).await,
        }
    });

    drop(log_guard);

    if let Err(e) = result {
        errln!();
        errln!("couchlink exited with an error:");
        errln!("  {e:#}");
        support::print_help_footer();
        std::process::exit(1);
    }
}

async fn switch(persona: protocol::Persona, args: SwitchArgs) -> anyhow::Result<()> {
    cmd_persona::run(
        persona,
        args.picos,
        PicoScope::from_all_flag(args.all),
        AfterSwitch::from_no_stream_flag(args.no_stream),
    )
    .await
}

async fn dispatch(command: Command) -> anyhow::Result<()> {
    use protocol::Persona;

    match command {
        Command::Run(args) => cmd_run::run(args.into()).await,
        Command::Keyboard(args) => switch(Persona::Keyboard, args).await,
        Command::DebugInput(args) => switch(Persona::Debug, args).await,
        Command::Auto(args) => {
            cmd_auto::run(
                args.picos,
                PicoScope::from_all_flag(args.all),
                AfterSwitch::from_no_stream_flag(args.no_stream),
            )
            .await
        }
        Command::Xinput(args) | Command::Xbox360(args) | Command::Controller(args) => {
            switch(Persona::Xinput, args).await
        }
        Command::Xbox(args) => {
            cmd_auto::run_family(
                args.picos,
                PicoScope::from_all_flag(args.all),
                AfterSwitch::from_no_stream_flag(args.no_stream),
                cmd_auto::XBOX_FAMILY,
                "Xbox",
            )
            .await
        }
        Command::Xboxone(args) => switch(Persona::XboxOne, args).await,
        Command::Maple(args) => switch(Persona::Maple, args).await,
        Command::Dinput(args) => {
            cmd_auto::run_family(
                args.picos,
                PicoScope::from_all_flag(args.all),
                AfterSwitch::from_no_stream_flag(args.no_stream),
                cmd_auto::PLAYSTATION_FAMILY,
                "DInput / PlayStation",
            )
            .await
        }
        Command::Ps3(args) => switch(Persona::Ps3, args).await,
        Command::Ps4(args) => switch(Persona::Ps4, args).await,
        Command::GenericHid(args) => switch(Persona::GenericHid, args).await,
        Command::BluetoothHid(args) => switch(Persona::BluetoothHid, args).await,
        Command::BluetoothXbox(args) => switch(Persona::BluetoothXbox, args).await,
        Command::BluetoothPlaystation(args) => switch(Persona::BluetoothPlaystation, args).await,
        Command::Setup { uf2 } => cmd_setup::run(uf2).await,
        Command::Doctor => cmd_doctor::run().await,
        Command::Flash { uf2, all, from_usb } => {
            cmd_flash::run(
                uf2,
                PicoScope::from_all_flag(all),
                BootselEntry::from_usb_flag(from_usb),
            )
            .await
        }
        Command::ConfigureWifi => cmd_configure_wifi::run().await,
        Command::Recover => cmd_run::run_recover_command().await,
        Command::Bootsel { all, ports } => {
            cmd_debug::run(cmd_debug::DebugOptions {
                actions: vec![cmd_debug::DebugAction::ToBootsel],
                all,
                ports,
                ..cmd_debug::DebugOptions::default()
            })
            .await
        }
        Command::Debug(args) => cmd_debug::run(args.into()).await,
        Command::Test {
            which,
            all,
            reboot_to_run,
            ips,
        } => cmd_test::run(&which, PicoScope::from_all_flag(all), reboot_to_run, ips).await,
        Command::Lab(args) => cmd_lab::run(args.into()).await,
        Command::LabPnpHelper {
            action,
            instance_ids,
            hold_seconds,
            result_file,
        } => cmd_lab::run_pnp_helper(action, &instance_ids, hold_seconds, result_file),
        Command::Logs { tail } => cmd_logs::run(tail).await,
        Command::Bundle { output } => cmd_bundle::run(output).await,
    }
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
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
    writeln!(f, "version: {VERSION}")?;
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
mod tests;
