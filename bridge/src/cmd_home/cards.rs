use super::inventory::{BootselPico, PicoInventory, SetupUsbPico};
use super::{PicoAction, identity_from_usb_pico, setup_board_label, yes_no};
use crate::{cmd_run, config};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub(super) struct PicoCard {
    pub(super) title: String,
    pub(super) status: String,
    pub(super) details: Vec<String>,
    pub(super) actions: Vec<PicoAction>,
}

pub(super) fn build_pico_cards(cfg: &config::Config, inventory: &PicoInventory) -> Vec<PicoCard> {
    let saved_ids: HashSet<u32> = cfg.picos.iter().map(|p| p.unique_id_short).collect();
    let mut cards = Vec::new();

    for saved in &cfg.picos {
        let nickname = saved.nickname.as_deref();
        if let Some(wifi) = inventory
            .wifi
            .iter()
            .find(|p| p.info.unique_id_short == saved.unique_id_short)
        {
            cards.push(wifi_card(wifi, true, nickname));
            continue;
        }
        if let Some(usb) = inventory
            .usb
            .iter()
            .find(|p| p.unique_id_short == Some(saved.unique_id_short))
        {
            cards.push(setup_usb_card(usb, true, nickname));
            continue;
        }
        cards.push(missing_saved_card(saved));
    }

    for pico in &inventory.wifi {
        if !saved_ids.contains(&pico.info.unique_id_short) {
            cards.push(wifi_card(pico, false, None));
        }
    }
    for pico in &inventory.usb {
        if pico
            .unique_id_short
            .is_some_and(|uid| saved_ids.contains(&uid))
        {
            continue;
        }
        cards.push(setup_usb_card(pico, false, None));
    }
    for pico in &inventory.bootsel {
        cards.push(bootsel_card(pico.clone()));
    }

    cards
}

pub(super) fn wifi_card(
    pico: &cmd_run::PicoTarget,
    saved: bool,
    nickname: Option<&str>,
) -> PicoCard {
    let nickname_suggestion = Some(pico.persona.console_label().to_string());
    let mut actions = vec![
        PicoAction::StartStreaming {
            target: pico.clone(),
        },
        PicoAction::ChooseRouting {
            target: pico.clone(),
        },
        PicoAction::CheckUsbAdapter {
            target: pico.clone(),
        },
        PicoAction::BlinkLed {
            target: pico.clone(),
        },
        PicoAction::SwitchToUsbDebug {
            target: pico.clone(),
        },
    ];
    if saved {
        actions.push(PicoAction::Rename {
            identity: cmd_run::identity_from_target(pico),
            nickname_suggestion,
        });
    } else {
        actions.push(PicoAction::SaveIdentity {
            identity: cmd_run::identity_from_target(pico),
            nickname_suggestion,
        });
    }

    PicoCard {
        title: titled(
            nickname,
            format!(
                "{} {} ({})",
                pico.board_label(),
                pico.uid_hex(),
                pico.persona.console_label()
            ),
        ),
        status: format!(
            "Wi-Fi ready at {} (fw v{})",
            pico.peer.ip(),
            pico.info.firmware_version()
        ),
        details: vec![
            if saved {
                "Saved Pico"
            } else {
                "Connected but not saved"
            }
            .to_string(),
        ],
        actions,
    }
}

pub(super) fn titled(nickname: Option<&str>, base: String) -> String {
    match nickname {
        Some(name) => format!("{name} - {base}"),
        None => base,
    }
}

pub(super) fn setup_usb_card(pico: &SetupUsbPico, saved: bool, nickname: Option<&str>) -> PicoCard {
    let uid = pico
        .unique_id_short
        .map(|uid| format!(" {uid:08X}"))
        .unwrap_or_default();
    let mut actions = Vec::new();
    if pico.creds_present {
        actions.push(PicoAction::RecoverToWifi {
            port: pico.port.clone(),
        });
    }
    actions.push(PicoAction::ConfigureWifi {
        port: pico.port.clone(),
    });
    actions.push(PicoAction::UpdateFirmwareFromSetupUsb {
        port: pico.port.clone(),
    });
    actions.push(PicoAction::ReadUsbLog {
        port: pico.port.clone(),
    });
    if let Some(identity) = identity_from_usb_pico(pico) {
        if saved {
            actions.push(PicoAction::Rename {
                identity,
                nickname_suggestion: None,
            });
        } else {
            actions.push(PicoAction::SaveIdentity {
                identity,
                nickname_suggestion: None,
            });
        }
    }

    PicoCard {
        title: titled(
            nickname,
            format!("{}{}", setup_board_label(pico.board_type), uid),
        ),
        status: format!("USB debug on {} (fw v{})", pico.port, pico.firmware),
        details: vec![
            format!("saved Wi-Fi: {}", yes_no(pico.creds_present)),
            format!(
                "SELF_TEST {} {}",
                if pico.self_test_passed {
                    "PASS"
                } else {
                    "FAIL"
                },
                pico.self_test_message
            ),
            if saved {
                "Saved Pico".to_string()
            } else {
                "Connected but not saved".to_string()
            },
        ],
        actions,
    }
}

pub(super) fn bootsel_card(pico: BootselPico) -> PicoCard {
    PicoCard {
        title: pico.board.label().to_string(),
        status: format!("BOOTSEL firmware mode at {}", pico.mount.display()),
        details: vec!["Identity is not available in BOOTSEL mode.".to_string()],
        actions: vec![PicoAction::FlashBootsel {
            mount: pico.mount,
            board: pico.board,
        }],
    }
}

pub(super) fn missing_saved_card(pico: &config::PicoIdentity) -> PicoCard {
    let mut actions = Vec::new();
    if let Some(ip) = &pico.last_ip {
        actions.push(PicoAction::FindLastIp {
            identity: pico.clone(),
            ip: ip.clone(),
        });
    }
    actions.push(PicoAction::Rename {
        identity: pico.clone(),
        nickname_suggestion: None,
    });
    actions.push(PicoAction::RemoveSaved {
        identity: pico.clone(),
    });
    let last_ip = pico.last_ip.as_ref().map_or_else(
        || "no last IP saved".to_string(),
        |ip| format!("last IP {ip}"),
    );
    PicoCard {
        title: pico.display_title(),
        status: format!(
            "Not seen right now ({last_ip}, fw v{})",
            pico.firmware_version()
        ),
        details: vec!["Saved Pico".to_string()],
        actions,
    }
}
