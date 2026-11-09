use super::wire::crc8;
use super::{
    MAGIC, PACKET_SIZE, Persona, TYPE_GET_LOG, TYPE_GET_PICO_STATE, TYPE_GET_USB_DIAG,
    TYPE_GET_VERSION, TYPE_IDENTIFY, TYPE_REBOOT_TO_SETUP, TYPE_SET_PERSONA, TYPE_SET_USB_CAPTURE,
};

pub(crate) fn encode_get_log(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_LOG;
    buf[2] = seq;
    buf[3] = 0;
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_get_usb_diag(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_USB_DIAG;
    buf[2] = seq;
    buf[3] = 0;
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_reboot_to_setup(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_REBOOT_TO_SETUP;
    buf[2] = seq;
    buf[3] = 0;
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_set_persona(seq: u8, persona: Persona) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_SET_PERSONA;
    buf[2] = seq;
    buf[3] = 0;
    buf[4] = persona.flash_byte();
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_set_usb_capture(
    seq: u8,
    persona: Persona,
    enabled: bool,
) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_SET_USB_CAPTURE;
    buf[2] = seq;
    buf[3] = 0;
    buf[4] = persona.flash_byte();
    buf[5] = u8::from(enabled);
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_get_version(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_VERSION;
    buf[2] = seq;
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_get_pico_state(seq: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_GET_PICO_STATE;
    buf[2] = seq;
    buf[16] = crc8(&buf[..16]);
    buf
}

pub(crate) fn encode_identify(seq: u8, blink_seconds: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];
    buf[0] = MAGIC;
    buf[1] = TYPE_IDENTIFY;
    buf[2] = seq;
    buf[4] = blink_seconds;
    buf[16] = crc8(&buf[..16]);
    buf
}
