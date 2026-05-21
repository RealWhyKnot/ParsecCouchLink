//! `couchlink save-wifi` -- host-local CLI for storing Wi-Fi credentials
//! into the DPAPI-encrypted vault that lab-mode reads when a remote
//! operator triggers `wifi_apply_saved`. Not exposed over the tunnel:
//! only the host's own console can write the vault, only the host's
//! own Windows login can decrypt it.

use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Input, Password};
use zeroize::Zeroize;

use crate::wifi_vault;

pub async fn run(ssid: Option<String>, password: Option<String>, clear: bool) -> Result<()> {
    if clear {
        if !wifi_vault::exists() {
            println!("No saved Wi-Fi credentials to clear.");
            return Ok(());
        }
        wifi_vault::clear().context("clearing wifi vault")?;
        println!("Cleared saved Wi-Fi credentials.");
        return Ok(());
    }

    // Resolve SSID: argument > interactive prompt.
    let ssid = match ssid {
        Some(s) if s.is_empty() => bail!("--ssid is empty"),
        Some(s) => s,
        None => prompt_ssid()?,
    };
    if ssid.len() > wifi_vault::SSID_MAX {
        bail!(
            "SSID is {} bytes, must be <= {}",
            ssid.len(),
            wifi_vault::SSID_MAX
        );
    }

    // Resolve password: argument > interactive hidden prompt. The
    // owned `String` here is the only place the cleartext lives between
    // user input and the DPAPI call; we zeroize on every exit path.
    let mut password = match password {
        Some(p) => p,
        None => prompt_password()?,
    };
    if password.len() > wifi_vault::PASSWORD_MAX {
        password.zeroize();
        bail!(
            "Password is {} bytes, must be <= {}",
            password.len(),
            wifi_vault::PASSWORD_MAX
        );
    }

    let save_result = wifi_vault::save(&ssid, &password);
    password.zeroize();
    save_result.context("saving wifi vault")?;

    let path = wifi_vault::vault_path()?;
    println!("Saved Wi-Fi credentials for SSID '{ssid}'.");
    println!("Encrypted vault: {}", path.display());
    println!(
        "Only this Windows login can read it. Lab-mode's `wifi_apply_saved` \
         command will decrypt in memory and push to the Pico without the \
         password ever leaving this machine."
    );
    Ok(())
}

fn prompt_ssid() -> Result<String> {
    let theme = ColorfulTheme::default();
    let ssid: String = Input::with_theme(&theme)
        .with_prompt("Wi-Fi SSID (2.4 GHz network)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.is_empty() {
                Err("SSID can't be empty")
            } else if input.len() > wifi_vault::SSID_MAX {
                Err("SSID is longer than 32 bytes")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .context("reading SSID")?;
    Ok(ssid)
}

fn prompt_password() -> Result<String> {
    let theme = ColorfulTheme::default();
    let password: String = Password::with_theme(&theme)
        .with_prompt("Wi-Fi password (hidden)")
        .with_confirmation("Confirm password", "Mismatch -- try again")
        .interact()
        .context("reading password")?;
    Ok(password)
}
