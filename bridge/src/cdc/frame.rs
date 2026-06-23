use anyhow::{bail, Result};
pub const FRAME_MAGIC: [u8; 2] = [0xA5, 0x5A];
pub const PROTO_VERSION: u8 = 1;
pub const MAX_PAYLOAD: usize = 4 + 16384;
pub const HEADER_LEN: usize = 8; // magic(2) + ver(1) + cmd(1) + len(2) + seq(1) + reserved(1)
pub const CRC_LEN: usize = 2;
pub const MAX_FRAME: usize = HEADER_LEN + MAX_PAYLOAD + CRC_LEN;

// Request opcodes
pub const CMD_HELLO: u8 = 0x01;
pub const CMD_GET_STATUS: u8 = 0x02;
pub const CMD_SET_WIFI: u8 = 0x03;
pub const CMD_REBOOT_TO_RUN: u8 = 0x05;
pub const CMD_SELF_TEST: u8 = 0x06;
pub const CMD_GET_DEVICE_NAME: u8 = 0x07;
pub const CMD_SET_DEVICE_NAME: u8 = 0x08;
pub const CMD_GET_UNIQUE_ID: u8 = 0x09;
pub const CMD_GET_LOG_BUFFER: u8 = 0x0A;
pub const CMD_REBOOT_TO_BOOTSEL: u8 = 0x0B;
pub const CMD_BT_STATE: u8 = 0x0C;
pub const CMD_BT_HEARTBEAT: u8 = 0x0D;
pub const CMD_BT_GET_STATUS: u8 = 0x0E;

// Response opcodes
pub const RSP_HELLO: u8 = 0x81;
pub const RSP_STATUS: u8 = 0x82;
pub const RSP_SET_WIFI: u8 = 0x83;
pub const RSP_REBOOT: u8 = 0x85;
pub const RSP_SELF_TEST: u8 = 0x86;
pub const RSP_DEVICE_NAME: u8 = 0x87;
pub const RSP_SET_DEVICE_NAME: u8 = 0x88;
pub const RSP_UNIQUE_ID: u8 = 0x89;
pub const RSP_LOG_BUFFER: u8 = 0x8A;
pub const RSP_REBOOT_TO_BOOTSEL: u8 = 0x8B;
pub const RSP_BT_STATE: u8 = 0x8C;
pub const RSP_BT_HEARTBEAT: u8 = 0x8D;
pub const RSP_BT_STATUS: u8 = 0x8E;
pub const RSP_NACK: u8 = 0xFE;

/// Short human label for a response opcode, used in the rare "unexpected
/// response" errors so a protocol mismatch reads as a name rather than a
/// bare hex byte.
pub(crate) fn response_name(command: u8) -> &'static str {
    match command {
        RSP_HELLO => "HELLO_ACK",
        RSP_STATUS => "STATUS",
        RSP_SET_WIFI => "SET_WIFI_ACK",
        RSP_REBOOT => "REBOOT_ACK",
        RSP_SELF_TEST => "SELF_TEST_ACK",
        RSP_DEVICE_NAME => "DEVICE_NAME",
        RSP_SET_DEVICE_NAME => "SET_DEVICE_NAME_ACK",
        RSP_UNIQUE_ID => "UNIQUE_ID",
        RSP_LOG_BUFFER => "LOG_BUFFER",
        RSP_REBOOT_TO_BOOTSEL => "REBOOT_TO_BOOTSEL_ACK",
        RSP_BT_STATE => "BT_STATE_ACK",
        RSP_BT_HEARTBEAT => "BT_HEARTBEAT_ACK",
        RSP_BT_STATUS => "BT_STATUS",
        RSP_NACK => "NACK",
        _ => "unknown",
    }
}

// Error codes (carried in the NACK payload).
pub const ERR_BAD_CRC: u8 = 0x01;
pub const ERR_BAD_VERSION: u8 = 0x02;
pub const ERR_UNKNOWN_COMMAND: u8 = 0x03;
pub const ERR_BAD_LENGTH: u8 = 0x04;
pub const ERR_FLASH_WRITE_FAIL: u8 = 0x05;
pub const ERR_FLASH_VERIFY_FAIL: u8 = 0x06;
pub const ERR_WIFI_JOIN_TIMEOUT: u8 = 0x10;
pub const ERR_AUTH_FAIL: u8 = 0x11;
pub const ERR_NO_2G_NETWORK: u8 = 0x12;
pub const ERR_INTERNAL: u8 = 0xFF;

pub const HELLO_FLAG_CREDS_PRESENT: u8 = 0x01;
pub const HELLO_FLAG_WIFI_JOINED: u8 = 0x02;
pub const HELLO_FLAG_RUN_MODE_OK: u8 = 0x04;
pub const HELLO_FLAG_RUN_MODE_ACTIVE: u8 = 0x08;

pub const BT_STATUS_VERSION: u8 = 2;
pub const BT_STATUS_V1_VERSION: u8 = 1;
pub const BT_STATUS_V1_FIXED_LEN: usize = 49;
pub const BT_STATUS_FIXED_LEN: usize = 99;
pub const BT_STATUS_FLAG_STARTED: u8 = 1 << 0;
pub const BT_STATUS_FLAG_CONNECTED: u8 = 1 << 1;
pub const BT_STATUS_FLAG_SEND_REQUESTED: u8 = 1 << 2;

pub fn err_name(code: u8) -> &'static str {
    match code {
        ERR_BAD_CRC => "bad CRC",
        ERR_BAD_VERSION => "protocol version mismatch",
        ERR_UNKNOWN_COMMAND => "unknown command",
        ERR_BAD_LENGTH => "bad payload length",
        ERR_FLASH_WRITE_FAIL => "flash write failed",
        ERR_FLASH_VERIFY_FAIL => "flash verify after write failed",
        ERR_WIFI_JOIN_TIMEOUT => "Wi-Fi join timed out",
        ERR_AUTH_FAIL => "Wi-Fi auth rejected (wrong password?)",
        ERR_NO_2G_NETWORK => "no 2.4 GHz SSID found (Pico 2 W is 2.4 GHz only)",
        ERR_INTERNAL => "internal firmware error",
        _ => "unknown error",
    }
}

/// Lossless representation of a decoded frame.
#[derive(Clone, Debug)]
pub struct Frame {
    pub command: u8,
    pub seq: u8,
    pub payload: Vec<u8>,
}

pub fn crc16(data: &[u8]) -> u16 {
    // CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no reflect, no xor out.
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

pub fn encode(command: u8, seq: u8, payload: &[u8]) -> Vec<u8> {
    let n = payload.len();
    assert!(n <= MAX_PAYLOAD, "payload too large");
    let mut buf = Vec::with_capacity(HEADER_LEN + n + CRC_LEN);
    buf.extend_from_slice(&FRAME_MAGIC);
    buf.push(PROTO_VERSION);
    buf.push(command);
    buf.extend_from_slice(&(n as u16).to_le_bytes());
    buf.push(seq);
    buf.push(0); // reserved
    buf.extend_from_slice(payload);
    let crc = crc16(&buf);
    buf.extend_from_slice(&crc.to_le_bytes());
    buf
}

pub fn try_decode(buf: &[u8]) -> Result<(Frame, usize)> {
    if buf.len() < HEADER_LEN + CRC_LEN {
        bail!("frame too short ({} bytes)", buf.len());
    }
    if buf[0..2] != FRAME_MAGIC {
        bail!("frame magic mismatch");
    }
    if buf[2] != PROTO_VERSION {
        bail!(
            "frame protocol version mismatch (got {}, want {})",
            buf[2],
            PROTO_VERSION
        );
    }
    let payload_len = u16::from_le_bytes([buf[4], buf[5]]) as usize;
    if payload_len > MAX_PAYLOAD {
        bail!("frame payload length out of range: {payload_len}");
    }
    let total = HEADER_LEN + payload_len + CRC_LEN;
    if buf.len() < total {
        bail!("frame incomplete: need {total}, have {}", buf.len());
    }
    let crc_expected = u16::from_le_bytes([
        buf[HEADER_LEN + payload_len],
        buf[HEADER_LEN + payload_len + 1],
    ]);
    let crc_computed = crc16(&buf[..HEADER_LEN + payload_len]);
    if crc_expected != crc_computed {
        bail!("frame CRC mismatch (got 0x{crc_expected:04X}, want 0x{crc_computed:04X})");
    }
    Ok((
        Frame {
            command: buf[3],
            seq: buf[6],
            payload: buf[HEADER_LEN..HEADER_LEN + payload_len].to_vec(),
        },
        total,
    ))
}
