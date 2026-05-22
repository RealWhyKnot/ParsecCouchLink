//! USB-CDC setup-mode framed protocol (see wiki/Protocol.md).
//!
//! All work in this module is blocking. Callers should wrap it in
//! `tokio::task::spawn_blocking` if they need to keep an async runtime
//! responsive.
//!
//! The full set of `CMD_*` / `RSP_*` opcodes and error codes is declared
//! here even when the current bridge only consumes a subset, so the file
//! tracks the spec one-to-one. `dead_code` is suppressed for that reason.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serialport::{SerialPort, SerialPortType};
use zeroize::Zeroize;

/// Pico setup-mode USB IDs. Distinct from the run-mode HID PID so Windows
/// does not cache one driver binding across a descriptor change.
pub const SETUP_VID: u16 = 0x2E8A;
pub const SETUP_PID: u16 = 0xCAF0;

pub const FRAME_MAGIC: [u8; 2] = [0xA5, 0x5A];
pub const PROTO_VERSION: u8 = 1;
pub const MAX_PAYLOAD: usize = 256;
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
pub const RSP_NACK: u8 = 0xFE;

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

/// One open setup-mode CDC connection. Handles request/response framing
/// and the no-pipelining rule.
pub struct PicoSetup {
    port: Box<dyn SerialPort>,
    port_name: String,
    seq: u8,
}

impl PicoSetup {
    /// Find the first Pico in setup mode (VID 2E8A, PID CAF0) and open it.
    pub fn open() -> Result<Self> {
        let port_name = find_setup_port()?;
        open_and_assert(&port_name)
    }

    pub fn open_named(port_name: &str) -> Result<Self> {
        open_and_assert(port_name)
    }

    /// Name of the underlying serial port, e.g. `"COM3"`. Carried so the
    /// HELLO timeout trace can name the port that went silent.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    pub fn exchange(&mut self, command: u8, payload: &[u8]) -> Result<Frame> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(command, seq, payload);
        self.port.write_all(&frame).context("writing CDC frame")?;
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }
        let resp = self.read_one_frame()?;
        if resp.command == RSP_NACK {
            let code = resp.payload.first().copied().unwrap_or(ERR_INTERNAL);
            let detail = resp.payload.get(1).copied().unwrap_or(0);
            return Err(anyhow!(
                "Pico NACK 0x{:02X} ({}), detail=0x{:02X}",
                code,
                err_name(code),
                detail,
            ));
        }
        Ok(resp)
    }

    // Like exchange() but produces a command-specific NACK error message.
    fn exchange_named(&mut self, cmd_label: &str, command: u8, payload: &[u8]) -> Result<Frame> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(command, seq, payload);
        self.port.write_all(&frame).context("writing CDC frame")?;
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }
        let resp = self.read_one_frame()?;
        if resp.command == RSP_NACK {
            let code = resp.payload.first().copied().unwrap_or(ERR_INTERNAL);
            let detail = resp.payload.get(1).copied().unwrap_or(0);
            return Err(anyhow!(
                "Pico rejected {}: {} (code 0x{:02X}, detail 0x{:02X})",
                cmd_label,
                err_name(code),
                code,
                detail,
            ));
        }
        Ok(resp)
    }

    fn read_one_frame(&mut self) -> Result<Frame> {
        self.read_one_frame_with_timeout(Duration::from_secs(3))
    }

    fn read_one_frame_with_timeout(&mut self, timeout: Duration) -> Result<Frame> {
        let mut buf = Vec::with_capacity(MAX_FRAME);
        let started = Instant::now();
        let deadline = started + timeout;
        loop {
            let mut tmp = [0u8; 256];
            match self.port.read(&mut tmp) {
                Ok(0) => {}
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    if Instant::now() >= deadline {
                        // Surface what the wire actually carried in the
                        // 3-second window. Without this the bundle can
                        // tell "the bridge gave up" but not "the bridge
                        // gave up after 0 bytes" vs "the bridge gave
                        // up after 12 bytes of UART noise".
                        let head: Vec<u8> = buf.iter().take(32).copied().collect();
                        let hex_str = if head.is_empty() {
                            "none".to_string()
                        } else {
                            format_hex(&head)
                        };
                        tracing::error!(
                            "cdc: read timeout on {port} after {elapsed} ms with \
                             {n} bytes received (first 32 = {hex})",
                            port = self.port_name,
                            elapsed = started.elapsed().as_millis(),
                            n = buf.len(),
                            hex = hex_str,
                        );
                        crate::journal!(
                            "cdc",
                            "read timeout on {} after {} ms; rx_bytes={} first32={}",
                            self.port_name,
                            started.elapsed().as_millis(),
                            buf.len(),
                            hex_str
                        );
                        bail!("timed out waiting for Pico response");
                    }
                }
                Err(e) => {
                    if let Some(why) = classify_ghost_com(&e) {
                        tracing::error!(
                            "cdc: read failed with {why}. The Pico likely just rebooted \
                             -- unplug and re-plug it (no need to hold BOOTSEL), then \
                             re-run the failing command."
                        );
                    }
                    return Err(e).context("reading CDC frame");
                }
            }
            // Resync: find magic in buf and try to decode from there.
            if let Some(start) = find_magic(&buf) {
                if start > 0 {
                    // A firmware that's alive but mis-framing leaves bytes
                    // here, and we want that to be visible at default
                    // verbosity, not buried at debug. The "buffer overflow"
                    // path below catches the much-later case where the
                    // garbage never resyncs.
                    let head: Vec<u8> = buf.iter().take(16).copied().collect();
                    tracing::info!(
                        "cdc: drained {} pre-magic bytes from {} (first 16 = {})",
                        start,
                        self.port_name,
                        format_hex(&head),
                    );
                    buf.drain(..start);
                }
                match try_decode(&buf) {
                    Ok((frame, consumed)) => {
                        buf.drain(..consumed);
                        return Ok(frame);
                    }
                    Err(_) => {
                        // incomplete or invalid; keep reading
                    }
                }
            }
            if buf.len() > MAX_FRAME * 4 {
                let head: Vec<u8> = buf.iter().take(16).copied().collect();
                tracing::warn!(
                    "cdc: receive buffer overflow on {} with no valid frame ({} bytes, first 16 = {})",
                    self.port_name,
                    buf.len(),
                    format_hex(&head),
                );
                bail!(
                    "CDC receive buffer overflow ({} bytes of garbage)",
                    buf.len()
                );
            }
        }
    }

    pub fn hello(&mut self) -> Result<HelloAck> {
        let resp = self.exchange_named("HELLO", CMD_HELLO, &[])?;
        if resp.command != RSP_HELLO {
            bail!("unexpected response 0x{:02X} to HELLO", resp.command);
        }
        if resp.payload.len() < 6 {
            bail!("HELLO_ACK truncated ({} bytes)", resp.payload.len());
        }
        Ok(HelloAck {
            proto_version: resp.payload[0],
            fw_major: resp.payload[1],
            fw_minor: resp.payload[2],
            fw_patch: resp.payload[3],
            board_type: resp.payload[4],
            flags: resp.payload[5],
        })
    }

    /// Variant of `hello()` that captures per-step state for the bundle's
    /// pico-diag.txt stub. Returns enough detail that an operator can
    /// distinguish "firmware silent" from "firmware mis-framing" from
    /// "firmware responded but we mis-decoded" without reading the bridge
    /// log. Used by `cmd_bundle::capture_pico_diag()`.
    pub fn hello_probe(&mut self) -> HelloProbe {
        self.hello_probe_with_timeout(Duration::from_secs(3))
    }

    /// Like `hello_probe()` but with a configurable read timeout. The
    /// bundle uses this with a longer deadline (10 s) so we capture
    /// late-arriving bytes from a slowly-booting firmware -- not all
    /// bringup paths complete inside the 3 s wizard budget.
    pub fn hello_probe_with_timeout(&mut self, read_timeout: Duration) -> HelloProbe {
        let started = Instant::now();
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(CMD_HELLO, seq, &[]);
        let frame_hex = format_hex(&frame);
        let write_start = Instant::now();
        if let Err(e) = self.port.write_all(&frame) {
            return HelloProbe {
                port: self.port_name.clone(),
                step_reached: HelloProbeStep::Write,
                frame_written_hex: frame_hex,
                bytes_received: 0,
                rx_first_32_hex: "none".to_string(),
                elapsed_ms: started.elapsed().as_millis(),
                result: Err(format!("write_all: {e:#}")),
            };
        }
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: probe flush returned {e:?}");
        }
        let write_elapsed = write_start.elapsed();
        tracing::info!(
            "cdc: probe wrote HELLO frame on {} ({} bytes in {} ms, read deadline {} ms)",
            self.port_name,
            frame.len(),
            write_elapsed.as_millis(),
            read_timeout.as_millis(),
        );

        // Reuse the same read loop so we capture exactly what the bridge
        // would see during a real HELLO. read_one_frame_with_timeout
        // emits the timeout / drained-bytes traces, which is what we
        // want.
        match self.read_one_frame_with_timeout(read_timeout) {
            Ok(resp) => {
                if resp.command != RSP_HELLO {
                    return HelloProbe {
                        port: self.port_name.clone(),
                        step_reached: HelloProbeStep::Decode,
                        frame_written_hex: frame_hex,
                        bytes_received: resp.payload.len(),
                        rx_first_32_hex: format_hex(
                            &resp.payload.iter().take(32).copied().collect::<Vec<u8>>(),
                        ),
                        elapsed_ms: started.elapsed().as_millis(),
                        result: Err(format!("unexpected response 0x{:02X}", resp.command)),
                    };
                }
                if resp.payload.len() < 6 {
                    return HelloProbe {
                        port: self.port_name.clone(),
                        step_reached: HelloProbeStep::Decode,
                        frame_written_hex: frame_hex,
                        bytes_received: resp.payload.len(),
                        rx_first_32_hex: format_hex(&resp.payload),
                        elapsed_ms: started.elapsed().as_millis(),
                        result: Err(format!(
                            "HELLO_ACK truncated ({} bytes)",
                            resp.payload.len()
                        )),
                    };
                }
                let ack = HelloAck {
                    proto_version: resp.payload[0],
                    fw_major: resp.payload[1],
                    fw_minor: resp.payload[2],
                    fw_patch: resp.payload[3],
                    board_type: resp.payload[4],
                    flags: resp.payload[5],
                };
                HelloProbe {
                    port: self.port_name.clone(),
                    step_reached: HelloProbeStep::Done,
                    frame_written_hex: frame_hex,
                    bytes_received: resp.payload.len(),
                    rx_first_32_hex: format_hex(&resp.payload),
                    elapsed_ms: started.elapsed().as_millis(),
                    result: Ok(ack),
                }
            }
            Err(e) => HelloProbe {
                port: self.port_name.clone(),
                step_reached: HelloProbeStep::Read,
                frame_written_hex: frame_hex,
                bytes_received: 0,
                rx_first_32_hex: "none".to_string(),
                elapsed_ms: started.elapsed().as_millis(),
                result: Err(format!("{e:#}")),
            },
        }
    }

    /// Push Wi-Fi credentials. The buffer is zeroed before returning.
    pub fn set_wifi(&mut self, ssid: &str, password: &mut String) -> Result<()> {
        if ssid.is_empty() || ssid.len() > 32 {
            bail!("SSID length out of range (1..=32)");
        }
        if password.len() > 63 {
            bail!("Wi-Fi password too long (max 63)");
        }
        let mut buf = Vec::with_capacity(2 + ssid.len() + password.len());
        buf.push(ssid.len() as u8);
        buf.extend_from_slice(ssid.as_bytes());
        buf.push(password.len() as u8);
        buf.extend_from_slice(password.as_bytes());
        // exchange_named is called via a manual inline here so we can zeroize
        // buf and password before propagating any error.
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(CMD_SET_WIFI, seq, &buf);
        buf.zeroize();
        let write_result = self.port.write_all(&frame).context("writing CDC frame");
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }
        password.zeroize();
        write_result?;
        let resp = self.read_one_frame()?;
        if resp.command == RSP_NACK {
            let code = resp.payload.first().copied().unwrap_or(ERR_INTERNAL);
            let detail = resp.payload.get(1).copied().unwrap_or(0);
            return Err(anyhow!(
                "Pico rejected SET_WIFI: {} (code 0x{:02X}, detail 0x{:02X})",
                err_name(code),
                code,
                detail,
            ));
        }
        if resp.command != RSP_SET_WIFI {
            bail!("unexpected response 0x{:02X} to SET_WIFI", resp.command);
        }
        Ok(())
    }

    pub fn reboot_to_run(&mut self) -> Result<()> {
        // After CMD_REBOOT_TO_RUN, three outcomes are all acceptable:
        //   - the firmware replies RSP_REBOOT and then reboots (happy path),
        //   - the host sees a read/write error because the device disconnected
        //     before the reply made it across (race; firmware always reboots
        //     after handling, so the operation succeeded),
        //   - deadline elapses with the port still open and no reply
        //     (genuine failure; firmware hung).
        // The previous implementation propagated the second case as a hard
        // error, masking successful reboots as failures in the wizard.
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(CMD_REBOOT_TO_RUN, seq, &[]);
        if let Err(e) = self.port.write_all(&frame) {
            if e.kind() != std::io::ErrorKind::TimedOut {
                tracing::info!(
                    "reboot: write returned {:?} -- treating as success (Pico rebooted)",
                    e.kind(),
                );
                return Ok(());
            }
            return Err(e).context("writing REBOOT_TO_RUN frame");
        }
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }

        let deadline = Instant::now() + Duration::from_millis(750);
        let mut buf = Vec::with_capacity(MAX_FRAME);
        loop {
            let mut tmp = [0u8; 64];
            match self.port.read(&mut tmp) {
                Ok(0) => {}
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    if Instant::now() >= deadline {
                        tracing::warn!(
                            "reboot: no RSP and port still open after 750 ms ({} bytes buffered)",
                            buf.len(),
                        );
                        bail!("REBOOT_TO_RUN: no response and port still open after 750 ms");
                    }
                }
                Err(e) => {
                    tracing::info!(
                        "reboot: read returned {:?} -- treating as success (Pico rebooted)",
                        e.kind(),
                    );
                    return Ok(());
                }
            }
            if let Some(start) = find_magic(&buf) {
                if start > 0 {
                    buf.drain(..start);
                }
                if let Ok((frame, _consumed)) = try_decode(&buf) {
                    if frame.command == RSP_REBOOT {
                        tracing::info!("reboot: got RSP_REBOOT (0x85)");
                        return Ok(());
                    }
                    if frame.command == RSP_NACK {
                        let code = frame.payload.first().copied().unwrap_or(ERR_INTERNAL);
                        let detail = frame.payload.get(1).copied().unwrap_or(0);
                        return Err(anyhow!(
                            "Pico rejected REBOOT_TO_RUN: {} (code 0x{:02X}, detail 0x{:02X})",
                            err_name(code),
                            code,
                            detail,
                        ));
                    }
                    bail!(
                        "unexpected response 0x{:02X} to REBOOT_TO_RUN",
                        frame.command,
                    );
                }
            }
        }
    }

    /// Fetch the firmware's in-RAM diagnostic ring buffer. Returns the
    /// captured bytes as UTF-8 (lossy on invalid sequences) plus a count
    /// of how many older bytes were dropped from the ring due to overflow.
    /// Empty string + 0 means the buffer was empty; an Err means a
    /// transport or protocol failure.
    ///
    /// Wire format on firmware >= v2026.5.16.5: the response payload is
    /// 4 bytes little-endian `lost` followed by the log text. Older
    /// firmware sends the log text only; this method degrades to
    /// `lost = 0` when the payload is short.
    pub fn get_log_buffer(&mut self) -> Result<(String, u32)> {
        let resp = self.exchange_named("GET_LOG_BUFFER", CMD_GET_LOG_BUFFER, &[])?;
        if resp.command != RSP_LOG_BUFFER {
            bail!(
                "unexpected response 0x{:02X} to GET_LOG_BUFFER",
                resp.command
            );
        }
        if resp.payload.len() >= 4 {
            let lost = u32::from_le_bytes([
                resp.payload[0],
                resp.payload[1],
                resp.payload[2],
                resp.payload[3],
            ]);
            let text = String::from_utf8_lossy(&resp.payload[4..]).into_owned();
            Ok((text, lost))
        } else {
            // Pre-v2026.5.16.5 firmware: no prefix, just the log text.
            Ok((String::from_utf8_lossy(&resp.payload).into_owned(), 0))
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HelloAck {
    pub proto_version: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub board_type: u8,
    pub flags: u8,
}

impl HelloAck {
    pub fn creds_present(&self) -> bool {
        self.flags & 0x01 != 0
    }
}

/// Outcome of a single HELLO wire exchange, with enough detail for the
/// bundle's pico-diag.txt stub to name the failure step. The probe runs
/// the same write + read loop as `PicoSetup::hello()` -- this is meant
/// to mirror reality, not a separate code path.
#[derive(Clone, Debug)]
pub struct HelloProbe {
    pub port: String,
    pub step_reached: HelloProbeStep,
    pub frame_written_hex: String,
    pub bytes_received: usize,
    pub rx_first_32_hex: String,
    pub elapsed_ms: u128,
    pub result: Result<HelloAck, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloProbeStep {
    /// The HELLO frame did not even leave the bridge.
    Write,
    /// Frame sent; no valid response decoded within the deadline.
    Read,
    /// Bytes received and decoded; the response was the wrong shape
    /// (wrong opcode or truncated HELLO_ACK).
    Decode,
    /// Full HELLO_ACK parsed.
    Done,
}

impl HelloProbeStep {
    pub fn as_str(self) -> &'static str {
        match self {
            HelloProbeStep::Write => "write",
            HelloProbeStep::Read => "read",
            HelloProbeStep::Decode => "decode",
            HelloProbeStep::Done => "done",
        }
    }
}

fn find_magic(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == FRAME_MAGIC)
}

fn format_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// Returns Some(human-readable cause) when the underlying serial I/O
/// failure looks like a "ghost COM" -- the Pico re-enumerated under
/// the bridge's open handle and Windows returned the handle as
/// invalid. The caller is expected to log + recommend a re-plug; a
/// full reconnect-by-unique-id is deferred to a future pass.
#[cfg(windows)]
pub fn classify_ghost_com(e: &std::io::Error) -> Option<&'static str> {
    match e.raw_os_error() {
        Some(995)  => Some("ERROR_OPERATION_ABORTED (995): the COM handle was cancelled, usually because the Pico re-enumerated"),
        Some(1167) => Some("ERROR_DEVICE_NOT_CONNECTED (1167): the Pico's USB endpoint went away (re-enumerated or unplugged)"),
        Some(22)   => Some("ERROR_BAD_COMMAND (22): the driver lost the device, usually a stale handle after a Pico reset"),
        Some(31)   => Some("ERROR_GEN_FAILURE (31): the USB stack returned a generic device failure, usually a stale handle after a Pico reset"),
        _ => None,
    }
}

#[cfg(not(windows))]
pub fn classify_ghost_com(_e: &std::io::Error) -> Option<&'static str> {
    None
}

// Open the named serial port and explicitly assert DTR + RTS. The
// firmware's tud_cdc_connected() is driven by DTR, and some Windows
// serial-port opens leave DTR low until the application drives it.
// Without the explicit assert, the firmware sees "host not opened"
// and silently discards incoming bytes, including HELLO -- which
// then times out 3 s later with no obvious clue why. The line state
// transitions also surface in the firmware's diag log via
// tud_cdc_line_state_cb, so a bundle from a successful open vs. a
// failed open look visibly different.
fn open_and_assert(port_name: &str) -> Result<PicoSetup> {
    let mut port = serialport::new(port_name, 1_000_000)
        .timeout(Duration::from_millis(500))
        .open()
        .with_context(|| {
            crate::journal!("cdc", "open {port_name} failed");
            format!("opening serial port {port_name}")
        })?;
    crate::journal!("cdc", "opened {port_name} @ 1Mbaud, 500ms read timeout");

    let dtr_ok = match port.write_data_terminal_ready(true) {
        Ok(()) => {
            tracing::info!("cdc: asserted DTR on {port_name}");
            true
        }
        Err(e) => {
            tracing::warn!("cdc: could not assert DTR on {port_name}: {e:?} -- HELLO may time out");
            false
        }
    };
    let rts_ok = match port.write_request_to_send(true) {
        Ok(()) => {
            tracing::info!("cdc: asserted RTS on {port_name}");
            true
        }
        Err(e) => {
            tracing::warn!("cdc: could not assert RTS on {port_name}: {e:?} -- usually harmless");
            false
        }
    };
    crate::journal!(
        "cdc",
        "{port_name} line driven dtr={} rts={}",
        if dtr_ok { "ok" } else { "FAIL" },
        if rts_ok { "ok" } else { "FAIL" }
    );

    Ok(PicoSetup {
        port,
        port_name: port_name.to_string(),
        seq: 0,
    })
}

pub fn find_setup_port() -> Result<String> {
    let ports = serialport::available_ports().context("enumerating serial ports")?;
    for p in ports {
        if let SerialPortType::UsbPort(info) = &p.port_type {
            if info.vid == SETUP_VID && info.pid == SETUP_PID {
                return Ok(p.port_name);
            }
        }
    }
    Err(anyhow!(
        "no Pico in setup mode found (looking for VID 0x{:04X} PID 0x{:04X}). \
         Make sure the Pico has just-flashed firmware and is plugged in.",
        SETUP_VID,
        SETUP_PID,
    ))
}

#[cfg(test)]
mod tests {
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
}
