use super::bluetooth::*;
use super::debug_harvest::*;
use super::*;

fn pico(uid: u32, ip: &str, board: u8) -> PicoTarget {
    PicoTarget {
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
        persona: Persona::Xinput,
        ack_flags: 0,
    }
}

#[test]
fn parse_user_slot_is_one_based() {
    assert_eq!(parse_user_slot("1").unwrap(), 0);
    assert_eq!(parse_user_slot("P4").unwrap(), 3);
    assert!(parse_user_slot("0").is_err());
    assert!(parse_user_slot("5").is_err());
}

#[test]
fn match_pico_by_uid_ip_and_board() {
    let picos = vec![
        pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
        pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
    ];
    assert_eq!(
        match_pico_selector("07D37EB6", &picos)
            .unwrap()
            .info
            .unique_id_short,
        0x07D37EB6
    );
    assert_eq!(
        match_pico_selector("192.168.50.4", &picos)
            .unwrap()
            .info
            .unique_id_short,
        0x523861E6
    );
    assert_eq!(
        match_pico_selector("192.168.50.4:4242", &picos)
            .unwrap()
            .info
            .unique_id_short,
        0x523861E6
    );
    assert_eq!(
        match_pico_selector("rp2040", &picos)
            .unwrap()
            .info
            .unique_id_short,
        0x523861E6
    );
}

#[test]
fn parse_ip_selector_accepts_ip_and_socket_addr() {
    assert_eq!(
        parse_ip_selector("192.168.50.4"),
        Some("192.168.50.4".parse().unwrap())
    );
    assert_eq!(
        parse_ip_selector("192.168.50.4:4242"),
        Some("192.168.50.4".parse().unwrap())
    );
    assert_eq!(parse_ip_selector("07D37EB6"), None);
}

#[test]
fn manual_ips_include_pico_and_route_targets() {
    let options = RunOptions {
        picos: vec!["192.168.50.4".to_string(), "07D37EB6".to_string()],
        routes: vec![
            "1=192.168.50.226".to_string(),
            "2:192.168.50.4:4242".to_string(),
        ],
        ..RunOptions::default()
    };
    assert_eq!(
        manual_ips_from_options(&options),
        vec![
            "192.168.50.4".parse::<IpAddr>().unwrap(),
            "192.168.50.226".parse::<IpAddr>().unwrap()
        ]
    );
}

#[test]
fn merge_unique_picos_updates_existing_and_adds_new() {
    let mut picos = vec![pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W)];
    let incoming = vec![
        pico(0x07D37EB6, "192.168.50.227", protocol::BOARD_PICO_2_W),
        pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
    ];

    merge_unique_picos(&mut picos, incoming);

    assert_eq!(picos.len(), 2);
    assert_eq!(picos[0].peer.ip().to_string(), "192.168.50.227");
    assert_eq!(picos[1].info.unique_id_short, 0x523861E6);
}

#[test]
fn recovered_target_count_ignores_already_online_picos() {
    let baseline_ids = HashSet::from([0x523861E6]);
    let picos = vec![
        pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
        pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
    ];

    assert_eq!(recovered_target_count(&picos, &baseline_ids), 1);
}

#[test]
fn parse_route_specs_maps_sources_to_targets() {
    let picos = vec![
        pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W),
        pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040),
    ];
    let specs = vec!["1=07D37EB6".to_string(), "2=192.168.50.4".to_string()];
    let routes = parse_route_specs(&specs, &picos).unwrap();
    assert_eq!(routes[0].source_slot, 0);
    assert_eq!(routes[0].pico.info.unique_id_short, 0x07D37EB6);
    assert_eq!(routes[1].source_slot, 1);
    assert_eq!(routes[1].pico.info.unique_id_short, 0x523861E6);
}

#[test]
fn validate_routes_rejects_same_pico_twice() {
    let target = pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W);
    let routes = vec![
        StreamRoute {
            source_slot: 0,
            pico: target.clone(),
        },
        StreamRoute {
            source_slot: 1,
            pico: target,
        },
    ];
    assert!(validate_routes(&routes).is_err());
}

#[test]
fn bluetooth_source_slot_decision_keeps_live_selected_source() {
    let connected = vec![xinput_slot(1), xinput_slot(2)];

    assert_eq!(
        bluetooth_source_slot_decision(1, &connected, true),
        BluetoothSourceSlotDecision::Ready
    );
}

#[test]
fn bluetooth_source_slot_decision_auto_switches_single_dead_saved_source() {
    let connected = vec![xinput_slot(0)];

    assert_eq!(
        bluetooth_source_slot_decision(1, &connected, true),
        BluetoothSourceSlotDecision::AutoSwitch { from: 1, to: 0 }
    );
}

#[test]
fn bluetooth_source_slot_decision_refuses_ambiguous_or_absent_source() {
    assert_eq!(
        bluetooth_source_slot_decision(1, &[xinput_slot(0), xinput_slot(2)], true),
        BluetoothSourceSlotDecision::Missing
    );
    assert_eq!(
        bluetooth_source_slot_decision(1, &[xinput_slot(0)], false),
        BluetoothSourceSlotDecision::Missing
    );
    assert_eq!(
        bluetooth_source_slot_decision(1, &[], true),
        BluetoothSourceSlotDecision::Missing
    );
}

#[test]
fn bluetooth_source_auto_switch_only_allows_single_bluetooth_route() {
    let mut bluetooth = pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W);
    bluetooth.persona = Persona::BluetoothHid;
    let xinput = pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040);

    assert!(should_auto_switch_bluetooth_source(&[StreamRoute {
        source_slot: 1,
        pico: bluetooth.clone(),
    }]));
    assert!(!should_auto_switch_bluetooth_source(&[
        StreamRoute {
            source_slot: 1,
            pico: bluetooth,
        },
        StreamRoute {
            source_slot: 0,
            pico: xinput,
        },
    ]));
}

#[test]
fn bluetooth_source_preflight_error_lists_live_slots() {
    let message = bluetooth_source_preflight_error(&[MissingBluetoothSource {
        pico_uid: "28249370".to_string(),
        selected_slot: 1,
        live_slots: vec![0, 2],
    }]);

    assert!(message.contains("Controller 2 for Pico 28249370 is not live"));
    assert!(message.contains("Controller 1, Controller 3"));
    assert!(message.contains("couchlink run --route N=UID"));
}

#[test]
fn debug_packet_harvest_targets_only_include_enabled_debug_routes() {
    let mut debug = pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W);
    debug.persona = Persona::Debug;
    let xinput = pico(0x523861E6, "192.168.50.4", protocol::BOARD_PICO_W_RP2040);
    let routes = vec![
        RouteRuntime::new(
            StreamRoute {
                source_slot: 0,
                pico: debug,
            },
            None,
            false,
        ),
        RouteRuntime::new(
            StreamRoute {
                source_slot: 1,
                pico: xinput,
            },
            None,
            false,
        ),
    ];

    let disabled = HashSet::new();
    assert!(has_debug_packet_routes(&routes, &disabled));
    assert_eq!(
        debug_packet_harvest_targets(&routes, &disabled)
            .iter()
            .map(|p| p.info.unique_id_short)
            .collect::<Vec<_>>(),
        vec![0x07D37EB6]
    );

    let disabled = HashSet::from([0x07D37EB6]);
    assert!(!has_debug_packet_routes(&routes, &disabled));
    assert!(debug_packet_harvest_targets(&routes, &disabled).is_empty());
}

fn xinput_slot(slot: u32) -> xinput::SlotSnapshot {
    xinput::SlotSnapshot {
        slot,
        state: protocol::GamepadState {
            buttons: slot as u16,
            left_trigger: slot as u8,
            right_trigger: 0,
            left_x: 0,
            left_y: 0,
            right_x: 0,
            right_y: 0,
        },
        packet_number: 100 + slot,
    }
}

#[test]
fn bluetooth_cdc_frame_preserves_controller_payload() {
    let state = protocol::GamepadState {
        buttons: 0x1234,
        left_trigger: 5,
        right_trigger: 6,
        left_x: -123,
        left_y: 456,
        right_x: -789,
        right_y: 1024,
    };
    let packet = Packet::state(7, protocol::FLAG_PARSEC_CONNECTED, state);
    let (command, payload) = bluetooth_cdc_frame_from_packet(&packet).unwrap();

    assert_eq!(command, cdc::CMD_BT_STATE);
    assert_eq!(payload[0], protocol::FLAG_PARSEC_CONNECTED);
    assert_eq!(&payload[1..], &packet.encode()[4..16]);

    let heartbeat = Packet::heartbeat(8, 0, state);
    let (command, payload) = bluetooth_cdc_frame_from_packet(&heartbeat).unwrap();
    assert_eq!(command, cdc::CMD_BT_HEARTBEAT);
    assert_eq!(payload[0], 0);
    assert_eq!(&payload[1..], &heartbeat.encode()[4..16]);
}

#[test]
fn bluetooth_routes_do_not_schedule_udp_recovery() {
    let mut bt = pico(0x07D37EB6, "192.168.50.226", protocol::BOARD_PICO_2_W);
    bt.persona = Persona::BluetoothHid;
    let mut routes = vec![RouteRuntime::new(
        StreamRoute {
            source_slot: 0,
            pico: bt,
        },
        None,
        true,
    )];
    routes[0].last_inbound = Instant::now() - (PEER_STALE_AFTER + Duration::from_secs(1));

    assert!(!schedule_recovery_if_needed(&mut routes));
    assert!(routes[0].last_recovery_attempt.is_none());
}

#[test]
fn bluetooth_status_formatter_explains_pairing_and_connection() {
    assert!(format_bluetooth_peer_state(None, None, false, None).contains("pending"));
    assert!(format_bluetooth_peer_state(None, None, true, None).contains("update Pico firmware"));
    assert!(format_bluetooth_peer_state(None, None, false, Some("timeout")).contains("timeout"));

    let mut status = bt_status(0, 0, 0);
    assert!(
        format_bluetooth_peer_state(Some(&status), None, false, None).contains("radio starting")
    );

    status.flags = cdc::BT_STATUS_FLAG_STARTED;
    status.local_name = "CouchLink BT HID".to_string();
    let waiting = format_bluetooth_peer_state(Some(&status), None, false, None);
    assert!(waiting.contains("discoverable as \"CouchLink BT HID\""));
    assert!(waiting.contains("PIN 0000"));
    assert!(should_print_bluetooth_pairing_hint(Some(&status)));

    status.status_version = cdc::BT_STATUS_VERSION + 1;
    status.decoded_status_version = cdc::BT_STATUS_VERSION;
    let newer = format_bluetooth_peer_state(Some(&status), None, false, None);
    assert!(newer.contains("newer BT_STATUS"));
    assert!(newer.contains("update CouchLink"));
    status.status_version = cdc::BT_STATUS_VERSION;
    status.decoded_status_version = cdc::BT_STATUS_VERSION;

    status.user_confirmation_request_count = 2;
    status.user_confirmation_response_count = 2;
    status.local_name = "Xbox Wireless Controller".to_string();
    let pairing_contact = format_bluetooth_peer_state(Some(&status), None, false, None);
    assert!(pairing_contact.contains("pairing/security seen"));
    assert!(pairing_contact.contains("no Classic HID channel opened"));
    assert!(pairing_contact.contains("blueretro-playstation first"));
    status.user_confirmation_request_count = 0;
    status.user_confirmation_response_count = 0;

    status.link_key_notification_count = 1;
    status.reconnect_state = 0x03;
    status.reconnect_schedule_count = 1;
    let reconnect_pending = format_bluetooth_peer_state(Some(&status), None, false, None);
    assert!(reconnect_pending.contains("HID reconnect scheduled"));
    assert!(reconnect_pending.contains("attempts 0/6"));

    status.reconnect_state = 0x01;
    status.reconnect_cycle_attempts = 2;
    status.reconnect_attempt_count = 2;
    status.reconnect_failed_count = 1;
    status.reconnect_blocked_count = 0;
    status.last_reconnect_status = 0x04;
    status.connection_complete_count = 2;
    status.last_connection_complete_status = 0x04;
    let reconnect_attempted = format_bluetooth_peer_state(Some(&status), None, false, None);
    assert!(reconnect_attempted.contains("HID reconnect attempts 2 failed 1"));
    assert!(reconnect_attempted.contains("ACL completes 2 last status 0x04"));
    status.link_key_notification_count = 0;
    status.reconnect_state = 0;
    status.reconnect_cycle_attempts = 0;
    status.reconnect_schedule_count = 0;
    status.reconnect_attempt_count = 0;
    status.reconnect_failed_count = 0;
    status.last_reconnect_status = 0;
    status.connection_complete_count = 0;
    status.last_connection_complete_status = 0;

    status.flags = cdc::BT_STATUS_FLAG_STARTED | cdc::BT_STATUS_FLAG_CONNECTED;
    status.report_send_count = 12;
    status.get_report_count = 2;
    status.get_report_success_count = 1;
    status.get_report_unsupported_count = 1;
    status.last_get_report_type = 3;
    status.last_get_report_id = 0x02;
    status.last_get_report_len = 36;
    status.set_report_count = 1;
    status.set_report_accepted_count = 1;
    status.last_set_report_type = 2;
    status.last_set_report_id = 0x11;
    status.last_set_report_len = 77;
    let connected = format_bluetooth_peer_state(Some(&status), Some(3), false, None);
    assert!(connected.contains("receiver connected"));
    assert!(connected.contains("HID report len 10"));
    assert!(connected.contains("reports +3 total 12"));
    assert!(connected.contains("GET_REPORT ok 1/2 rejected 1"));
    assert!(connected.contains("last GET feature 0x02 len 36"));
    assert!(connected.contains("SET_REPORT accepted 1/1"));
    assert!(connected.contains("last SET output 0x11 len 77"));
    assert!(connected.contains("no PC CDC input frames yet"));
    assert!(!should_print_bluetooth_pairing_hint(Some(&status)));

    status.bt_cdc_state_count = 4;
    status.bt_cdc_heartbeat_count = 9;
    status.bt_cdc_last_frame_ms = 200;
    status.bt_cdc_last_command = cdc::CMD_BT_HEARTBEAT;
    status.bt_cdc_last_seq = 31;
    status.bt_cdc_last_flags = protocol::FLAG_PARSEC_CONNECTED;
    let connected_with_input = format_bluetooth_peer_state(Some(&status), Some(1), false, None);
    assert!(connected_with_input.contains("PC CDC input state 4 heartbeat 9"));
    assert!(connected_with_input.contains("last cmd 0x0D seq 31 flags 0x01"));
}

#[test]
fn setup_usb_recovery_skips_run_mode_cdc() {
    let hello = cdc::HelloAck {
        proto_version: cdc::PROTO_VERSION,
        fw_major: 26,
        fw_minor: 6,
        fw_patch: 20,
        board_type: protocol::BOARD_PICO_2_W,
        flags: cdc::HELLO_FLAG_CREDS_PRESENT | cdc::HELLO_FLAG_RUN_MODE_ACTIVE,
        firmware_version: crate::firmware_version::FirmwareVersion::Legacy {
            major: 0,
            minor: 0,
            patch: 0,
        },
    };

    assert_eq!(
        classify_setup_usb_hello(&hello),
        SetupUsbMode::RunModeActive
    );

    let mut setup = hello;
    setup.flags = cdc::HELLO_FLAG_CREDS_PRESENT;
    assert_eq!(
        classify_setup_usb_hello(&setup),
        SetupUsbMode::SetupModeWithCredentials
    );

    setup.flags = 0;
    assert_eq!(
        classify_setup_usb_hello(&setup),
        SetupUsbMode::SetupModeWithoutCredentials
    );
}

fn bt_status(flags: u8, report_send_count: u32, close_count: u32) -> cdc::BtStatus {
    cdc::BtStatus {
        status_version: cdc::BT_STATUS_VERSION,
        decoded_status_version: cdc::BT_STATUS_VERSION,
        flags,
        target: 0,
        last_status: 0,
        report_len: 10,
        cid: 0,
        init_count: 1,
        ready_count: 1,
        open_count: if flags & cdc::BT_STATUS_FLAG_CONNECTED != 0 {
            1
        } else {
            0
        },
        close_count,
        can_send_count: report_send_count,
        report_build_count: report_send_count,
        report_send_count,
        send_request_count: report_send_count,
        last_event_ms: 100,
        last_send_ms: if report_send_count > 0 { 120 } else { 0 },
        get_report_count: 0,
        get_report_success_count: 0,
        get_report_unsupported_count: 0,
        set_report_count: 0,
        set_report_accepted_count: 0,
        set_report_unsupported_count: 0,
        out_report_count: 0,
        out_report_accepted_count: 0,
        out_report_unsupported_count: 0,
        last_get_report_id: 0,
        last_get_report_type: 0,
        last_set_report_id: 0,
        last_set_report_type: 0,
        last_out_report_id: 0,
        last_out_report_type: 0,
        last_get_report_len: 0,
        last_set_report_len: 0,
        last_out_report_len: 0,
        pin_code_request_count: 0,
        pin_code_response_count: 0,
        user_confirmation_request_count: 0,
        user_confirmation_response_count: 0,
        simple_pairing_complete_count: 0,
        authentication_complete_count: 0,
        link_key_notification_count: 0,
        encryption_change_count: 0,
        disconnection_complete_count: 0,
        hid_open_failed_count: 0,
        last_security_event_ms: 0,
        last_simple_pairing_status: 0,
        last_authentication_status: 0,
        last_encryption_status: 0,
        last_encryption_enabled: 0,
        last_disconnection_reason: 0,
        last_hid_open_status: 0,
        reconnect_state: 0,
        reconnect_cycle_attempts: 0,
        last_reconnect_status: 0,
        last_reconnect_reason: 0,
        reconnect_schedule_count: 0,
        reconnect_attempt_count: 0,
        reconnect_success_count: 0,
        reconnect_failed_count: 0,
        reconnect_blocked_count: 0,
        last_reconnect_ms: 0,
        connection_complete_count: 0,
        last_connection_complete_status: 0,
        last_connection_complete_link_type: 0,
        last_connection_complete_ms: 0,
        incoming_l2cap_connection_count: 0,
        incoming_l2cap_hid_control_count: 0,
        incoming_l2cap_hid_interrupt_count: 0,
        last_incoming_l2cap_psm: 0,
        last_incoming_l2cap_local_cid: 0,
        last_incoming_l2cap_ms: 0,
        bt_cdc_state_count: 0,
        bt_cdc_heartbeat_count: 0,
        bt_cdc_bad_length_count: 0,
        bt_cdc_rejected_count: 0,
        bt_cdc_last_frame_ms: 0,
        bt_cdc_last_state_ms: 0,
        bt_cdc_last_heartbeat_ms: 0,
        bt_cdc_last_seq: 0,
        bt_cdc_last_command: 0,
        bt_cdc_last_flags: 0,
        local_name: String::new(),
    }
}
