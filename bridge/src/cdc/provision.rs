use super::frame::{
    CMD_REBOOT_TO_BOOTSEL, CMD_REBOOT_TO_RUN, CMD_SET_AUTH_KEY, CMD_SET_WIFI, ERR_INTERNAL,
    MAX_FRAME, RSP_NACK, RSP_REBOOT, RSP_REBOOT_TO_BOOTSEL, RSP_SET_AUTH_KEY, RSP_SET_WIFI, encode,
    err_name, response_name, try_decode,
};
use super::{PicoSetup, find_magic};
use anyhow::{Context, Result, anyhow, bail};
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

impl PicoSetup {
    pub(crate) fn set_wifi(&mut self, ssid: &str, password: &mut String) -> Result<()> {
        if ssid.is_empty() || ssid.len() > 32 {
            bail!("SSID length out of range (1..=32)");
        }
        if password.len() > 63 {
            bail!("Wi-Fi password too long (max 63)");
        }
        let mut buf = Vec::with_capacity(2 + ssid.len() + password.len());
        buf.push(u8::try_from(ssid.len())?);
        buf.extend_from_slice(ssid.as_bytes());
        buf.push(u8::try_from(password.len())?);
        buf.extend_from_slice(password.as_bytes());
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

    pub(crate) fn set_auth_key(&mut self, key: &[u8; 32]) -> Result<()> {
        let resp = self.exchange_named("SET_AUTH_KEY", CMD_SET_AUTH_KEY, key)?;
        if resp.command != RSP_SET_AUTH_KEY {
            bail!(
                "unexpected response 0x{:02X} ({}) to SET_AUTH_KEY",
                resp.command,
                response_name(resp.command)
            );
        }
        Ok(())
    }

    pub(crate) fn reboot_to_run(&mut self) -> Result<()> {
        self.reboot_with_ack(CMD_REBOOT_TO_RUN, RSP_REBOOT, "REBOOT_TO_RUN")
    }

    pub(crate) fn reboot_to_bootsel(&mut self) -> Result<()> {
        self.reboot_with_ack(
            CMD_REBOOT_TO_BOOTSEL,
            RSP_REBOOT_TO_BOOTSEL,
            "REBOOT_TO_BOOTSEL",
        )
    }

    fn reboot_with_ack(&mut self, command: u8, response: u8, label: &str) -> Result<()> {
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
}
