use std::fmt::Write as _;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serialport::{SerialPort, SerialPortType};

mod bt_status;
mod frame;
mod hello;
mod provision;

pub(crate) use bt_status::{BtStatus, decode_bt_status_payload};
pub(crate) use frame::*;
pub(crate) use hello::HelloAck;

pub(crate) const SETUP_VID: u16 = 0x2E8A;
pub(crate) const SETUP_PID: u16 = 0xCAF0;

pub(crate) fn setup_pid_tag() -> String {
    format!("PID_{SETUP_PID:04X}")
}

pub(crate) fn setup_hardware_id() -> String {
    format!("VID_{SETUP_VID:04X}&PID_{SETUP_PID:04X}")
}

pub(crate) struct PicoSetup {
    port: Box<dyn SerialPort>,
    port_name: String,
    seq: u8,
}

impl PicoSetup {
    pub(crate) fn open_named(port_name: &str) -> Result<Self> {
        open_and_assert(port_name)
    }

    pub(crate) fn port_name(&self) -> &str {
        &self.port_name
    }

    pub(crate) fn write_frame_no_response(
        &mut self,
        command: u8,
        seq: u8,
        payload: &[u8],
    ) -> Result<()> {
        let frame = encode(command, seq, payload);
        self.port.write_all(&frame).context("writing CDC frame")?;
        if let Err(e) = self.port.flush() {
            tracing::debug!("cdc: flush after write returned {e:?}");
        }
        Ok(())
    }

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
            if let Some(start) = find_magic(&buf) {
                if start > 0 {
                    let head: Vec<u8> = buf.iter().take(16).copied().collect();
                    tracing::info!(
                        "cdc: drained {} pre-magic bytes from {} (first 16 = {})",
                        start,
                        self.port_name,
                        format_hex(&head),
                    );
                    buf.drain(..start);
                }
                if let Ok((frame, consumed)) = try_decode(&buf) {
                    buf.drain(..consumed);
                    return Ok(frame);
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
}

impl PicoSetup {
    pub(crate) fn self_test(&mut self) -> Result<SelfTestAck> {
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

    pub(crate) fn unique_id_short(&mut self) -> Result<u32> {
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

    pub(crate) fn get_log_buffer(&mut self) -> Result<(String, u32)> {
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
            Ok((String::from_utf8_lossy(&resp.payload).into_owned(), 0))
        }
    }

    pub(crate) fn bt_status(&mut self) -> Result<BtStatus> {
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

#[derive(Clone, Debug)]
pub(crate) struct SelfTestAck {
    pub passed: bool,
    pub message: String,
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
        let _ = write!(s, "{b:02X}");
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

pub(crate) fn error_has_nack_code(error: &anyhow::Error, code: u8) -> bool {
    let needle = format!("code 0x{code:02X}");
    error
        .chain()
        .any(|cause| cause.to_string().contains(&needle))
}

#[cfg(windows)]
pub(crate) fn classify_ghost_com(e: &std::io::Error) -> Option<&'static str> {
    match e.raw_os_error() {
        Some(995) => Some(
            "ERROR_OPERATION_ABORTED (995): the COM handle was cancelled, usually because the Pico re-enumerated",
        ),
        Some(1167) => Some(
            "ERROR_DEVICE_NOT_CONNECTED (1167): the Pico's USB endpoint went away (re-enumerated or unplugged)",
        ),
        Some(22) => Some(
            "ERROR_BAD_COMMAND (22): the driver lost the device, usually a stale handle after a Pico reset",
        ),
        Some(31) => Some(
            "ERROR_GEN_FAILURE (31): the USB stack returned a generic device failure, usually a stale handle after a Pico reset",
        ),
        _ => None,
    }
}

#[cfg(not(windows))]
pub fn classify_ghost_com(_e: &std::io::Error) -> Option<&'static str> {
    None
}

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

pub(crate) fn find_setup_port() -> Result<String> {
    let ports = find_setup_ports()?;
    ports.into_iter().next().ok_or_else(|| {
        anyhow!(
            "no Pico in setup mode found (looking for VID 0x{SETUP_VID:04X} PID 0x{SETUP_PID:04X}). \
             Make sure the Pico has just-flashed firmware and is plugged in.",
        )
    })
}

pub(crate) fn find_setup_ports() -> Result<Vec<String>> {
    let ports = serialport::available_ports().context("enumerating serial ports")?;
    let mut hits = Vec::new();
    for p in ports {
        if let SerialPortType::UsbPort(info) = &p.port_type
            && info.vid == SETUP_VID
            && info.pid == SETUP_PID
        {
            hits.push(p.port_name);
        }
    }
    hits.sort();
    Ok(hits)
}

#[cfg(test)]
mod tests;
