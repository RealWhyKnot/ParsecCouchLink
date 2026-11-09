mod actions;
mod cards;
mod inventory;
mod streaming;
mod tools;

use anyhow::Result;

use crate::tui::select;
use crate::{config, protocol};
use actions::{
    PicoAction, add_new_pico, identity_from_usb_pico, refresh_saved_observations, run_pico_action,
    seed_saved_picos_from_legacy_last, setup_board_label, yes_no,
};
use cards::{PicoCard, build_pico_cards};
use inventory::{PicoInventory, scan_pico_inventory};
use streaming::{route_one, stream};
use tools::{menu_item, tools_menu};

const HOME_SCAN_SECONDS: u64 = 3;

pub(crate) async fn run() -> Result<()> {
    loop {
        print_header();
        match basic_tab().await? {
            BasicNav::Stay => {}
            BasicNav::Advanced => tools_menu().await?,
            BasicNav::Quit => return Ok(()),
        }
    }
}

fn print_header() {
    outln!();
    outln!("Parsec CouchLink");
    outln!("Remote Parsec controllers -> Pico output modes");
    outln!();
}

enum BasicNav {
    Stay,
    Advanced,
    Quit,
}

#[derive(Clone, Debug)]
enum BasicSelection {
    PicoAction(PicoAction),
    Refresh,
    AddNew,
    Advanced,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputModeChoice {
    Auto,
    Persona(protocol::Persona),
    Family(&'static [protocol::Persona]),
}

impl InputModeChoice {
    fn bluetooth_persona(self) -> Option<protocol::Persona> {
        match self {
            Self::Persona(persona) if persona.is_bluetooth() => Some(persona),
            _ => None,
        }
    }
}

async fn basic_tab() -> Result<BasicNav> {
    let inventory = scan_pico_inventory().await?;
    let mut cfg = config::load().unwrap_or_default();
    let mut changed = seed_saved_picos_from_legacy_last(&mut cfg);
    if refresh_saved_observations(&mut cfg, &inventory) {
        changed = true;
    }
    if changed {
        config::save(&cfg)?;
    }

    let cards = build_pico_cards(&cfg, &inventory);
    print_basic_home(&cards, &inventory);

    let mut selections = Vec::new();
    let mut choices = Vec::new();
    for card in &cards {
        for action in &card.actions {
            choices.push(menu_item(
                &format!("{}: {}", card.title, action.label()),
                &action.help(),
            ));
            selections.push(BasicSelection::PicoAction(action.clone()));
        }
    }

    choices.push(menu_item("Refresh scan", "look for Pico boards again"));
    selections.push(BasicSelection::Refresh);
    choices.push(menu_item(
        "Add or reinstall a Pico",
        "flash firmware, set Wi-Fi, and save it",
    ));
    selections.push(BasicSelection::AddNew);
    choices.push(menu_item(
        "Advanced tab",
        "one-off diagnostics, fixes, logs, and command reference",
    ));
    selections.push(BasicSelection::Advanced);
    choices.push(menu_item("Quit", "close this menu"));
    selections.push(BasicSelection::Quit);

    let idx = select("Basic action", &choices, 0).await?;
    match selections[idx].clone() {
        BasicSelection::PicoAction(action) => {
            run_pico_action(action).await?;
            Ok(BasicNav::Stay)
        }
        BasicSelection::Refresh => Ok(BasicNav::Stay),
        BasicSelection::AddNew => {
            add_new_pico().await?;
            Ok(BasicNav::Stay)
        }
        BasicSelection::Advanced => Ok(BasicNav::Advanced),
        BasicSelection::Quit => Ok(BasicNav::Quit),
    }
}

fn print_basic_home(cards: &[PicoCard], inventory: &PicoInventory) {
    outln!("Tabs: [Basic] Advanced");
    outln!();
    outln!("Basic");
    outln!("Each Pico has its own commands. Basic actions target one Pico at a time.");
    outln!();

    if cards.is_empty() {
        outln!("No Pico was found on Wi-Fi, setup USB, or BOOTSEL.");
        outln!("Choose `Add or reinstall a Pico` to start from scratch.");
    } else {
        for (idx, card) in cards.iter().enumerate() {
            outln!("{}. {}", idx + 1, card.title);
            outln!("   {}", card.status);
            for detail in &card.details {
                outln!("   {detail}");
            }
            if card.actions.is_empty() {
                outln!("   Commands: none available from Basic for this state.");
            } else {
                outln!("   Commands:");
                for action in &card.actions {
                    outln!("     - {} ({})", action.label(), action.target_hint());
                }
            }
            outln!();
        }
    }

    if !inventory.usb_errors.is_empty() {
        outln!("Setup USB devices needing attention:");
        for (port, error) in &inventory.usb_errors {
            outln!("  {port}: {error}");
        }
        outln!();
    }
    if !inventory.scan_errors.is_empty() {
        outln!("Scan warnings:");
        for error in &inventory.scan_errors {
            outln!("  {error}");
        }
        outln!();
    }
}

#[cfg(test)]
mod tests;
