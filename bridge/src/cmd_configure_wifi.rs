//! `ptd-bridge configure-wifi` -- re-provision a Pico that's in setup mode
//! over USB-CDC. Prompts for SSID + password, sends `SET_WIFI`, then
//! `REBOOT_TO_RUN`.
//!
//! The password is read with `dialoguer::Password` (no echo), held in a
//! `String` only as long as needed, and zeroized on `Drop` via the helper
//! in `cdc.rs`. Neither SSID nor password ever hits disk.

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Input, Password};
use zeroize::Zeroize;

use crate::cdc;

pub async fn run() -> Result<()> {
    println!("ptd-bridge configure-wifi");
    println!();
    println!(
        "Looking for a Pico in setup mode (VID 0x{:04X}, PID 0x{:04X})...",
        cdc::SETUP_VID,
        cdc::SETUP_PID
    );

    let port = cdc::find_setup_port()
        .context("no Pico in setup mode. Hold BOOTSEL and re-flash, or run `ptd-bridge setup`.")?;
    println!("Found Pico on {port}");

    // dialoguer is blocking; isolate it from the async runtime.
    let mut creds = tokio::task::spawn_blocking(prompt_credentials).await??;

    let mut pico = cdc::PicoSetup::open_named(&port).context("opening CDC port for setup")?;
    let hello = pico.hello().context("CDC HELLO failed")?;
    println!(
        "  -> Pico firmware v{}.{}.{} (proto v{}, board 0x{:02X})",
        hello.fw_major, hello.fw_minor, hello.fw_patch, hello.proto_version, hello.board_type,
    );
    if hello.proto_version != cdc::PROTO_VERSION {
        bail!(
            "Pico speaks CDC protocol v{}, bridge speaks v{}. Update the side that's older.",
            hello.proto_version,
            cdc::PROTO_VERSION,
        );
    }

    // Move fields out of `creds` without destructuring (Drop on the parent
    // would forbid the destructure-move). After the takes, `creds.password`
    // is an empty String and the eventual Drop is a no-op.
    let ssid = std::mem::take(&mut creds.ssid);
    let mut password = std::mem::take(&mut creds.password);
    drop(creds);
    println!("Sending Wi-Fi credentials to Pico...");
    let result = pico.set_wifi(&ssid, &mut password);
    // set_wifi zeroizes on success; double-belt for the error path.
    password.zeroize();
    drop(password);
    result.context("SET_WIFI failed")?;
    println!("  -> stored. Asking Pico to reboot into run mode.");

    pico.reboot_to_run().context("REBOOT_TO_RUN failed")?;
    println!();
    println!(
        "Pico will reboot. Once it has joined the network it will respond \
         to `ptd-bridge test discover`."
    );
    Ok(())
}

struct Credentials {
    ssid: String,
    password: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

fn prompt_credentials() -> Result<Credentials> {
    let theme = ColorfulTheme::default();
    let ssid: String = Input::with_theme(&theme)
        .with_prompt("Wi-Fi SSID (2.4 GHz network)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.is_empty() {
                Err("SSID can't be empty")
            } else if input.len() > 32 {
                Err("SSID can't be longer than 32 bytes")
            } else {
                Ok(())
            }
        })
        .interact_text()?;
    let password: String = Password::with_theme(&theme)
        .with_prompt("Wi-Fi password (hidden)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.len() > 63 {
                Err("password can't be longer than 63 bytes (WPA2 limit)")
            } else {
                Ok(())
            }
        })
        .interact()?;
    Ok(Credentials { ssid, password })
}
