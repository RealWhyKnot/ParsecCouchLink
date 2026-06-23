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
        ),
        RouteRuntime::new(
            StreamRoute {
                source_slot: 1,
                pico: xinput,
            },
            None,
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
    assert!(!should_print_bluetooth_pairing_hint(Some(&status)));
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
        local_name: String::new(),
    }
}
