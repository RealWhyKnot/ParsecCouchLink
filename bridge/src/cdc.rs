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

use crate::firmware_version::FirmwareVersion;

mod frame;

pub use frame::*;

/// CouchLink CDC USB IDs. Setup mode and Bluetooth run mode both use this
/// identity; wired USB-output run personas use separate descriptors.
pub const SETUP_VID: u16 = 0x2E8A;
pub const SETUP_PID: u16 = 0xCAF0;

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

    pub fn write_frame_no_response(&mut self, command: u8, seq: u8, payload: &[u8]) -> Result<()> {
        let frame = encode(command, seq, payload);
        self.port.write_all(&frame).context("writing CDC frame")?;
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }
        Ok(())
    }

    // Like exchange() but produces a command-specific NACK error message.
    fn exchange_named(
        &mut self,
        command_label: &str,
        command: u8,
        payload: &[u8],
    ) -> Result<Frame> {
        self.exchange_named_with_timeout(command_label, command, payload, Duration::from_secs(3))
    }

    fn exchange_named_with_timeout(
        &mut self,
        command_label: &str,
        command: u8,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Frame> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(command, seq, payload);
        self.port.write_all(&frame).context("writing CDC frame")?;
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }
        let resp = self.read_one_frame_with_timeout(timeout)?;
        if resp.command == RSP_NACK {
            let code = resp.payload.first().copied().unwrap_or(ERR_INTERNAL);
            let detail = resp.payload.get(1).copied().unwrap_or(0);
            return Err(anyhow!(
                "Pico rejected {}: {} (code 0x{:02X}, detail 0x{:02X})",
                command_label,
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
            bail!(
                "unexpected response 0x{:02X} ({}) to HELLO",
                resp.command,
                response_name(resp.command)
            );
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
            firmware_version: FirmwareVersion::from_hello_payload(&resp.payload),
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
                        result: Err(format!(
                            "unexpected response 0x{:02X} ({})",
                            resp.command,
                            response_name(resp.command)
                        )),
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
                    firmware_version: FirmwareVersion::from_hello_payload(&resp.payload),
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
            bail!(
                "unexpected response 0x{:02X} ({}) to SET_WIFI",
                resp.command,
                response_name(resp.command)
            );
        }
        Ok(())
    }

    pub fn reboot_to_run(&mut self) -> Result<()> {
        self.reboot_with_ack(CMD_REBOOT_TO_RUN, RSP_REBOOT, "REBOOT_TO_RUN")
    }

    pub fn reboot_to_bootsel(&mut self) -> Result<()> {
        self.reboot_with_ack(
            CMD_REBOOT_TO_BOOTSEL,
            RSP_REBOOT_TO_BOOTSEL,
            "REBOOT_TO_BOOTSEL",
        )
    }

    fn reboot_with_ack(&mut self, command: u8, response: u8, label: &str) -> Result<()> {
        // After a reboot command, three outcomes are all acceptable:
        //   - the firmware replies with the expected ACK and then reboots,
        //   - the host sees a read/write error because the device disconnected
        //     before the reply made it across (race; firmware always reboots
        //     after handling, so the operation succeeded),
        //   - deadline elapses with the port still open and no reply
        //     (genuine failure; firmware hung).
        // The previous implementation propagated the second case as a hard
        // error, masking successful reboots as failures in the wizard.
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(command, seq, &[]);
        if let Err(e) = self.port.write_all(&frame) {
            if e.kind() != std::io::ErrorKind::TimedOut {
                tracing::info!(
                    "reboot: {label} write returned {:?} -- treating as success (Pico rebooted)",
                    e.kind(),
                );
                return Ok(());
            }
            return Err(e).with_context(|| format!("writing {label} frame"));
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
                            "reboot: {label} no RSP and port still open after 750 ms ({} bytes buffered)",
                            buf.len(),
                        );
                        bail!("{label}: no response and port still open after 750 ms");
                    }
                }
                Err(e) => {
                    tracing::info!(
                        "reboot: {label} read returned {:?} -- treating as success (Pico rebooted)",
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
                    if frame.command == response {
                        tracing::info!(
                            "reboot: got expected response 0x{:02X} for {label}",
                            response
                        );
                        return Ok(());
                    }
                    if frame.command == RSP_NACK {
                        let code = frame.payload.first().copied().unwrap_or(ERR_INTERNAL);
                        let detail = frame.payload.get(1).copied().unwrap_or(0);
                        return Err(anyhow!(
                            "Pico rejected {label}: {} (code 0x{:02X}, detail 0x{:02X})",
                            err_name(code),
                            code,
                            detail,
                        ));
                    }
                    bail!(
                        "unexpected response 0x{:02X} ({}) to {label}",
                        frame.command,
                        response_name(frame.command)
                    );
                }
            }
        }
    }

    pub fn self_test(&mut self) -> Result<SelfTestAck> {
        let resp = self.exchange_named("SELF_TEST", CMD_SELF_TEST, &[])?;
        if resp.command != RSP_SELF_TEST {
            bail!(
                "unexpected response 0x{:02X} ({}) to SELF_TEST",
                resp.command,
                response_name(resp.command)
            );
        }
        if resp.payload.is_empty() {
            bail!("SELF_TEST response truncated (0 bytes)");
        }
        Ok(SelfTestAck {
            passed: resp.payload[0] == 0,
            message: String::from_utf8_lossy(&resp.payload[1..]).into_owned(),
        })
    }

    pub fn unique_id_short(&mut self) -> Result<u32> {
        let resp = self.exchange_named("GET_UNIQUE_ID", CMD_GET_UNIQUE_ID, &[])?;
        if resp.command != RSP_UNIQUE_ID {
            bail!(
                "unexpected response 0x{:02X} ({}) to GET_UNIQUE_ID",
                resp.command,
                response_name(resp.command)
            );
        }
        short_unique_id_from_payload(&resp.payload)
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
                "unexpected response 0x{:02X} ({}) to GET_LOG_BUFFER",
                resp.command,
                response_name(resp.command)
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

    pub fn bt_status(&mut self) -> Result<BtStatus> {
        let resp = self.exchange_named_with_timeout(
            "BT_GET_STATUS",
            CMD_BT_GET_STATUS,
            &[],
            Duration::from_millis(250),
        )?;
        if resp.command != RSP_BT_STATUS {
            bail!(
                "unexpected response 0x{:02X} ({}) to BT_GET_STATUS",
                resp.command,
                response_name(resp.command)
            );
        }
        decode_bt_status_payload(&resp.payload)
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
    pub firmware_version: FirmwareVersion,
}

impl HelloAck {
    pub fn creds_present(&self) -> bool {
        self.flags & HELLO_FLAG_CREDS_PRESENT != 0
    }

    pub fn run_mode_active(&self) -> bool {
        self.flags & HELLO_FLAG_RUN_MODE_ACTIVE != 0
    }

    pub fn firmware_version(&self) -> FirmwareVersion {
        self.firmware_version
    }
}

#[derive(Clone, Debug)]
pub struct SelfTestAck {
    pub passed: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtStatus {
    pub flags: u8,
    pub target: u8,
    pub last_status: u8,
    pub report_len: u8,
    pub cid: u16,
    pub init_count: u32,
    pub ready_count: u32,
    pub open_count: u32,
    pub close_count: u32,
    pub can_send_count: u32,
    pub report_build_count: u32,
    pub report_send_count: u32,
    pub send_request_count: u32,
    pub last_event_ms: u32,
    pub last_send_ms: u32,
    pub get_report_count: u32,
    pub get_report_success_count: u32,
    pub get_report_unsupported_count: u32,
    pub set_report_count: u32,
    pub set_report_accepted_count: u32,
    pub set_report_unsupported_count: u32,
    pub out_report_count: u32,
    pub out_report_accepted_count: u32,
    pub out_report_unsupported_count: u32,
    pub last_get_report_id: u8,
    pub last_get_report_type: u8,
    pub last_set_report_id: u8,
    pub last_set_report_type: u8,
    pub last_out_report_id: u8,
    pub last_out_report_type: u8,
    pub last_get_report_len: u16,
    pub last_set_report_len: u16,
    pub last_out_report_len: u16,
    pub local_name: String,
}

impl BtStatus {
    pub fn started(&self) -> bool {
        self.flags & BT_STATUS_FLAG_STARTED != 0
    }

    pub fn connected(&self) -> bool {
        self.flags & BT_STATUS_FLAG_CONNECTED != 0
    }

    pub fn send_requested(&self) -> bool {
        self.flags & BT_STATUS_FLAG_SEND_REQUESTED != 0
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

fn short_unique_id_from_payload(payload: &[u8]) -> Result<u32> {
    if payload.len() < 4 {
        bail!("GET_UNIQUE_ID response truncated ({} bytes)", payload.len());
    }
    Ok(u32::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3],
    ]))
}

fn read_u16_le(payload: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([payload[offset], payload[offset + 1]])
}

fn read_u32_le(payload: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        payload[offset],
        payload[offset + 1],
        payload[offset + 2],
        payload[offset + 3],
    ])
}

pub fn decode_bt_status_payload(payload: &[u8]) -> Result<BtStatus> {
    if payload.is_empty() {
        bail!("BT_STATUS response truncated ({} bytes)", payload.len());
    }
    let version = payload[0];
    let fixed_len = match version {
        BT_STATUS_V1_VERSION => BT_STATUS_V1_FIXED_LEN,
        BT_STATUS_VERSION => BT_STATUS_FIXED_LEN,
        _ => bail!(
            "BT_STATUS version mismatch (got {}, want {} or {})",
            payload[0],
            BT_STATUS_V1_VERSION,
            BT_STATUS_VERSION
        ),
    };
    if payload.len() < fixed_len {
        bail!("BT_STATUS response truncated ({} bytes)", payload.len());
    }
    let name_len_offset = if version == BT_STATUS_V1_VERSION {
        BT_STATUS_V1_FIXED_LEN - 1
    } else {
        BT_STATUS_FIXED_LEN - 1
    };
    let name_start = fixed_len;
    let name_len = payload[name_len_offset] as usize;
    let need = name_start + name_len;
    if payload.len() < need {
        bail!(
            "BT_STATUS local name truncated (need {need} bytes, have {})",
            payload.len()
        );
    }
    let v2 = version == BT_STATUS_VERSION;
    Ok(BtStatus {
        flags: payload[1],
        target: payload[2],
        last_status: payload[3],
        report_len: payload[4],
        cid: read_u16_le(payload, 6),
        init_count: read_u32_le(payload, 8),
        ready_count: read_u32_le(payload, 12),
        open_count: read_u32_le(payload, 16),
        close_count: read_u32_le(payload, 20),
        can_send_count: read_u32_le(payload, 24),
        report_build_count: read_u32_le(payload, 28),
        report_send_count: read_u32_le(payload, 32),
        send_request_count: read_u32_le(payload, 36),
        last_event_ms: read_u32_le(payload, 40),
        last_send_ms: read_u32_le(payload, 44),
        get_report_count: if v2 { read_u32_le(payload, 48) } else { 0 },
        get_report_success_count: if v2 { read_u32_le(payload, 52) } else { 0 },
        get_report_unsupported_count: if v2 { read_u32_le(payload, 56) } else { 0 },
        set_report_count: if v2 { read_u32_le(payload, 60) } else { 0 },
        set_report_accepted_count: if v2 { read_u32_le(payload, 64) } else { 0 },
        set_report_unsupported_count: if v2 { read_u32_le(payload, 68) } else { 0 },
        out_report_count: if v2 { read_u32_le(payload, 72) } else { 0 },
        out_report_accepted_count: if v2 { read_u32_le(payload, 76) } else { 0 },
        out_report_unsupported_count: if v2 { read_u32_le(payload, 80) } else { 0 },
        last_get_report_id: if v2 { payload[84] } else { 0 },
        last_get_report_type: if v2 { payload[85] } else { 0 },
        last_set_report_id: if v2 { payload[86] } else { 0 },
        last_set_report_type: if v2 { payload[87] } else { 0 },
        last_out_report_id: if v2 { payload[88] } else { 0 },
        last_out_report_type: if v2 { payload[89] } else { 0 },
        last_get_report_len: if v2 { read_u16_le(payload, 92) } else { 0 },
        last_set_report_len: if v2 { read_u16_le(payload, 94) } else { 0 },
        last_out_report_len: if v2 { read_u16_le(payload, 96) } else { 0 },
        local_name: String::from_utf8_lossy(&payload[name_start..need]).into_owned(),
    })
}

pub fn error_has_nack_code(error: &anyhow::Error, code: u8) -> bool {
    let needle = format!("code 0x{code:02X}");
    error
        .chain()
        .any(|cause| cause.to_string().contains(&needle))
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
    let ports = find_setup_ports()?;
    ports.into_iter().next().ok_or_else(|| {
        anyhow!(
            "no Pico in setup mode found (looking for VID 0x{:04X} PID 0x{:04X}). \
             Make sure the Pico has just-flashed firmware and is plugged in.",
            SETUP_VID,
            SETUP_PID,
        )
    })
}

pub fn find_setup_ports() -> Result<Vec<String>> {
    let ports = serialport::available_ports().context("enumerating serial ports")?;
    let mut hits = Vec::new();
    for p in ports {
        if let SerialPortType::UsbPort(info) = &p.port_type {
            if info.vid == SETUP_VID && info.pid == SETUP_PID {
                hits.push(p.port_name);
            }
        }
    }
    hits.sort();
    Ok(hits)
}

#[cfg(test)]
mod tests;
