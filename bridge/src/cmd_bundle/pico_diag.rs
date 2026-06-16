//! Pico diag-log capture across the three transports (setup-mode CDC,
//! WinUSB vendor control, run-mode UDP) and the `DiagOutcome` model that
//! drives pico-diag.txt.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::protocol::{self, LogChunk, Packet, PacketKind, ACK_FLAG_LOG_CHUNK_SUPPORTED};
use crate::{cdc, cmd_run, config};
use tokio::net::UdpSocket;

use super::usb_enum::{setup_probe_failed_diagnosis, stub_failure};

/// Source the Pico diag log came from (or would have come from, if
/// capture failed). The bundle stub names this so an operator can tell
/// at a glance whether the failure was on the USB-CDC path or the
/// run-mode UDP path.
#[derive(Clone, Debug)]
pub(super) enum DiagSource {
    SetupCdc,
    VendorControl,
    RunUdp { peer: SocketAddr },
}

impl DiagSource {
    /// Short discriminant for the manifest's `pico_diag_source` field.
    fn as_str(&self) -> &'static str {
        match self {
            DiagSource::SetupCdc => "setup-cdc",
            DiagSource::VendorControl => "vendor-control",
            DiagSource::RunUdp { .. } => "run-udp",
        }
    }

    /// Human-readable description for the pico-diag.txt stub header,
    /// including the peer address when known.
    fn describe(&self) -> String {
        match self {
            DiagSource::SetupCdc => "setup-mode USB-CDC".to_string(),
            DiagSource::VendorControl => "USB vendor control transfer".to_string(),
            DiagSource::RunUdp { peer } => format!("run-mode UDP from {peer}"),
        }
    }
}

/// The result of an attempt to capture the firmware's diag-log ring,
/// rich enough that the bundle's pico-diag.txt stub can name the
/// specific step that failed (port enum / port open / HELLO write /
/// HELLO read / GET_LOG over UDP / etc.). Replaces the previous
/// `Option<(String, u32)>` which collapsed every failure mode into
/// the same generic stub.
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

    /// Body of pico-diag.txt for this outcome.
    ///
    /// On the Captured path the body is the firmware diag log itself.
    /// On every failure path the body has a two-section layout: a
    /// `Suggested next step` block leading with the most likely cause
    /// and an ordered list of things to try, followed by a `Diagnostic
    /// details` block with the raw captured fields. The order is
    /// intentional -- an operator reading the file top-down hits an
    /// action before they hit jargon.
    pub(super) fn stub_text(&self) -> String {
        match self {
            DiagOutcome::Captured { text, lost, source } => {
                let prefix = format!("--- captured via {}", source.describe());
                let prefix = if *lost > 0 {
                    format!(
                        "{prefix}; {lost} byte(s) dropped from the ring before this snapshot ---\n",
                    )
                } else {
                    format!("{prefix} ---\n")
                };
                format!("{prefix}{text}")
            }
            DiagOutcome::Empty { source } => stub_failure(
                "Pico answered, but its diag ring was empty.",
                &[
                    "Re-run the failing command and immediately run bundle while \
                     the Pico is still in the same state. If the failure was at \
                     boot, the reboot between the failure and bundle wiped the \
                     in-RAM ring.",
                    "If this is reproducible, attach a bug report -- an \
                     answering-but-empty Pico is unusual.",
                ],
                &[("source", &source.describe())],
            ),
            DiagOutcome::NoSetupPort => stub_failure(
                "No setup-mode Pico found, and no last-known run-mode Pico to \
                 fall back to.",
                &[
                    "Unplug the Pico. Hold BOOTSEL while plugging it back in. \
                     Wait until Windows shows a RPI-RP2 or RP2350 drive in File \
                     Explorer.",
                    "Run `couchlink.exe flash` to copy the matching UF2 onto \
                     the drive.",
                    "Once the Pico reboots into setup mode (it should appear as \
                     a new COM port within ~5 seconds), re-run this bundle.",
                    "If no COM port shows up at all, try a different micro-USB \
                     DATA cable (charge-only cables fail) or a different USB \
                     port on the PC.",
                ],
                &[("looking_for_vid_pid", "0x2E8A:0xCAF0")],
            ),
            DiagOutcome::SetupOpenFailed { error } => stub_failure(
                "Pico is enumerated, but the bridge could not open its COM port.",
                &[
                    "Another application is probably holding the port. Close any \
                     open serial terminals (PuTTY, Tera Term, Arduino Serial \
                     Monitor, screen, minicom) and re-run.",
                    "If no app is open, unplug + replug the Pico and re-run.",
                    "If the error mentions ACCESS_DENIED specifically, a Windows \
                     driver may have failed to bind; check Device Manager for a \
                     yellow exclamation mark on the Pico's entry.",
                ],
                &[("error", error)],
            ),
            DiagOutcome::SetupProbeFailed {
                port,
                step,
                elapsed_ms,
                bytes_received,
                rx_first_32_hex,
                error,
            } => {
                let (root, steps) = setup_probe_failed_diagnosis(step, *bytes_received);
                stub_failure(
                    root,
                    steps,
                    &[
                        ("port", port),
                        ("step", step),
                        ("elapsed_ms", &elapsed_ms.to_string()),
                        ("bytes_received", &bytes_received.to_string()),
                        ("rx_first_32_hex", rx_first_32_hex),
                        ("error", error),
                    ],
                )
            }
            DiagOutcome::NoLastPicoInConfig => stub_failure(
                "No setup-mode Pico found, and no run-mode Pico has ever been \
                 seen by this bridge installation.",
                &[
                    "Run `couchlink setup` to provision a Pico (flash + Wi-Fi).",
                    "Or, if a Pico is already running on your LAN, run \
                     `couchlink bundle` from the bridge PC and choose \
                     `Enter Pico IP manually` from the menu if broadcast \
                     discovery is blocked.",
                ],
                &[],
            ),
            DiagOutcome::VendorNotFound => stub_failure(
                "No Pico with a diag-vendor interface (WinUSB-bound) is \
                 currently enumerated. Either the Pico is in run mode (no \
                 diag interface present) or its firmware predates the \
                 WinUSB diag channel.",
                &[
                    "If the Pico is in run mode, retrieval falls through to \
                     UDP automatically; this stub means UDP also did not \
                     succeed -- see the UDP entries for diagnostics.",
                    "If the Pico is in setup mode but no diag interface is \
                     visible, the firmware predates the diag-vendor \
                     interface. Reflash with the matching couchlink-*.uf2.",
                ],
                &[("looking_for_vid_pid", "0x2E8A:0xCAF0 + vendor interface")],
            ),
            DiagOutcome::VendorOpenFailed { error } => stub_failure(
                "Found a Pico with a diag-vendor interface but could not \
                 claim it via WinUSB.",
                &[
                    "Another process may be holding the diag interface. Close \
                     any running couchlink instances and re-run bundle.",
                    "If Windows shows the diag interface as 'driver not \
                     loaded' in Device Manager, the MS OS 2.0 descriptor \
                     binding may have failed. Unplug and replug the Pico; \
                     Windows re-evaluates WinUSB binding on enumeration.",
                ],
                &[("error", error)],
            ),
            DiagOutcome::VendorTransferFailed {
                step,
                bytes_received,
                error,
            } => stub_failure(
                "Vendor control transfer to retrieve the diag log failed.",
                &[
                    "Re-run bundle. Control transfers occasionally fail under \
                     bus glitches; a retry usually succeeds.",
                    "If the error mentions PIPE or STALL, the firmware did \
                     not recognise the vendor request -- the bridge and \
                     firmware may be on mismatched protocol versions. \
                     Reflash with the matching couchlink-*.uf2.",
                ],
                &[
                    ("step", step),
                    ("bytes_received", &bytes_received.to_string()),
                    ("error", error),
                ],
            ),
            DiagOutcome::UdpDiscoveryFailed { reason } => stub_failure(
                "Bundle tried a run-mode UDP probe but the Pico did not answer.",
                &[
                    "Confirm the Pico is powered on, plugged into the USB4MAPLE \
                     (or equivalent), and within Wi-Fi range of the AP it was \
                     provisioned against.",
                    "If you have changed Wi-Fi networks since setup, the saved \
                     credentials are now stale. Hold BOOTSEL for 3+ seconds \
                     during plug-in to wipe the saved creds, then re-run \
                     `couchlink setup`.",
                    "If you have multiple network adapters, make sure the bridge \
                     is allowed through Windows Firewall on the active profile. \
                     `couchlink bundle` includes the firewall and network snapshot.",
                ],
                &[("error", reason)],
            ),
            DiagOutcome::UdpProbeFailed {
                peer,
                step,
                elapsed_ms,
                chunks_received,
                error,
            } => stub_failure(
                "Pico answered initial discovery but the CMD_GET_LOG exchange \
                 did not complete.",
                &[
                    "The Pico is alive on the LAN -- it answered discovery -- \
                     but either the request did not reach it or its reply did \
                     not reach us. Run `couchlink bundle` again; transient \
                     packet loss is the most likely cause.",
                    "If it persists across multiple bundle attempts, check \
                     whether anything on the network is doing aggressive packet \
                     inspection (some corporate firewalls drop unknown UDP \
                     types).",
                ],
                &[
                    ("peer", &peer.to_string()),
                    ("step", step),
                    ("elapsed_ms", &elapsed_ms.to_string()),
                    ("chunks_received", &chunks_received.to_string()),
                    ("error", error),
                ],
            ),
            DiagOutcome::UdpUnsupported { peer } => stub_failure(
                "Pico is reachable on the LAN but is running pre-LogChunk \
                 firmware.",
                &[
                    "The Pico answered discovery, but its ACK does not advertise \
                     the LogChunk capability bit. The firmware predates the \
                     run-mode diag pull.",
                    "Hold BOOTSEL while plugging the Pico into this PC, then \
                     flash with `couchlink.exe flash`. The new firmware \
                     advertises the bit and the next bundle will UDP-pull diag \
                     automatically.",
                    "If you cannot reflash right now, the bridge log at \
                     %LOCALAPPDATA%\\ParsecCouchLink\\data\\logs has \
                     bridge-side observations that do not depend on the firmware.",
                ],
                &[("peer", &peer.to_string())],
            ),
        }
    }
}

/// Try setup-mode USB-CDC first, then WinUSB vendor control transfer
/// (works even when CDC bulk endpoints are wedged), then run-mode UDP.
/// First successful capture wins; on total failure, return the CDC
/// outcome because it carries the richest diagnostic detail.
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

/// WinUSB vendor-control diag retrieval. Wraps the blocking nusb
/// implementation in `spawn_blocking` (matches `try_capture_setup_cdc`'s
/// shape), translates `VendorDiagOutcome` to `DiagOutcome`.
async fn try_capture_vendor_control() -> DiagOutcome {
    use crate::diag_usb::{capture_diag_blocking, VendorDiagOutcome};
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

/// Setup-mode CDC path. Distinguishes:
///   - find_setup_port() failed -> NoSetupPort
///   - port found, open failed -> SetupOpenFailed
///   - port + open OK, HELLO probe failed -> SetupProbeFailed (with step)
///   - HELLO OK, get_log_buffer failed -> SetupProbeFailed { step: "get_log_buffer" }
///   - get_log_buffer OK, payload empty -> Empty
///   - all OK with text -> Captured
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

        // 10-second deadline (vs. the wizard's 3 s): the bundle is the
        // "something failed; gather everything" path, so we'd rather
        // wait for late-arriving bytes from a slow-booting firmware
        // than declare timeout fast. If the wizard already saw a 3 s
        // timeout, an extra 7 s here often surfaces a delayed RSP that
        // would otherwise be invisible.
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

        // HELLO ok; pull the diag ring.
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

// Run-mode UDP path. Tries broadcast first so a stale last_ip does not
// prevent diag capture; falls back to unicast against last_ip only when
// broadcast finds nothing. Two-second timeout on each leg keeps the
// bundle fast in the common failure case.
async fn try_capture_run_udp() -> DiagOutcome {
    let cfg = config::load().unwrap_or_default();
    let last_ip = cfg.last_pico.as_ref().and_then(|p| p.last_ip.clone());
    if last_ip.is_none() && cfg.last_pico.is_none() {
        tracing::info!("bundle: no last_pico in config; UDP probe not attempted");
        return DiagOutcome::NoLastPicoInConfig;
    }

    let socket = match crate::net::bind_udp("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("bind: {e}"),
            };
        }
    };
    if let Err(e) = socket.set_broadcast(true) {
        tracing::warn!("bundle: set_broadcast failed: {e} -- broadcast leg skipped");
    }

    // Step 1: short broadcast discovery (2 s).
    let peer_addr = match broadcast_for_ack(&socket, Duration::from_secs(2)).await {
        Ok(addr) => {
            tracing::info!("bundle: broadcast discovery found Pico at {addr}");
            addr
        }
        Err(broadcast_err) => {
            // Broadcast found nothing. Try unicast against last_ip if we have one.
            let Some(last_ip) = last_ip else {
                tracing::info!("bundle: broadcast found nothing and no last_ip; UDP probe done");
                return DiagOutcome::NoLastPicoInConfig;
            };
            let peer: SocketAddr = match format!("{last_ip}:{}", protocol::PORT).parse() {
                Ok(a) => a,
                Err(e) => {
                    return DiagOutcome::UdpDiscoveryFailed {
                        reason: format!("config last_ip `{last_ip}` did not parse: {e}"),
                    };
                }
            };
            tracing::info!(
                "bundle: broadcast found nothing ({broadcast_err}); \
                 trying unicast to last known IP {peer}"
            );
            match unicast_for_ack(&socket, peer, Duration::from_secs(2)).await {
                Ok(pkt) => {
                    tracing::info!(
                        "bundle: broadcast found nothing; reaching last known IP {peer} \
                         flags=0x{:02X}",
                        pkt.flags,
                    );
                    peer
                }
                Err(e) => {
                    return DiagOutcome::UdpDiscoveryFailed {
                        reason: format!("broadcast: {broadcast_err}; unicast to {peer}: {e}"),
                    };
                }
            }
        }
    };

    // Step 2: read the capability flag from the peer we found.
    let ack_started = Instant::now();
    let ack_packet = match unicast_for_ack(&socket, peer_addr, Duration::from_secs(2)).await {
        Ok(p) => p,
        Err(e) => {
            return DiagOutcome::UdpDiscoveryFailed {
                reason: format!("ack probe: {e}"),
            };
        }
    };
    tracing::info!(
        "bundle: UDP ack from {peer_addr} after {} ms, flags=0x{:02X}",
        ack_started.elapsed().as_millis(),
        ack_packet.flags,
    );

    if ack_packet.flags & ACK_FLAG_LOG_CHUNK_SUPPORTED == 0 {
        return DiagOutcome::UdpUnsupported { peer: peer_addr };
    }

    capture_log_chunks(&socket, peer_addr).await
}

async fn capture_log_chunks(socket: &UdpSocket, peer_addr: SocketAddr) -> DiagOutcome {
    // Send GET_LOG, collect chunks until LAST_CHUNK or timeout.
    let started = Instant::now();
    let req = protocol::encode_get_log(0);
    if let Err(e) = socket.send_to(&req, peer_addr).await {
        return DiagOutcome::UdpProbeFailed {
            peer: peer_addr,
            step: "send_get_log",
            elapsed_ms: started.elapsed().as_millis(),
            chunks_received: 0,
            error: format!("{e}"),
        };
    }

    let mut chunks: BTreeMap<u8, LogChunk> = BTreeMap::new();
    let mut got_last = false;
    let mut buf = [0u8; 1024];
    // Overall deadline gives the firmware time to drain the ring. With 64
    // chunks of 256 bytes each, even a slow per-chunk cadence completes
    // well inside this budget.
    let deadline = started + Duration::from_millis(4000);
    while !got_last {
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(d) if !d.is_zero() => d,
            _ => break,
        };
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if from != peer_addr {
                    tracing::debug!("bundle: UDP probe dropped pkt from {from}");
                    continue;
                }
                match LogChunk::decode(&buf[..n]) {
                    Ok(chunk) => {
                        tracing::debug!(
                            "bundle: log chunk idx={} len={} last={}",
                            chunk.chunk_index,
                            chunk.payload.len(),
                            chunk.is_last(),
                        );
                        if chunk.is_last() {
                            got_last = true;
                        }
                        chunks.insert(chunk.chunk_index, chunk);
                    }
                    Err(e) => {
                        tracing::debug!("bundle: UDP probe garbled chunk from {from}: {e}");
                    }
                }
            }
            Ok(Err(e)) => {
                return DiagOutcome::UdpProbeFailed {
                    peer: peer_addr,
                    step: "recv_chunk",
                    elapsed_ms: started.elapsed().as_millis(),
                    chunks_received: chunks.len() as u16,
                    error: format!("{e}"),
                };
            }
            Err(_) => break, // overall timeout
        }
    }

    if chunks.is_empty() {
        return DiagOutcome::UdpProbeFailed {
            peer: peer_addr,
            step: "wait_for_chunks",
            elapsed_ms: started.elapsed().as_millis(),
            chunks_received: 0,
            error: "no LogChunk datagrams received before the 4000 ms deadline".to_string(),
        };
    }

    let lost = chunks.get(&0).map(|c| c.lost_bytes).unwrap_or(0);
    let mut text_bytes: Vec<u8> = Vec::new();
    for c in chunks.values() {
        text_bytes.extend_from_slice(&c.payload);
    }
    let text = String::from_utf8_lossy(&text_bytes).into_owned();
    if text.is_empty() {
        DiagOutcome::Empty {
            source: DiagSource::RunUdp { peer: peer_addr },
        }
    } else {
        DiagOutcome::Captured {
            source: DiagSource::RunUdp { peer: peer_addr },
            text,
            lost,
        }
    }
}

async fn unicast_for_ack(
    socket: &UdpSocket,
    peer: SocketAddr,
    timeout: Duration,
) -> Result<Packet, String> {
    let req = Packet::discover(0).encode();
    socket
        .send_to(&req, peer)
        .await
        .map_err(|e| format!("send: {e}"))?;
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| format!("no ack within {} ms", timeout.as_millis()))?;
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if from != peer {
                    continue;
                }
                match Packet::decode(&buf[..n]) {
                    Ok(pkt) if matches!(pkt.kind, PacketKind::Ack(_)) => return Ok(pkt),
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::debug!("bundle: discarded non-ack from {from}: {e}");
                    }
                }
            }
            Ok(Err(e)) => return Err(format!("recv: {e}")),
            Err(_) => {
                return Err(format!("no ack within {} ms", timeout.as_millis()));
            }
        }
    }
}

/// Broadcast a Discover and return the address of the first Pico that answers.
/// Returns `Err(reason)` if no ack arrives within `timeout`.
async fn broadcast_for_ack(socket: &UdpSocket, timeout: Duration) -> Result<SocketAddr, String> {
    let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", protocol::PORT)
        .parse()
        .expect("broadcast addr is constant");
    let req = Packet::discover(0).encode();
    socket
        .send_to(&req, broadcast_addr)
        .await
        .map_err(|e| format!("send: {e}"))?;
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| format!("no ack within {} ms", timeout.as_millis()))?;
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => match Packet::decode(&buf[..n]) {
                Ok(pkt) if matches!(pkt.kind, PacketKind::Ack(_)) => return Ok(from),
                Ok(_) => continue,
                Err(e) => {
                    tracing::debug!("bundle: discarded non-ack from {from}: {e}");
                }
            },
            Ok(Err(e)) => return Err(format!("recv: {e}")),
            Err(_) => return Err(format!("no ack within {} ms", timeout.as_millis())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer() -> SocketAddr {
        "10.0.0.24:4242".parse().unwrap()
    }

    /// Every variant has a distinct discriminant string. These strings
    /// ship in manifest.json and are likely to be grepped on by humans
    /// or future tooling, so stability matters.
    #[test]
    fn discriminant_strings_are_distinct() {
        let variants = [
            DiagOutcome::Captured {
                source: DiagSource::SetupCdc,
                text: "".into(),
                lost: 0,
            },
            DiagOutcome::Empty {
                source: DiagSource::SetupCdc,
            },
            DiagOutcome::NoSetupPort,
            DiagOutcome::SetupOpenFailed { error: "x".into() },
            DiagOutcome::SetupProbeFailed {
                port: "COM3".into(),
                step: "read",
                elapsed_ms: 0,
                bytes_received: 0,
                rx_first_32_hex: "none".into(),
                error: "x".into(),
            },
            DiagOutcome::NoLastPicoInConfig,
            DiagOutcome::UdpDiscoveryFailed { reason: "x".into() },
            DiagOutcome::UdpProbeFailed {
                peer: make_peer(),
                step: "send_get_log",
                elapsed_ms: 0,
                chunks_received: 0,
                error: "x".into(),
            },
            DiagOutcome::UdpUnsupported { peer: make_peer() },
        ];
        let mut seen = std::collections::HashSet::new();
        for v in &variants {
            let d = v.discriminant_str();
            assert!(seen.insert(d), "duplicate discriminant: {d}");
        }
        assert_eq!(seen.len(), variants.len());
    }

    #[test]
    fn captured_stub_includes_text_and_source() {
        let out = DiagOutcome::Captured {
            source: DiagSource::SetupCdc,
            text: "BOOT: hello".into(),
            lost: 0,
        };
        let stub = out.stub_text();
        assert!(
            stub.contains("setup-mode USB-CDC"),
            "missing source: {stub}"
        );
        assert!(stub.contains("BOOT: hello"), "missing text: {stub}");
    }

    #[test]
    fn captured_stub_flags_lost_bytes() {
        let out = DiagOutcome::Captured {
            source: DiagSource::RunUdp { peer: make_peer() },
            text: "tail-of-ring".into(),
            lost: 1234,
        };
        let stub = out.stub_text();
        assert!(
            stub.contains("1234 byte(s) dropped"),
            "missing lost: {stub}"
        );
        assert!(stub.contains("10.0.0.24:4242"), "missing peer: {stub}");
    }

    #[test]
    fn setup_probe_failed_names_step_and_bytes() {
        let out = DiagOutcome::SetupProbeFailed {
            port: "COM3".into(),
            step: "read",
            elapsed_ms: 3012,
            bytes_received: 0,
            rx_first_32_hex: "none".into(),
            error: "timed out".into(),
        };
        let stub = out.stub_text();
        // Lead section is the operator-facing "Suggested next step";
        // the captured fields appear in the trailing Diagnostic block.
        assert!(
            stub.contains("=== Suggested next step ==="),
            "no header: {stub}"
        );
        assert!(stub.contains("Try this (in order):"), "no try list: {stub}");
        assert!(
            stub.contains("=== Diagnostic details ==="),
            "no detail block: {stub}"
        );
        assert!(stub.contains("port: COM3"), "missing port field: {stub}");
        assert!(stub.contains("step: read"), "missing step field: {stub}");
        assert!(
            stub.contains("elapsed_ms: 3012"),
            "missing elapsed field: {stub}"
        );
        assert!(
            stub.contains("bytes_received: 0"),
            "missing bytes_received field: {stub}"
        );
        // The read+0 case is the most common reproduction shape and should
        // lead with a fault-during-init story.
        assert!(
            stub.contains("did not write a single byte"),
            "missing read+0 lead: {stub}"
        );
    }

    #[test]
    fn udp_unsupported_names_peer() {
        let out = DiagOutcome::UdpUnsupported { peer: make_peer() };
        let stub = out.stub_text();
        assert!(stub.contains("10.0.0.24:4242"));
        // soft_wrap can break "LogChunk capability bit" across a line,
        // so check for the unbreakable token only.
        assert!(
            stub.contains("LogChunk"),
            "missing capability mention: {stub}"
        );
        assert!(stub.contains("peer: 10.0.0.24:4242"));
    }

    #[test]
    fn udp_probe_failed_names_step_and_count() {
        let out = DiagOutcome::UdpProbeFailed {
            peer: make_peer(),
            step: "recv_chunk",
            elapsed_ms: 1500,
            chunks_received: 3,
            error: "lost peer".into(),
        };
        let stub = out.stub_text();
        assert!(stub.contains("10.0.0.24:4242"));
        assert!(stub.contains("step: recv_chunk"));
        assert!(stub.contains("chunks_received: 3"));
    }

    /// `source_str` returns the manifest-facing source for captured/empty
    /// outcomes only -- it is None for every failure variant.
    #[test]
    fn source_str_only_set_when_reachable() {
        assert_eq!(
            DiagOutcome::Captured {
                source: DiagSource::SetupCdc,
                text: "".into(),
                lost: 0,
            }
            .source_str(),
            Some("setup-cdc"),
        );
        assert_eq!(
            DiagOutcome::Empty {
                source: DiagSource::RunUdp { peer: make_peer() },
            }
            .source_str(),
            Some("run-udp"),
        );
        assert!(DiagOutcome::NoSetupPort.source_str().is_none());
        assert!(DiagOutcome::UdpUnsupported { peer: make_peer() }
            .source_str()
            .is_none());
    }

    #[test]
    fn failed_diag_selection_surfaces_udp_last_known_failure() {
        let selected = choose_failed_diag_outcome(
            DiagOutcome::NoSetupPort,
            DiagOutcome::VendorNotFound,
            DiagOutcome::UdpDiscoveryFailed {
                reason: "broadcast: no ack; unicast: no ack".into(),
            },
        );

        assert_eq!(selected.discriminant_str(), "udp_discovery_failed");
        let stub = selected.stub_text();
        assert!(
            stub.contains("run-mode UDP probe"),
            "wrong stub selected: {stub}"
        );
    }

    #[test]
    fn failed_diag_selection_keeps_setup_probe_detail() {
        let selected = choose_failed_diag_outcome(
            DiagOutcome::SetupProbeFailed {
                port: "COM3".into(),
                step: "read",
                elapsed_ms: 3012,
                bytes_received: 0,
                rx_first_32_hex: "none".into(),
                error: "timed out".into(),
            },
            DiagOutcome::VendorNotFound,
            DiagOutcome::UdpDiscoveryFailed {
                reason: "no ack".into(),
            },
        );

        assert_eq!(selected.discriminant_str(), "setup_probe_failed");
        assert!(selected.stub_text().contains("port: COM3"));
    }

    #[test]
    fn failed_diag_selection_distinguishes_no_known_run_mode_pico() {
        let selected = choose_failed_diag_outcome(
            DiagOutcome::NoSetupPort,
            DiagOutcome::VendorNotFound,
            DiagOutcome::NoLastPicoInConfig,
        );

        assert_eq!(selected.discriminant_str(), "no_last_pico_in_config");
    }
}
