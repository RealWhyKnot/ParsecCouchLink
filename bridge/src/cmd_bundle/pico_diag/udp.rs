use super::{DiagOutcome, DiagSource};
use crate::protocol::{self, ACK_FLAG_LOG_CHUNK_SUPPORTED, LogChunk, Packet, PacketKind};
use crate::{config, udp_auth};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

pub(super) async fn try_capture_run_udp() -> DiagOutcome {
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

    let peer_addr = match broadcast_for_ack(&socket, Duration::from_secs(2)).await {
        Ok(addr) => {
            tracing::info!("bundle: broadcast discovery found Pico at {addr}");
            addr
        }
        Err(broadcast_err) => {
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

pub(super) async fn capture_log_chunks(socket: &UdpSocket, peer_addr: SocketAddr) -> DiagOutcome {
    let started = Instant::now();
    if let Err(e) = udp_auth::check_access(peer_addr) {
        return DiagOutcome::UdpProbeFailed {
            peer: peer_addr,
            step: "auth",
            elapsed_ms: 0,
            chunks_received: 0,
            error: format!("{e:#}"),
        };
    }
    let req = udp_auth::seal(peer_addr, &protocol::encode_get_log(0));
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
                    chunks_received: u16::try_from(chunks.len()).unwrap_or(u16::MAX),
                    error: format!("{e}"),
                };
            }
            Err(_) => break,
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

    let lost = chunks.get(&0).map_or(0, |c| c.lost_bytes);
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

pub(super) async fn unicast_for_ack(
    socket: &UdpSocket,
    peer: SocketAddr,
    timeout: Duration,
) -> Result<Packet, String> {
    let req = udp_auth::discover_frame(0);
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
                match udp_auth::decode_reply(from, &buf[..n]) {
                    Ok(pkt) if matches!(pkt.kind, PacketKind::Ack(_)) => return Ok(pkt),
                    Ok(_) => {}
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

pub(super) async fn broadcast_for_ack(
    socket: &UdpSocket,
    timeout: Duration,
) -> Result<SocketAddr, String> {
    let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", protocol::PORT)
        .parse()
        .expect("broadcast addr is constant");
    let req = udp_auth::discover_frame(0);
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
            Ok(Ok((n, from))) => match udp_auth::decode_reply(from, &buf[..n]) {
                Ok(pkt) if matches!(pkt.kind, PacketKind::Ack(_)) => return Ok(from),
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("bundle: discarded non-ack from {from}: {e}");
                }
            },
            Ok(Err(e)) => return Err(format!("recv: {e}")),
            Err(_) => return Err(format!("no ack within {} ms", timeout.as_millis())),
        }
    }
}
