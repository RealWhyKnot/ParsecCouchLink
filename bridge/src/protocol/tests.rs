use super::*;

#[test]
fn crc8_smbus_check_value() {
    // Canonical CRC-8/SMBUS check over ASCII "123456789" is 0xF4.
    assert_eq!(crc8(b"123456789"), 0xF4);
}

#[test]
fn empty_input_crc_is_zero() {
    assert_eq!(crc8(&[]), 0x00);
}

#[test]
fn state_roundtrip() {
    let pkt = Packet::state(
        42,
        FLAG_PARSEC_CONNECTED,
        GamepadState {
            buttons: 0xABCD,
            left_trigger: 0x12,
            right_trigger: 0xFE,
            left_x: -32000,
            left_y: 32000,
            right_x: 0,
            right_y: -1,
        },
    );
    let buf = pkt.encode();
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_STATE);
    assert_eq!(buf[2], 42);
    assert_eq!(buf[3], FLAG_PARSEC_CONNECTED);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn heartbeat_roundtrip() {
    let st = GamepadState {
        buttons: 0x1234,
        ..Default::default()
    };
    let pkt = Packet::heartbeat(7, 0, st);
    let buf = pkt.encode();
    assert_eq!(buf[1], TYPE_HEARTBEAT);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn discover_roundtrip() {
    let pkt = Packet::discover(0);
    let buf = pkt.encode();
    assert_eq!(buf[1], TYPE_DISCOVER);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn key_state_roundtrip() {
    let rep = KeyboardReport {
        modifiers: 0b0000_0010, // left shift
        keys: [0x04, 0x05, 0x28, 0x00, 0x00, 0x00],
    };
    let pkt = Packet::key_state(11, FLAG_PARSEC_CONNECTED, rep);
    let buf = pkt.encode();
    assert_eq!(buf[1], TYPE_KEY_STATE);
    // modifiers in body[0], reserved zero in body[1], keys in body[2..8]
    assert_eq!(buf[4], 0b0000_0010);
    assert_eq!(buf[5], 0);
    assert_eq!(&buf[6..12], &[0x04, 0x05, 0x28, 0x00, 0x00, 0x00]);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn key_heartbeat_roundtrip() {
    let rep = KeyboardReport::default();
    let pkt = Packet::key_heartbeat(3, 0, rep);
    let buf = pkt.encode();
    assert_eq!(buf[1], TYPE_KEY_HEARTBEAT);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn key_state_full_six_keys_and_all_modifiers() {
    let rep = KeyboardReport {
        modifiers: 0xFF, // every modifier held
        keys: [0x04, 0x16, 0x07, 0x09, 0x0A, 0x28],
    };
    let pkt = Packet::key_state(0, 0, rep);
    let back = Packet::decode(&pkt.encode()).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn key_decode_ignores_reserved_and_trailing_body_bytes() {
    // A peer (or a future firmware) may set the HID reserved byte or the
    // unused trailing body bytes; decode must ignore them and still
    // recover the same report, and the CRC must stay valid.
    let rep = KeyboardReport {
        modifiers: 0x01,
        keys: [0x04, 0, 0, 0, 0, 0],
    };
    let mut buf = Packet::key_state(1, 0, rep).encode();
    buf[5] = 0xAB; // HID reserved byte (body[1])
    buf[12] = 0xCD; // first unused trailing body byte (body[8])
    buf[15] = 0xEF; // last unused trailing body byte (body[11])
    buf[16] = crc8(&buf[..16]); // re-CRC after stomping the body
    let back = Packet::decode(&buf).unwrap();
    match back.kind {
        PacketKind::KeyState(got) => assert_eq!(got, rep),
        other => panic!("expected KeyState, got {other:?}"),
    }
}

#[test]
fn set_persona_encode_shape() {
    let buf = encode_set_persona(5, Persona::Keyboard);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_SET_PERSONA);
    assert_eq!(buf[2], 5);
    assert_eq!(buf[3], 0);
    assert_eq!(buf[4], 1); // keyboard flash byte
    for b in &buf[5..16] {
        assert_eq!(*b, 0);
    }
    assert_eq!(buf[16], crc8(&buf[..16]));
    assert_eq!(encode_set_persona(0, Persona::Xinput)[4], 0);
    assert_eq!(encode_set_persona(0, Persona::Debug)[4], 6);
    assert_eq!(encode_set_persona(0, Persona::GenericHid)[4], 7);
    assert_eq!(encode_set_persona(0, Persona::BluetoothHid)[4], 8);
    assert_eq!(encode_set_persona(0, Persona::BluetoothXbox)[4], 9);
    assert_eq!(encode_set_persona(0, Persona::BluetoothPlaystation)[4], 10);
    // Decode is lenient on unknown types; SET_PERSONA isn't a PacketKind.
    assert_eq!(
        Packet::decode(&buf),
        Err(DecodeError::UnknownType(TYPE_SET_PERSONA))
    );
}

#[test]
fn set_usb_capture_encode_shape() {
    let buf = encode_set_usb_capture(7, Persona::Ps4, true);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_SET_USB_CAPTURE);
    assert_eq!(buf[2], 7);
    assert_eq!(buf[3], 0);
    assert_eq!(buf[4], 4); // PS4 flash byte
    assert_eq!(buf[5], 1); // enable capture
    for b in &buf[6..16] {
        assert_eq!(*b, 0);
    }
    assert_eq!(buf[16], crc8(&buf[..16]));

    let clear = encode_set_usb_capture(8, Persona::Xinput, false);
    assert_eq!(clear[1], TYPE_SET_USB_CAPTURE);
    assert_eq!(clear[4], 0);
    assert_eq!(clear[5], 0);
    assert_eq!(clear[16], crc8(&clear[..16]));
    assert_eq!(
        Packet::decode(&buf),
        Err(DecodeError::UnknownType(TYPE_SET_USB_CAPTURE))
    );
}

#[test]
fn persona_from_ack_flags() {
    assert_eq!(Persona::from_ack_flags(0), Persona::Xinput);
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_LOG_CHUNK_SUPPORTED),
        Persona::Xinput
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_KEYBOARD_PERSONA | ACK_FLAG_ALT_PERSONA),
        Persona::Debug
    );
    assert_eq!(
        Persona::from_ack_flags(
            ACK_FLAG_KEYBOARD_PERSONA | ACK_FLAG_ALT_PERSONA | ACK_FLAG_USB_DIAG_SUPPORTED
        ),
        Persona::Debug
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_KEYBOARD_PERSONA),
        Persona::Keyboard
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_KEYBOARD_PERSONA | ACK_FLAG_USB_DIAG_SUPPORTED),
        Persona::Keyboard
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_MAPLE_PERSONA | ACK_FLAG_USB_DIAG_SUPPORTED),
        Persona::Maple
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_DINPUT_PERSONA | ACK_FLAG_USB_DIAG_SUPPORTED),
        Persona::Ps3
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_DINPUT_PERSONA | ACK_FLAG_ALT_PERSONA),
        Persona::Ps4
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_MAPLE_PERSONA | ACK_FLAG_ALT_PERSONA),
        Persona::XboxOne
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_KEYBOARD_PERSONA | ACK_FLAG_MAPLE_PERSONA),
        Persona::Keyboard
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_DINPUT_PERSONA | ACK_FLAG_MAPLE_PERSONA),
        Persona::GenericHid
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_ALT_PERSONA),
        Persona::BluetoothHid
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_ALT_PERSONA | ACK_FLAG_LOG_CHUNK_SUPPORTED),
        Persona::BluetoothHid
    );
    assert_eq!(
        Persona::from_ack_flags(
            ACK_FLAG_DINPUT_PERSONA | ACK_FLAG_MAPLE_PERSONA | ACK_FLAG_ALT_PERSONA
        ),
        Persona::BluetoothXbox
    );
    assert_eq!(
        Persona::from_ack_flags(
            ACK_FLAG_KEYBOARD_PERSONA
                | ACK_FLAG_DINPUT_PERSONA
                | ACK_FLAG_MAPLE_PERSONA
                | ACK_FLAG_ALT_PERSONA
        ),
        Persona::BluetoothPlaystation
    );
    assert_eq!(
        Persona::from_ack_flags(ACK_FLAG_KEYBOARD_PERSONA | ACK_FLAG_DINPUT_PERSONA),
        Persona::Keyboard
    );
    assert_eq!(Persona::Xinput.flash_byte(), 0);
    assert_eq!(Persona::Keyboard.flash_byte(), 1);
    assert_eq!(Persona::Maple.flash_byte(), 2);
    assert_eq!(Persona::Ps3.flash_byte(), 3);
    assert_eq!(Persona::Ps4.flash_byte(), 4);
    assert_eq!(Persona::XboxOne.flash_byte(), 5);
    assert_eq!(Persona::Debug.flash_byte(), 6);
    assert_eq!(Persona::GenericHid.flash_byte(), 7);
    assert_eq!(Persona::BluetoothHid.flash_byte(), 8);
    assert_eq!(Persona::BluetoothXbox.flash_byte(), 9);
    assert_eq!(Persona::BluetoothPlaystation.flash_byte(), 10);
    assert!(Persona::BluetoothHid.is_bluetooth());
    assert!(Persona::BluetoothXbox.is_bluetooth());
    assert!(Persona::BluetoothPlaystation.is_bluetooth());
    assert!(!Persona::GenericHid.is_bluetooth());
}

#[test]
fn ack_roundtrip() {
    let info = AckInfo {
        proto_version: PROTO_VERSION,
        fw_major: 0,
        fw_minor: 1,
        fw_patch: 2,
        board_type: BOARD_PICO_2_W,
        uptime_seconds: 0x123456, // fits in u24
        unique_id_short: 0xDEADBEEF,
        full_version: None,
    };
    let pkt = Packet::ack(99, info);
    let buf = pkt.encode();
    assert_eq!(buf[1], TYPE_ACK);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(pkt, back);
    if let PacketKind::Ack(got) = back.kind {
        assert_eq!(got, info);
    } else {
        panic!("decoded ack as wrong kind");
    }
}

#[test]
fn version_request_encode_shape() {
    let buf = encode_get_version(7);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_GET_VERSION);
    assert_eq!(buf[2], 7);
    assert_eq!(buf[16], crc8(&buf[..16]));
    assert_eq!(
        Packet::decode(&buf),
        Err(DecodeError::UnknownType(TYPE_GET_VERSION))
    );
}

#[test]
fn version_reply_roundtrip_with_suffix() {
    let info = VersionInfo {
        version: FirmwareVersion::Release {
            year: 2026,
            month: 6,
            day: 15,
            revision: Some(0),
            suffix: Some(*b"0030"),
        },
    };
    let buf = info.encode(12, 0);
    assert_eq!(buf[1], TYPE_VERSION);
    assert_eq!(buf[5], 0x07);
    let (seq, flags, decoded) = VersionInfo::decode_with_header(&buf).unwrap();
    assert_eq!(seq, 12);
    assert_eq!(flags, 0);
    assert_eq!(decoded.version.to_string(), "2026.6.15.0-0030");
    let decoded = VersionInfo::decode(&buf).unwrap();
    assert_eq!(decoded.version.to_string(), "2026.6.15.0-0030");
}

#[test]
fn version_reply_roundtrip_without_suffix() {
    let info = VersionInfo {
        version: FirmwareVersion::Release {
            year: 2026,
            month: 6,
            day: 15,
            revision: Some(1),
            suffix: None,
        },
    };
    let decoded = VersionInfo::decode(&info.encode(0, 0)).unwrap();
    assert_eq!(decoded.version.to_string(), "2026.6.15.1");
}

#[test]
fn ack_prefers_full_version_when_available() {
    let ack = AckInfo {
        proto_version: PROTO_VERSION,
        fw_major: 26,
        fw_minor: 6,
        fw_patch: 15,
        board_type: BOARD_PICO_2_W,
        uptime_seconds: 1,
        unique_id_short: 0xDEADBEEF,
        full_version: Some(FirmwareVersion::Release {
            year: 2026,
            month: 6,
            day: 15,
            revision: Some(0),
            suffix: Some(*b"0030"),
        }),
    };
    assert_eq!(ack.firmware_version().to_string(), "2026.6.15.0-0030");
}

#[test]
fn ack_uptime_u24_clamp() {
    // Anything above 0xFFFFFF won't survive the wire roundtrip.
    let info = AckInfo {
        uptime_seconds: 0x01_AB_CD_EF, // top byte 0x01 will be dropped
        ..Default::default()
    };
    let buf = Packet::ack(0, info).encode();
    let back = Packet::decode(&buf).unwrap();
    if let PacketKind::Ack(got) = back.kind {
        assert_eq!(got.uptime_seconds, 0x00_AB_CD_EF);
    } else {
        unreachable!();
    }
}

#[test]
fn bad_crc_rejected() {
    let mut buf = Packet::discover(0).encode();
    buf[16] ^= 0xFF;
    assert_eq!(Packet::decode(&buf), Err(DecodeError::BadCrc));
}

#[test]
fn wrong_magic_rejected() {
    let mut buf = Packet::discover(0).encode();
    buf[0] = 0x00;
    assert_eq!(Packet::decode(&buf), Err(DecodeError::WrongMagic));
}

#[test]
fn wrong_size_rejected() {
    let buf = [0u8; 16];
    assert_eq!(Packet::decode(&buf), Err(DecodeError::WrongSize));
}

#[test]
fn unknown_type_rejected() {
    let mut buf = Packet::discover(0).encode();
    buf[1] = 0xFE;
    buf[16] = crc8(&buf[..16]);
    assert_eq!(Packet::decode(&buf), Err(DecodeError::UnknownType(0xFE)));
}

#[test]
fn seq_wraparound() {
    assert!(seq_is_newer(1, 0));
    assert!(seq_is_newer(0, 255));
    assert!(!seq_is_newer(0, 1));
    assert!(!seq_is_newer(0, 0));
    // half-window boundary
    assert!(seq_is_newer(127, 0));
    assert!(!seq_is_newer(128, 0));
}

#[test]
fn get_log_encode_shape() {
    let buf = encode_get_log(42);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_GET_LOG);
    assert_eq!(buf[2], 42);
    assert_eq!(buf[3], 0);
    // body[0..12] reserved -> zeros
    for b in &buf[4..16] {
        assert_eq!(*b, 0);
    }
    // CRC-8 is good
    assert_eq!(buf[16], crc8(&buf[..16]));
    // And it still decodes via the existing Packet path so old code
    // paths see a known shape (even though they ignore the new type).
    // Decode is lenient on unknown types: TYPE_GET_LOG is not in
    // PacketKind, so this should error with UnknownType(0x05).
    assert_eq!(
        Packet::decode(&buf),
        Err(DecodeError::UnknownType(TYPE_GET_LOG))
    );
}

#[test]
fn usb_diag_request_encode_shape() {
    let buf = encode_get_usb_diag(7);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_GET_USB_DIAG);
    assert_eq!(buf[2], 7);
    assert_eq!(buf[3], 0);
    for b in &buf[4..16] {
        assert_eq!(*b, 0);
    }
    assert_eq!(buf[16], crc8(&buf[..16]));
}

#[test]
fn pico_state_request_encode_shape() {
    let buf = encode_get_pico_state(11);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_GET_PICO_STATE);
    assert_eq!(buf[2], 11);
    assert_eq!(buf[3], 0);
    for b in &buf[4..16] {
        assert_eq!(*b, 0);
    }
    assert_eq!(buf[16], crc8(&buf[..16]));
    assert_eq!(
        Packet::decode(&buf),
        Err(DecodeError::UnknownType(TYPE_GET_PICO_STATE))
    );
}

#[test]
fn reboot_to_setup_request_encode_shape() {
    let buf = encode_reboot_to_setup(9);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_REBOOT_TO_SETUP);
    assert_eq!(buf[2], 9);
    assert_eq!(buf[3], 0);
    for b in &buf[4..16] {
        assert_eq!(*b, 0);
    }
    assert_eq!(buf[16], crc8(&buf[..16]));
    assert_eq!(
        Packet::decode(&buf),
        Err(DecodeError::UnknownType(TYPE_REBOOT_TO_SETUP))
    );
}

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
    buf[USB_DIAG_V1_WIRE_SIZE - 2] = (crc & 0xFF) as u8;
    buf[USB_DIAG_V1_WIRE_SIZE - 1] = (crc >> 8) as u8;

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
        unique_id_short: 0x02E22DA9,
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
    buf[PICO_STATE_V1_WIRE_SIZE - 2] = (crc & 0xFF) as u8;
    buf[PICO_STATE_V1_WIRE_SIZE - 1] = (crc >> 8) as u8;

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
    buf[PICO_STATE_V2_WIRE_SIZE - 2] = (crc & 0xFF) as u8;
    buf[PICO_STATE_V2_WIRE_SIZE - 1] = (crc >> 8) as u8;

    let back = PicoStateDiag::decode(&buf).unwrap();
    assert_eq!(back.version, PICO_STATE_V2_VERSION);
    assert_eq!(back.xinput_in_blocked_not_mounted_count, 12);
    assert_eq!(back.last_in_blocked_ms, 44);
    assert_eq!(back.last_in_blocked_reason, USB_DIAG_IN_BLOCKED_NOT_READY);
    assert_eq!(back.bt_flags, 0);
    assert_eq!(back.bt_report_send_count, 0);
    assert_eq!(back.bt_target_label(), "bluetooth");
}

fn make_chunk(idx: u8, flags: u8, total: u8, lost: u32, payload: &[u8]) -> LogChunk {
    LogChunk {
        chunk_index: idx,
        flags,
        total_chunks: total,
        lost_bytes: lost,
        payload: payload.to_vec(),
    }
}

#[test]
fn log_chunk_roundtrip_empty_payload() {
    let c = make_chunk(0, LOG_CHUNK_FLAG_LAST, 1, 0, &[]);
    let buf = c.encode();
    assert_eq!(buf.len(), LOG_CHUNK_HEADER_LEN + 2);
    let back = LogChunk::decode(&buf).unwrap();
    assert_eq!(back, c);
    assert!(back.is_last());
}

#[test]
fn log_chunk_roundtrip_max_payload() {
    let payload: Vec<u8> = (0..LOG_CHUNK_MAX_PAYLOAD)
        .map(|i| (i & 0xFF) as u8)
        .collect();
    assert_eq!(payload.len(), LOG_CHUNK_MAX_PAYLOAD);
    let c = make_chunk(5, 0, 16, 1234, &payload);
    let buf = c.encode();
    assert_eq!(buf.len(), LOG_CHUNK_HEADER_LEN + LOG_CHUNK_MAX_PAYLOAD + 2);
    let back = LogChunk::decode(&buf).unwrap();
    assert_eq!(back, c);
    assert!(!back.is_last());
    assert_eq!(back.lost_bytes, 1234);
}

#[test]
fn log_chunk_last_flag_decoded() {
    let c = make_chunk(15, LOG_CHUNK_FLAG_LAST, 16, 0, b"final-chunk");
    let buf = c.encode();
    let back = LogChunk::decode(&buf).unwrap();
    assert!(back.is_last());
}

#[test]
fn log_chunk_bad_crc_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hello");
    let mut buf = c.encode();
    let last = buf.len() - 1;
    buf[last] ^= 0xFF;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::BadCrc { .. }) => (),
        other => panic!("expected BadCrc, got {other:?}"),
    }
}

#[test]
fn log_chunk_wrong_magic_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hi");
    let mut buf = c.encode();
    buf[0] = 0x00;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::WrongMagic) => (),
        other => panic!("expected WrongMagic, got {other:?}"),
    }
}

#[test]
fn log_chunk_wrong_type_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hi");
    let mut buf = c.encode();
    buf[1] = TYPE_ACK;
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::WrongType(t)) if t == TYPE_ACK => (),
        other => panic!("expected WrongType(TYPE_ACK), got {other:?}"),
    }
}

#[test]
fn log_chunk_length_mismatch_rejected() {
    let c = make_chunk(0, 0, 1, 0, b"hi");
    let mut buf = c.encode();
    // Claim a longer payload than the buffer actually contains.
    buf[5] = 99; // payload_len LE lo
    match LogChunk::decode(&buf) {
        Err(LogChunkDecodeError::LengthMismatch { .. }) => (),
        other => panic!("expected LengthMismatch, got {other:?}"),
    }
}

#[test]
fn ack_capability_flag_decoded() {
    // Hand-roll an ACK datagram with the capability bit set in the
    // flags byte so the bridge sees what new firmware will send.
    let info = AckInfo {
        proto_version: PROTO_VERSION,
        fw_major: 1,
        fw_minor: 2,
        fw_patch: 3,
        board_type: BOARD_PICO_W_RP2040,
        uptime_seconds: 60,
        unique_id_short: 0xABCD1234,
        full_version: None,
    };
    let mut buf = Packet::ack(7, info).encode();
    // Stomp the flags byte with the capability bit and re-CRC.
    buf[3] = ACK_FLAG_LOG_CHUNK_SUPPORTED;
    buf[16] = crc8(&buf[..16]);
    let back = Packet::decode(&buf).unwrap();
    assert_eq!(
        back.flags & ACK_FLAG_LOG_CHUNK_SUPPORTED,
        ACK_FLAG_LOG_CHUNK_SUPPORTED
    );
    match back.kind {
        PacketKind::Ack(got) => assert_eq!(got, info),
        other => panic!("expected Ack, got {other:?}"),
    }
}
