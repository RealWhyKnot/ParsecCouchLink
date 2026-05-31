//! Small async wrappers around the `dialoguer` prompts the guided menus use.
//!
//! Each prompt blocks, so it runs on a `spawn_blocking` task to keep the Tokio
//! runtime free, and they all share one `ColorfulTheme` so every menu in the
//! app looks the same. The home, setup, and debug flows call these instead of
//! each keeping their own copy.

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};

/// Free-text line input. Empty input is allowed (callers treat it as "cancel").
pub async fn input_text(prompt: &str) -> Result<String> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()
    })
    .await?
    .context("reading input")
}

/// Single-choice menu. Returns the index of the chosen item.
pub async fn select(prompt: &str, items: &[impl ToString], default: usize) -> Result<usize> {
    let prompt = prompt.to_string();
    let items: Vec<String> = items.iter().map(ToString::to_string).collect();
    tokio::task::spawn_blocking(move || {
        Select::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(&items)
            .default(default.min(items.len().saturating_sub(1)))
            .interact()
    })
    .await?
    .context("reading menu selection")
}

/// Multi-choice menu. Returns the indices of the checked items.
pub async fn multiselect(prompt: &str, items: &[String], defaults: &[bool]) -> Result<Vec<usize>> {
    let prompt = prompt.to_string();
    let items = items.to_vec();
    let defaults = defaults.to_vec();
    tokio::task::spawn_blocking(move || {
        MultiSelect::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(&items)
            .defaults(&defaults)
            .interact()
    })
    .await?
    .context("reading menu selection")
}

/// Yes/no confirmation with a default.
pub async fn confirm(prompt: &str, default: bool) -> Result<bool> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(default)
            .interact()
    })
    .await?
    .context("reading confirmation")
}

/// Wait for the operator to press Enter. Used between screens.
pub async fn press_enter(prompt: &str) -> Result<()> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::{BufRead, Write};
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        write!(stdout, "{} ", prompt)?;
        stdout.flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        Ok(())
    })
    .await?
    .context("waiting for Enter")?;
    Ok(())
}
