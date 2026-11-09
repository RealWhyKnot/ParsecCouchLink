use crate::protocol::*;

#[test]
fn usb_diag_roundtrip() {
    let diag = UsbDiag {
        seq: 9,
        flags: 0,
        version: USB_DIAG_VERSION,
        usb_flags: USB_DIAG_FLAG_MOUNTED,
        activity_flags: USB_DIAG_ACTIVITY_SENT | USB_DIAG_ACTIVITY_OUT,
        last_out_len: 8,
        now_ms: 1000,
        last_bridge_packet_ms: 900,
        mount_count: 1,
        umount_count: 2,
        suspend_count: 3,
        resume_count: 4,
        device_desc_count: 5,
        config_desc_count: 6,
        xinput_in_queued_count: 7,
        xinput_in_sent_count: 8,
        xinput_out_count: 9,
        xinput_in_blocked_not_mounted_count: 10,
        xinput_in_blocked_not_ready_count: 11,
        xinput_in_blocked_short_write_count: 12,
        xinput_in_idle_suppressed_count: 13,
        last_mount_ms: 10,
        last_umount_ms: 11,
        last_in_queued_ms: 12,
        last_in_sent_ms: 13,
        last_out_ms: 14,
        last_in_blocked_ms: 15,
        last_in_blocked_reason: USB_DIAG_IN_BLOCKED_NOT_READY,
        last_in_blocked_want: 20,
        last_in_blocked_got: 4,
        last_out_byte0: 0x01,
        last_out_byte1: 0x08,
    };
    let buf = diag.encode();
    assert_eq!(buf.len(), USB_DIAG_WIRE_SIZE);
    assert_eq!(buf[1], TYPE_USB_DIAG);
    let back = UsbDiag::decode(&buf).unwrap();
    assert_eq!(back, diag);
    assert!(back.mounted());
    assert!(back.xinput_report_sent());
    assert!(back.xinput_out_seen());
    assert_eq!(back.age_ms(990), Some(10));
    assert_eq!(back.in_blocked_total(), 33);
    assert_eq!(
        usb_in_blocked_reason_label(back.last_in_blocked_reason),
        "not_ready"
    );
}

#[test]
fn usb_diag_bad_crc_rejected() {
    let mut buf = UsbDiag {
        seq: 0,
        flags: 0,
        version: USB_DIAG_VERSION,
        usb_flags: 0,
        activity_flags: 0,
        last_out_len: 0,
        now_ms: 0,
        last_bridge_packet_ms: 0,
        mount_count: 0,
        umount_count: 0,
        suspend_count: 0,
        resume_count: 0,
        device_desc_count: 0,
        config_desc_count: 0,
        xinput_in_queued_count: 0,
        xinput_in_sent_count: 0,
        xinput_out_count: 0,
        xinput_in_blocked_not_mounted_count: 0,
        xinput_in_blocked_not_ready_count: 0,
        xinput_in_blocked_short_write_count: 0,
        xinput_in_idle_suppressed_count: 0,
        last_mount_ms: 0,
        last_umount_ms: 0,
        last_in_queued_ms: 0,
        last_in_sent_ms: 0,
        last_out_ms: 0,
        last_in_blocked_ms: 0,
        last_in_blocked_reason: 0,
        last_in_blocked_want: 0,
        last_in_blocked_got: 0,
        last_out_byte0: 0,
        last_out_byte1: 0,
    }
    .encode();
    buf[10] ^= 0xFF;
    match UsbDiag::decode(&buf) {
        Err(UsbDiagDecodeError::BadCrc { .. }) => (),
        other => panic!("expected BadCrc, got {other:?}"),
    }
}

#[test]
fn usb_diag_v1_decode_defaults_new_block_fields() {
    let mut buf = [0u8; USB_DIAG_V1_WIRE_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_USB_DIAG;
    buf[2] = 5;
    buf[4] = USB_DIAG_V1_VERSION;
    buf[5] = USB_DIAG_FLAG_MOUNTED;
    buf[6] = USB_DIAG_ACTIVITY_SENT;
    put_u32_le(&mut buf, 44, 9);
    let crc =
        crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..USB_DIAG_V1_WIRE_SIZE - 2]);
    buf[USB_DIAG_V1_WIRE_SIZE - 2..USB_DIAG_V1_WIRE_SIZE].copy_from_slice(&crc.to_le_bytes());

    let back = UsbDiag::decode(&buf).unwrap();
    assert_eq!(back.version, USB_DIAG_V1_VERSION);
    assert_eq!(back.xinput_in_sent_count, 9);
    assert_eq!(back.in_blocked_total(), 0);
    assert_eq!(back.last_in_blocked_reason, USB_DIAG_IN_BLOCKED_NONE);
}

#[test]
fn pico_state_roundtrip() {
    let diag = PicoStateDiag {
        seq: 3,
        flags: 0,
        version: PICO_STATE_VERSION,
        proto_version: PROTO_VERSION,
        board_type: BOARD_PICO_2_W,
        persona_byte: Persona::Maple.flash_byte(),
        unique_id_short: 0x02E2_2DA9,
        uptime_seconds: 123,
        tx_count: 456,
        rx_count: 789,
        now_ms: 1000,
        last_bridge_packet_ms: 950,
        mount_count: 1,
        umount_count: 2,
        suspend_count: 3,
        resume_count: 4,
        device_desc_count: 5,
        config_desc_count: 6,
        xinput_in_queued_count: 7,
        xinput_in_sent_count: 8,
        xinput_out_count: 9,
        xinput_in_blocked_not_mounted_count: 10,
        xinput_in_blocked_not_ready_count: 11,
        xinput_in_blocked_short_write_count: 12,
        xinput_in_idle_suppressed_count: 13,
        last_mount_ms: 10,
        last_umount_ms: 11,
        last_in_queued_ms: 12,
        last_in_sent_ms: 13,
        last_out_ms: 14,
        last_in_blocked_ms: 15,
        last_in_blocked_reason: USB_DIAG_IN_BLOCKED_SHORT_WRITE,
        last_in_blocked_want: 36,
        last_in_blocked_got: 8,
        last_out_len: 8,
        last_out_byte0: 1,
        last_out_byte1: 2,
        usb_flags: USB_DIAG_FLAG_MOUNTED,
        activity_flags: USB_DIAG_ACTIVITY_SENT | USB_DIAG_ACTIVITY_PEER,
        malformed_udp_count: 42,
        bt_flags: BT_HID_STATUS_STARTED | BT_HID_STATUS_CONNECTED,
        bt_target: 2,
        bt_last_status: 0,
        bt_report_len: 10,
        bt_cid: 0x1234,
        bt_init_count: 1,
        bt_ready_count: 2,
        bt_open_count: 3,
        bt_close_count: 4,
        bt_can_send_count: 5,
        bt_report_build_count: 6,
        bt_report_send_count: 7,
        bt_send_request_count: 8,
        bt_last_event_ms: 999,
        bt_last_send_ms: 1001,
    };

    let buf = diag.encode();
    assert_eq!(buf.len(), PICO_STATE_WIRE_SIZE);
    assert_eq!(buf[1], TYPE_PICO_STATE);
    let back = PicoStateDiag::decode(&buf).unwrap();
    assert_eq!(back, diag);
    assert_eq!(back.persona(), Some(Persona::Maple));
    let mut debug_diag = diag.clone();
    debug_diag.persona_byte = Persona::Debug.flash_byte();
    assert_eq!(debug_diag.persona(), Some(Persona::Debug));
    let mut generic_diag = diag;
    generic_diag.persona_byte = Persona::GenericHid.flash_byte();
    assert_eq!(generic_diag.persona(), Some(Persona::GenericHid));
    let mut bt_diag = generic_diag;
    bt_diag.persona_byte = Persona::BluetoothPlaystation.flash_byte();
    assert_eq!(bt_diag.persona(), Some(Persona::BluetoothPlaystation));
    let json = back.to_json_map();
    assert_eq!(json["malformed_udp_count"], serde_json::json!(42));
    assert_eq!(json["in_blocked_not_ready"], serde_json::json!(11));
    assert_eq!(
        json["last_in_blocked_reason"],
        serde_json::json!("short_write")
    );
    assert_eq!(json["bt_connected"], serde_json::json!(true));
    assert_eq!(
        json["bt_target_label"],
        serde_json::json!("bluetooth-playstation")
    );
}

#[test]
fn pico_state_bad_crc_rejected() {
    let mut buf = PicoStateDiag {
        seq: 0,
        flags: 0,
        version: PICO_STATE_VERSION,
        proto_version: PROTO_VERSION,
        board_type: BOARD_PICO_2_W,
        persona_byte: 0,
        unique_id_short: 0,
        uptime_seconds: 0,
        tx_count: 0,
        rx_count: 0,
        now_ms: 0,
        last_bridge_packet_ms: 0,
        mount_count: 0,
        umount_count: 0,
        suspend_count: 0,
        resume_count: 0,
        device_desc_count: 0,
        config_desc_count: 0,
        xinput_in_queued_count: 0,
        xinput_in_sent_count: 0,
        xinput_out_count: 0,
        xinput_in_blocked_not_mounted_count: 0,
        xinput_in_blocked_not_ready_count: 0,
        xinput_in_blocked_short_write_count: 0,
        xinput_in_idle_suppressed_count: 0,
        last_mount_ms: 0,
        last_umount_ms: 0,
        last_in_queued_ms: 0,
        last_in_sent_ms: 0,
        last_out_ms: 0,
        last_in_blocked_ms: 0,
        last_in_blocked_reason: 0,
        last_in_blocked_want: 0,
        last_in_blocked_got: 0,
        last_out_len: 0,
        last_out_byte0: 0,
        last_out_byte1: 0,
        usb_flags: 0,
        activity_flags: 0,
        malformed_udp_count: 0,
        bt_flags: 0,
        bt_target: 0,
        bt_last_status: 0,
        bt_report_len: 0,
        bt_cid: 0,
        bt_init_count: 0,
        bt_ready_count: 0,
        bt_open_count: 0,
        bt_close_count: 0,
        bt_can_send_count: 0,
        bt_report_build_count: 0,
        bt_report_send_count: 0,
        bt_send_request_count: 0,
        bt_last_event_ms: 0,
        bt_last_send_ms: 0,
    }
    .encode();
    buf[20] ^= 0xFF;
    match PicoStateDiag::decode(&buf) {
        Err(PicoStateDecodeError::BadCrc { .. }) => (),
        other => panic!("expected BadCrc, got {other:?}"),
    }
}

#[test]
fn pico_state_v1_decode_defaults_new_block_fields() {
    let mut buf = [0u8; PICO_STATE_V1_WIRE_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_PICO_STATE;
    buf[2] = 5;
    buf[4] = PICO_STATE_V1_VERSION;
    buf[5] = PROTO_VERSION;
    put_u32_le(&mut buf, 60, 9);
    let crc =
        crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..PICO_STATE_V1_WIRE_SIZE - 2]);
    buf[PICO_STATE_V1_WIRE_SIZE - 2..PICO_STATE_V1_WIRE_SIZE].copy_from_slice(&crc.to_le_bytes());

    let back = PicoStateDiag::decode(&buf).unwrap();
    assert_eq!(back.version, PICO_STATE_V1_VERSION);
    assert_eq!(back.xinput_in_sent_count, 9);
    assert_eq!(back.xinput_in_blocked_not_ready_count, 0);
    assert_eq!(back.last_in_blocked_reason, USB_DIAG_IN_BLOCKED_NONE);
    assert_eq!(back.bt_init_count, 0);
    assert_eq!(back.bt_target_label(), "bluetooth");
}

#[test]
fn pico_state_v2_decode_defaults_bluetooth_fields() {
    let mut buf = [0u8; PICO_STATE_V2_WIRE_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_PICO_STATE;
    buf[2] = 6;
    buf[4] = PICO_STATE_V2_VERSION;
    buf[5] = PROTO_VERSION;
    put_u32_le(&mut buf, 100, 12);
    put_u32_le(&mut buf, 116, 44);
    buf[120] = USB_DIAG_IN_BLOCKED_NOT_READY;
    let crc =
        crc::Crc::<u16>::new(&crc::CRC_16_IBM_3740).checksum(&buf[..PICO_STATE_V2_WIRE_SIZE - 2]);
    buf[PICO_STATE_V2_WIRE_SIZE - 2..PICO_STATE_V2_WIRE_SIZE].copy_from_slice(&crc.to_le_bytes());

    let back = PicoStateDiag::decode(&buf).unwrap();
    assert_eq!(back.version, PICO_STATE_V2_VERSION);
    assert_eq!(back.xinput_in_blocked_not_mounted_count, 12);
    assert_eq!(back.last_in_blocked_ms, 44);
    assert_eq!(back.last_in_blocked_reason, USB_DIAG_IN_BLOCKED_NOT_READY);
    assert_eq!(back.bt_flags, 0);
    assert_eq!(back.bt_report_send_count, 0);
    assert_eq!(back.bt_target_label(), "bluetooth");
}
