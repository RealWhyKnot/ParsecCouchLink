mod stubs;
mod udp;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::protocol::ACK_FLAG_LOG_CHUNK_SUPPORTED;
use crate::{cdc, cmd_run};

use stubs::{
    captured_stub, empty_stub, no_last_pico_in_config_stub, no_setup_port_stub,
    setup_open_failed_stub, setup_probe_failed_stub, udp_discovery_failed_stub,
    udp_probe_failed_stub, udp_unsupported_stub, vendor_not_found_stub, vendor_open_failed_stub,
    vendor_transfer_failed_stub,
};
use udp::{capture_log_chunks, try_capture_run_udp, unicast_for_ack};

#[derive(Clone, Debug)]
pub(super) enum DiagSource {
    SetupCdc,
    VendorControl,
    RunUdp { peer: SocketAddr },
}

impl DiagSource {
    fn as_str(&self) -> &'static str {
        match self {
            DiagSource::SetupCdc => "setup-cdc",
            DiagSource::VendorControl => "vendor-control",
            DiagSource::RunUdp { .. } => "run-udp",
        }
    }

    fn describe(&self) -> String {
        match self {
            DiagSource::SetupCdc => "setup-mode USB-CDC".to_string(),
            DiagSource::VendorControl => "USB vendor control transfer".to_string(),
            DiagSource::RunUdp { peer } => format!("run-mode UDP from {peer}"),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum DiagOutcome {
    Captured {
        source: DiagSource,
        text: String,
        lost: u32,
    },
    Empty {
        source: DiagSource,
    },
    NoSetupPort,
    SetupOpenFailed {
        error: String,
    },
    SetupProbeFailed {
        port: String,
        step: &'static str,
        elapsed_ms: u128,
        bytes_received: usize,
        rx_first_32_hex: String,
        error: String,
    },
    NoLastPicoInConfig,
    VendorNotFound,
    VendorOpenFailed {
        error: String,
    },
    VendorTransferFailed {
        step: &'static str,
        bytes_received: usize,
        error: String,
    },
    UdpDiscoveryFailed {
        reason: String,
    },
    UdpProbeFailed {
        peer: SocketAddr,
        step: &'static str,
        elapsed_ms: u128,
        chunks_received: u16,
        error: String,
    },
    UdpUnsupported {
        peer: SocketAddr,
    },
}

impl DiagOutcome {
    pub(super) fn discriminant_str(&self) -> &'static str {
        match self {
            DiagOutcome::Captured { .. } => "captured",
            DiagOutcome::Empty { .. } => "empty",
            DiagOutcome::NoSetupPort => "no_setup_port",
            DiagOutcome::SetupOpenFailed { .. } => "setup_open_failed",
            DiagOutcome::SetupProbeFailed { .. } => "setup_probe_failed",
            DiagOutcome::NoLastPicoInConfig => "no_last_pico_in_config",
            DiagOutcome::VendorNotFound => "vendor_not_found",
            DiagOutcome::VendorOpenFailed { .. } => "vendor_open_failed",
            DiagOutcome::VendorTransferFailed { .. } => "vendor_transfer_failed",
            DiagOutcome::UdpDiscoveryFailed { .. } => "udp_discovery_failed",
            DiagOutcome::UdpProbeFailed { .. } => "udp_probe_failed",
            DiagOutcome::UdpUnsupported { .. } => "udp_unsupported",
        }
    }

    pub(super) fn source_str(&self) -> Option<&'static str> {
        match self {
            DiagOutcome::Captured { source, .. } | DiagOutcome::Empty { source } => {
                Some(source.as_str())
            }
            _ => None,
        }
    }

    pub(super) fn lost_bytes(&self) -> u32 {
        match self {
            DiagOutcome::Captured { lost, .. } => *lost,
            _ => 0,
        }
    }

    pub(super) fn stub_text(&self) -> String {
        match self {
            DiagOutcome::Captured { text, lost, source } => captured_stub(text, *lost, source),
            DiagOutcome::Empty { source } => empty_stub(source),
            DiagOutcome::NoSetupPort => no_setup_port_stub(),
            DiagOutcome::SetupOpenFailed { error } => setup_open_failed_stub(error),
            DiagOutcome::SetupProbeFailed {
                port,
                step,
                elapsed_ms,
                bytes_received,
                rx_first_32_hex,
                error,
            } => setup_probe_failed_stub(
                port,
                step,
                *elapsed_ms,
                *bytes_received,
                rx_first_32_hex,
                error,
            ),
            DiagOutcome::NoLastPicoInConfig => no_last_pico_in_config_stub(),
            DiagOutcome::VendorNotFound => vendor_not_found_stub(),
            DiagOutcome::VendorOpenFailed { error } => vendor_open_failed_stub(error),
            DiagOutcome::VendorTransferFailed {
                step,
                bytes_received,
                error,
            } => vendor_transfer_failed_stub(step, *bytes_received, error),
            DiagOutcome::UdpDiscoveryFailed { reason } => udp_discovery_failed_stub(reason),
            DiagOutcome::UdpProbeFailed {
                peer,
                step,
                elapsed_ms,
                chunks_received,
                error,
            } => udp_probe_failed_stub(*peer, step, *elapsed_ms, *chunks_received, error),
            DiagOutcome::UdpUnsupported { peer } => udp_unsupported_stub(*peer),
        }
    }
}

pub(super) async fn capture_pico_diag() -> DiagOutcome {
    let cdc_result = try_capture_setup_cdc().await;
    if matches!(
        cdc_result,
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. }
    ) {
        return cdc_result;
    }

    tracing::info!(
        "bundle: CDC diag path returned {}, trying vendor control transfer",
        cdc_result.discriminant_str()
    );
    let vendor_result = try_capture_vendor_control().await;
    if matches!(
        vendor_result,
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. }
    ) {
        tracing::info!("bundle: diag captured via USB vendor control transfer");
        return vendor_result;
    }

    tracing::info!(
        "bundle: vendor control path returned {}, trying run-mode UDP",
        vendor_result.discriminant_str()
    );
    let udp_result = try_capture_run_udp().await;
    if matches!(
        udp_result,
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. }
    ) {
        tracing::info!("bundle: diag captured via UDP TYPE_GET_LOG");
        return udp_result;
    }

    tracing::warn!(
        "bundle: all three diag paths failed (cdc={}, vendor={}, udp={})",
        cdc_result.discriminant_str(),
        vendor_result.discriminant_str(),
        udp_result.discriminant_str()
    );
    choose_failed_diag_outcome(cdc_result, vendor_result, udp_result)
}

fn choose_failed_diag_outcome(
    cdc_result: DiagOutcome,
    vendor_result: DiagOutcome,
    udp_result: DiagOutcome,
) -> DiagOutcome {
    let mut best = cdc_result;
    for candidate in [vendor_result, udp_result] {
        if failure_rank(&candidate) > failure_rank(&best) {
            best = candidate;
        }
    }
    best
}

fn failure_rank(outcome: &DiagOutcome) -> u8 {
    match outcome {
        DiagOutcome::Captured { .. } | DiagOutcome::Empty { .. } => 100,
        DiagOutcome::SetupProbeFailed { .. }
        | DiagOutcome::VendorTransferFailed { .. }
        | DiagOutcome::UdpProbeFailed { .. } => 60,
        DiagOutcome::SetupOpenFailed { .. } | DiagOutcome::VendorOpenFailed { .. } => 50,
        DiagOutcome::UdpUnsupported { .. } => 45,
        DiagOutcome::UdpDiscoveryFailed { .. } => 40,
        DiagOutcome::NoLastPicoInConfig => 20,
        DiagOutcome::NoSetupPort | DiagOutcome::VendorNotFound => 10,
    }
}

pub(super) async fn capture_run_udp_for_target(pico: &cmd_run::PicoTarget) -> DiagOutcome {
    let socket = match crate::net::bind_udp("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("bind: {e}"),
            };
        }
    };
    let ack_started = Instant::now();
    let ack_packet = match unicast_for_ack(&socket, pico.peer, Duration::from_secs(2)).await {
        Ok(p) => p,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("ack probe to {}: {e}", pico.peer),
            };
        }
    };
    tracing::info!(
        "bundle: per-Pico UDP ack from {} after {} ms, flags=0x{:02X}",
        pico.peer,
        ack_started.elapsed().as_millis(),
        ack_packet.flags,
    );
    if ack_packet.flags & ACK_FLAG_LOG_CHUNK_SUPPORTED == 0 {
        return DiagOutcome::UdpUnsupported { peer: pico.peer };
    }
    capture_log_chunks(&socket, pico.peer).await
}

async fn try_capture_vendor_control() -> DiagOutcome {
    use crate::diag_usb::{VendorDiagOutcome, capture_diag_blocking};
    let outcome = match tokio::task::spawn_blocking(capture_diag_blocking).await {
        Ok(o) => o,
        Err(join_err) => {
            return DiagOutcome::VendorTransferFailed {
                step: "spawn",
                bytes_received: 0,
                error: format!("blocking task panicked: {join_err}"),
            };
        }
    };

    match outcome {
        VendorDiagOutcome::Captured { text, lost } => DiagOutcome::Captured {
            source: DiagSource::VendorControl,
            text,
            lost,
        },
        VendorDiagOutcome::Empty => DiagOutcome::Empty {
            source: DiagSource::VendorControl,
        },
        VendorDiagOutcome::NotFound => DiagOutcome::VendorNotFound,
        VendorDiagOutcome::OpenFailed { error } => DiagOutcome::VendorOpenFailed { error },
        VendorDiagOutcome::TransferFailed {
            step,
            bytes_received,
            error,
        } => DiagOutcome::VendorTransferFailed {
            step,
            bytes_received,
            error,
        },
    }
}

async fn try_capture_setup_cdc() -> DiagOutcome {
    tokio::task::spawn_blocking(|| {
        let port = match cdc::find_setup_port() {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("bundle: find_setup_port: {e:#}");
                return DiagOutcome::NoSetupPort;
            }
        };
        tracing::info!("bundle: setup-mode CDC port at {port}");
        let mut pico = match cdc::PicoSetup::open_named(&port) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("bundle: setup-mode CDC open on {port} failed: {e:#}");
                return DiagOutcome::SetupOpenFailed {
                    error: format!("{e:#}"),
                };
            }
        };

        let probe = pico.hello_probe_with_timeout(Duration::from_secs(10));
        if let Err(err) = probe.result.clone() {
            tracing::error!(
                "bundle: HELLO probe failed at step `{}` after {} ms (rx_bytes={}): {}",
                probe.step_reached.as_str(),
                probe.elapsed_ms,
                probe.bytes_received,
                err,
            );
            return DiagOutcome::SetupProbeFailed {
                port: probe.port,
                step: probe.step_reached.as_str(),
                elapsed_ms: probe.elapsed_ms,
                bytes_received: probe.bytes_received,
                rx_first_32_hex: probe.rx_first_32_hex,
                error: err,
            };
        }

        let log_start = Instant::now();
        match pico.get_log_buffer() {
            Ok((text, _lost)) if text.is_empty() => DiagOutcome::Empty {
                source: DiagSource::SetupCdc,
            },
            Ok((text, lost)) => DiagOutcome::Captured {
                source: DiagSource::SetupCdc,
                text,
                lost,
            },
            Err(e) => {
                tracing::error!(
                    "bundle: GET_LOG_BUFFER on {} failed after {} ms: {e:#}",
                    probe.port,
                    log_start.elapsed().as_millis(),
                );
                DiagOutcome::SetupProbeFailed {
                    port: probe.port,
                    step: "get_log_buffer",
                    elapsed_ms: log_start.elapsed().as_millis(),
                    bytes_received: 0,
                    rx_first_32_hex: "n/a".to_string(),
                    error: format!("{e:#}"),
                }
            }
        }
    })
    .await
    .unwrap_or_else(|join_err| DiagOutcome::SetupOpenFailed {
        error: format!("spawn_blocking task failed: {join_err}"),
    })
}

#[cfg(test)]
mod tests;
