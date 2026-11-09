mod diag;
mod log_chunk;
mod requests;

use super::*;

#[test]
fn crc8_smbus_check_value() {
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
        modifiers: 0b0000_0010,
        keys: [0x04, 0x05, 0x28, 0x00, 0x00, 0x00],
    };
    let pkt = Packet::key_state(11, FLAG_PARSEC_CONNECTED, rep);
    let buf = pkt.encode();
    assert_eq!(buf[1], TYPE_KEY_STATE);
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
        modifiers: 0xFF,
        keys: [0x04, 0x16, 0x07, 0x09, 0x0A, 0x28],
    };
    let pkt = Packet::key_state(0, 0, rep);
    let back = Packet::decode(&pkt.encode()).unwrap();
    assert_eq!(pkt, back);
}

#[test]
fn key_decode_ignores_reserved_and_trailing_body_bytes() {
    let rep = KeyboardReport {
        modifiers: 0x01,
        keys: [0x04, 0, 0, 0, 0, 0],
    };
    let mut buf = Packet::key_state(1, 0, rep).encode();
    buf[5] = 0xAB;
    buf[12] = 0xCD;
    buf[15] = 0xEF;
    buf[16] = crc8(&buf[..16]);
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
    assert_eq!(buf[4], 1);
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
    assert_eq!(buf[4], 4);
    assert_eq!(buf[5], 1);
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
        uptime_seconds: 0x0012_3456,
        unique_id_short: 0xDEAD_BEEF,
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
        unique_id_short: 0xDEAD_BEEF,
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
    let info = AckInfo {
        uptime_seconds: 0x01_AB_CD_EF,
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
    assert!(seq_is_newer(127, 0));
    assert!(!seq_is_newer(128, 0));
}

#[test]
fn ack_capability_flag_decoded() {
    let info = AckInfo {
        proto_version: PROTO_VERSION,
        fw_major: 1,
        fw_minor: 2,
        fw_patch: 3,
        board_type: BOARD_PICO_W_RP2040,
        uptime_seconds: 60,
        unique_id_short: 0xABCD_1234,
        full_version: None,
    };
    let mut buf = Packet::ack(7, info).encode();
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
