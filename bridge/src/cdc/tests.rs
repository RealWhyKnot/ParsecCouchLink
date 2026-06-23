use super::*;

#[test]
fn crc16_known_value() {
    // CRC-16/CCITT-FALSE check over "123456789" is 0x29B1.
    assert_eq!(crc16(b"123456789"), 0x29B1);
}

#[test]
fn frame_roundtrip() {
    let payload = b"hello pico";
    let buf = encode(CMD_HELLO, 7, payload);
    let (f, used) = try_decode(&buf).unwrap();
    assert_eq!(used, buf.len());
    assert_eq!(f.command, CMD_HELLO);
    assert_eq!(f.seq, 7);
    assert_eq!(f.payload, payload);
}

#[test]
fn frame_bad_crc() {
    let mut buf = encode(CMD_HELLO, 0, b"x");
    let last = buf.len() - 1;
    buf[last] ^= 0xFF;
    assert!(try_decode(&buf).is_err());
}

#[test]
fn short_unique_id_uses_first_four_bytes_little_endian() {
    let payload = [0xB6, 0x7E, 0xD3, 0x07, 0xAA, 0xBB, 0xCC, 0xDD];
    assert_eq!(short_unique_id_from_payload(&payload).unwrap(), 0x07D37EB6);
    assert!(short_unique_id_from_payload(&payload[..3]).is_err());
}

#[test]
fn bt_status_payload_decodes_wire_shape() {
    let name = b"CouchLink BT HID";
    let mut payload = vec![0u8; BT_STATUS_FIXED_LEN + name.len()];
    payload[0] = BT_STATUS_VERSION;
    payload[1] = BT_STATUS_FLAG_STARTED | BT_STATUS_FLAG_CONNECTED;
    payload[2] = 2;
    payload[3] = 0x44;
    payload[4] = 10;
    payload[6..8].copy_from_slice(&0x1234u16.to_le_bytes());
    payload[8..12].copy_from_slice(&1u32.to_le_bytes());
    payload[12..16].copy_from_slice(&2u32.to_le_bytes());
    payload[16..20].copy_from_slice(&3u32.to_le_bytes());
    payload[20..24].copy_from_slice(&4u32.to_le_bytes());
    payload[24..28].copy_from_slice(&5u32.to_le_bytes());
    payload[28..32].copy_from_slice(&6u32.to_le_bytes());
    payload[32..36].copy_from_slice(&7u32.to_le_bytes());
    payload[36..40].copy_from_slice(&8u32.to_le_bytes());
    payload[40..44].copy_from_slice(&0x11223344u32.to_le_bytes());
    payload[44..48].copy_from_slice(&0x55667788u32.to_le_bytes());
    payload[48..52].copy_from_slice(&9u32.to_le_bytes());
    payload[52..56].copy_from_slice(&10u32.to_le_bytes());
    payload[56..60].copy_from_slice(&11u32.to_le_bytes());
    payload[60..64].copy_from_slice(&12u32.to_le_bytes());
    payload[64..68].copy_from_slice(&13u32.to_le_bytes());
    payload[68..72].copy_from_slice(&14u32.to_le_bytes());
    payload[72..76].copy_from_slice(&15u32.to_le_bytes());
    payload[76..80].copy_from_slice(&16u32.to_le_bytes());
    payload[80..84].copy_from_slice(&17u32.to_le_bytes());
    payload[84] = 0x02;
    payload[85] = 3;
    payload[86] = 0x11;
    payload[87] = 2;
    payload[88] = 0x03;
    payload[89] = 2;
    payload[92..94].copy_from_slice(&36u16.to_le_bytes());
    payload[94..96].copy_from_slice(&77u16.to_le_bytes());
    payload[96..98].copy_from_slice(&8u16.to_le_bytes());
    payload[98] = name.len() as u8;
    payload[99..].copy_from_slice(name);

    let status = decode_bt_status_payload(&payload).unwrap();

    assert!(status.started());
    assert!(status.connected());
    assert!(!status.send_requested());
    assert_eq!(status.target, 2);
    assert_eq!(status.last_status, 0x44);
    assert_eq!(status.report_len, 10);
    assert_eq!(status.cid, 0x1234);
    assert_eq!(status.init_count, 1);
    assert_eq!(status.ready_count, 2);
    assert_eq!(status.open_count, 3);
    assert_eq!(status.close_count, 4);
    assert_eq!(status.can_send_count, 5);
    assert_eq!(status.report_build_count, 6);
    assert_eq!(status.report_send_count, 7);
    assert_eq!(status.send_request_count, 8);
    assert_eq!(status.last_event_ms, 0x11223344);
    assert_eq!(status.last_send_ms, 0x55667788);
    assert_eq!(status.get_report_count, 9);
    assert_eq!(status.get_report_success_count, 10);
    assert_eq!(status.get_report_unsupported_count, 11);
    assert_eq!(status.set_report_count, 12);
    assert_eq!(status.set_report_accepted_count, 13);
    assert_eq!(status.set_report_unsupported_count, 14);
    assert_eq!(status.out_report_count, 15);
    assert_eq!(status.out_report_accepted_count, 16);
    assert_eq!(status.out_report_unsupported_count, 17);
    assert_eq!(status.last_get_report_id, 0x02);
    assert_eq!(status.last_get_report_type, 3);
    assert_eq!(status.last_set_report_id, 0x11);
    assert_eq!(status.last_set_report_type, 2);
    assert_eq!(status.last_out_report_id, 0x03);
    assert_eq!(status.last_out_report_type, 2);
    assert_eq!(status.last_get_report_len, 36);
    assert_eq!(status.last_set_report_len, 77);
    assert_eq!(status.last_out_report_len, 8);
    assert_eq!(status.local_name, "CouchLink BT HID");
}

#[test]
fn bt_status_payload_decodes_v1_wire_shape() {
    let name = b"Legacy BT";
    let mut payload = vec![0u8; BT_STATUS_V1_FIXED_LEN + name.len()];
    payload[0] = BT_STATUS_V1_VERSION;
    payload[1] = BT_STATUS_FLAG_STARTED;
    payload[2] = 1;
    payload[32..36].copy_from_slice(&7u32.to_le_bytes());
    payload[48] = name.len() as u8;
    payload[49..].copy_from_slice(name);

    let status = decode_bt_status_payload(&payload).unwrap();

    assert!(status.started());
    assert!(!status.connected());
    assert_eq!(status.target, 1);
    assert_eq!(status.report_send_count, 7);
    assert_eq!(status.get_report_count, 0);
    assert_eq!(status.set_report_count, 0);
    assert_eq!(status.out_report_count, 0);
    assert_eq!(status.local_name, "Legacy BT");
}

#[test]
fn bt_status_payload_rejects_bad_shape() {
    assert!(decode_bt_status_payload(&[0; BT_STATUS_FIXED_LEN - 1]).is_err());
    let mut bad_version = vec![0u8; BT_STATUS_FIXED_LEN];
    bad_version[0] = BT_STATUS_VERSION + 1;
    assert!(decode_bt_status_payload(&bad_version).is_err());
    let mut bad_name = vec![0u8; BT_STATUS_FIXED_LEN];
    bad_name[0] = BT_STATUS_VERSION;
    bad_name[98] = 1;
    assert!(decode_bt_status_payload(&bad_name).is_err());
}
