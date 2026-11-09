use super::frame::{
    CMD_HELLO, HELLO_FLAG_CREDS_PRESENT, HELLO_FLAG_RUN_MODE_ACTIVE, RSP_HELLO, encode,
    response_name,
};
use super::{PicoSetup, format_hex};
use crate::firmware_version::FirmwareVersion;
use anyhow::{Result, bail};
use std::io::Write;
use std::time::{Duration, Instant};

impl PicoSetup {
    pub(crate) fn hello(&mut self) -> Result<HelloAck> {
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

    pub(crate) fn hello_probe_with_timeout(&mut self, read_timeout: Duration) -> HelloProbe {
        let started = Instant::now();
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let frame = encode(CMD_HELLO, seq, &[]);
        let write_start = Instant::now();
        if let Err(e) = self.port.write_all(&frame) {
            return HelloProbe {
                port: self.port_name.clone(),
                step_reached: HelloProbeStep::Write,
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

        match self.read_one_frame_with_timeout(read_timeout) {
            Ok(resp) => {
                if resp.command != RSP_HELLO {
                    return HelloProbe {
                        port: self.port_name.clone(),
                        step_reached: HelloProbeStep::Decode,
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
                    bytes_received: resp.payload.len(),
                    rx_first_32_hex: format_hex(&resp.payload),
                    elapsed_ms: started.elapsed().as_millis(),
                    result: Ok(ack),
                }
            }
            Err(e) => HelloProbe {
                port: self.port_name.clone(),
                step_reached: HelloProbeStep::Read,
                bytes_received: 0,
                rx_first_32_hex: "none".to_string(),
                elapsed_ms: started.elapsed().as_millis(),
                result: Err(format!("{e:#}")),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HelloAck {
    pub proto_version: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub board_type: u8,
    pub flags: u8,
    pub firmware_version: FirmwareVersion,
}

impl HelloAck {
    pub(crate) fn creds_present(&self) -> bool {
        self.flags & HELLO_FLAG_CREDS_PRESENT != 0
    }

    pub(crate) fn run_mode_active(&self) -> bool {
        self.flags & HELLO_FLAG_RUN_MODE_ACTIVE != 0
    }

    pub(crate) fn firmware_version(&self) -> FirmwareVersion {
        self.firmware_version
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HelloProbe {
    pub port: String,
    pub step_reached: HelloProbeStep,
    pub bytes_received: usize,
    pub rx_first_32_hex: String,
    pub elapsed_ms: u128,
    pub result: Result<HelloAck, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HelloProbeStep {
    Write,
    Read,
    Decode,
    Done,
}

impl HelloProbeStep {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            HelloProbeStep::Write => "write",
            HelloProbeStep::Read => "read",
            HelloProbeStep::Decode => "decode",
            HelloProbeStep::Done => "done",
        }
    }
}
