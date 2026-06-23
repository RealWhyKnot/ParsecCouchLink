use super::*;
use crate::protocol;

fn pico(uid: u32, ip: &str, board: u8) -> cmd_run::PicoTarget {
    cmd_run::PicoTarget {
        peer: format!("{ip}:4242").parse().unwrap(),
        info: protocol::AckInfo {
            proto_version: protocol::PROTO_VERSION,
            fw_major: 26,
            fw_minor: 5,
            fw_patch: 30,
            board_type: board,
            uptime_seconds: 12,
            unique_id_short: uid,
            full_version: None,
        },
        persona: protocol::Persona::Xinput,
        ack_flags: 0,
    }
}

fn saved_pico(uid: u32, ip: Option<&str>) -> config::PicoIdentity {
    config::PicoIdentity {
        unique_id_short: uid,
        board_type: protocol::BOARD_PICO_2_W,
        fw_major: 26,
        fw_minor: 5,
        fw_patch: 30,
        last_ip: ip.map(|s| s.to_string()),
        device_name: None,
    }
}

fn setup_usb(port: &str, uid: Option<u32>, creds_present: bool) -> SetupUsbPico {
    SetupUsbPico {
        port: port.to_string(),
        unique_id_short: uid,
        board_type: protocol::BOARD_PICO_2_W,
        firmware: "2026.5.30.9-E56A".to_string(),
        fw_major: 26,
        fw_minor: 5,
        fw_patch: 30,
        creds_present,
        self_test_passed: true,
        self_test_message: "pass".to_string(),
    }
}

#[test]
fn basic_cards_prefer_live_wifi_for_saved_pico() {
    let cfg = config::Config {
        picos: vec![saved_pico(0x07D37EB6, Some("192.168.50.1"))],
        ..config::Config::default()
    };
    let inventory = PicoInventory {
        wifi: vec![pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W)],
        ..PicoInventory::default()
    };

    let cards = build_pico_cards(&cfg, &inventory);

    assert_eq!(cards.len(), 1);
    assert!(cards[0].status.contains("Wi-Fi ready at 192.168.50.226"));
    assert!(cards[0].actions.iter().any(|action| matches!(
        action,
        PicoAction::StartStreaming { target }
            if target.info.unique_id_short == 0x07D37EB6
    )));
    assert!(!cards[0]
        .actions
        .iter()
        .any(|action| matches!(action, PicoAction::SaveIdentity { .. })));
}

#[test]
fn basic_cards_expose_setup_usb_and_bootsel_targets() {
    let cfg = config::Config::default();
    let inventory = PicoInventory {
        usb: vec![
            setup_usb("COM4", Some(0x07D37EB6), true),
            setup_usb("COM5", Some(0x523861E6), false),
        ],
        bootsel: vec![BootselPico {
            mount: PathBuf::from("I:\\"),
            board: cmd_flash::BootselBoard::Rp2040,
        }],
        ..PicoInventory::default()
    };

    let cards = build_pico_cards(&cfg, &inventory);

    let com4 = cards
        .iter()
        .find(|card| card.status.contains("COM4"))
        .expect("COM4 card");
    assert!(com4
        .actions
        .iter()
        .any(|action| { matches!(action, PicoAction::RecoverToWifi { port } if port == "COM4") }));
    assert!(com4.actions.iter().any(|action| {
        matches!(action, PicoAction::UpdateFirmwareFromSetupUsb { port } if port == "COM4")
    }));

    let com5 = cards
        .iter()
        .find(|card| card.status.contains("COM5"))
        .expect("COM5 card");
    assert!(!com5
        .actions
        .iter()
        .any(|action| matches!(action, PicoAction::RecoverToWifi { .. })));
    assert!(com5
        .actions
        .iter()
        .any(|action| { matches!(action, PicoAction::ConfigureWifi { port } if port == "COM5") }));

    assert!(cards.iter().any(|card| {
        card.actions.iter().any(|action| {
            matches!(
                action,
                PicoAction::FlashBootsel { mount, board }
                    if mount == &PathBuf::from("I:\\")
                        && *board == cmd_flash::BootselBoard::Rp2040
            )
        })
    }));
}

#[test]
fn basic_cards_keep_multiple_wifi_picos_targeted() {
    let cfg = config::Config::default();
    let inventory = PicoInventory {
        wifi: vec![
            pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
            pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        ],
        ..PicoInventory::default()
    };

    let cards = build_pico_cards(&cfg, &inventory);
    let streaming_targets: Vec<u32> = cards
        .iter()
        .flat_map(|card| &card.actions)
        .filter_map(|action| match action {
            PicoAction::StartStreaming { target } => Some(target.info.unique_id_short),
            _ => None,
        })
        .collect();

    assert_eq!(streaming_targets, vec![0x07D37EB6, 0x523861E6]);
    assert!(cards.iter().all(|card| {
        card.actions.iter().all(|action| {
            !matches!(
                action,
                PicoAction::RecoverToWifi { port } if port.eq_ignore_ascii_case("all")
            )
        })
    }));
}

#[test]
fn missing_saved_pico_actions_are_targeted_to_saved_identity() {
    let cfg = config::Config {
        picos: vec![saved_pico(0x07D37EB6, Some("192.168.50.226"))],
        ..config::Config::default()
    };

    let cards = build_pico_cards(&cfg, &PicoInventory::default());

    assert_eq!(cards.len(), 1);
    assert!(cards[0].actions.iter().any(|action| matches!(
        action,
        PicoAction::FindLastIp { identity, ip }
            if identity.unique_id_short == 0x07D37EB6 && ip == "192.168.50.226"
    )));
    assert!(cards[0].actions.iter().any(|action| matches!(
        action,
        PicoAction::RemoveSaved { identity }
            if identity.unique_id_short == 0x07D37EB6
    )));
}

#[test]
fn seed_saved_picos_migrates_legacy_last_pico_once() {
    let mut cfg = config::Config {
        last_pico: Some(saved_pico(0x07D37EB6, None)),
        ..config::Config::default()
    };

    assert!(seed_saved_picos_from_legacy_last(&mut cfg));
    assert_eq!(cfg.picos.len(), 1);
    assert!(!seed_saved_picos_from_legacy_last(&mut cfg));
    assert_eq!(cfg.picos.len(), 1);
}
