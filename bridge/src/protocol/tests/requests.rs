use crate::protocol::*;

#[test]
fn get_log_encode_shape() {
    let buf = encode_get_log(42);
    assert_eq!(buf.len(), PACKET_SIZE);
    assert_eq!(buf[0], MAGIC);
    assert_eq!(buf[1], TYPE_GET_LOG);
    assert_eq!(buf[2], 42);
    assert_eq!(buf[3], 0);
    for b in &buf[4..16] {
        assert_eq!(*b, 0);
    }
    assert_eq!(buf[16], crc8(&buf[..16]));
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
