//! Steady-state network task. Streams gamepad state to the latched Pico,
//! emits a 60 Hz heartbeat between state changes, and watches for the
//! Pico's 1 Hz proof-of-life heartbeat coming back. If the Pico stops
//! answering, exits PeerLost so the supervisor can re-enter discovery.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::{watch, Notify};
use tokio::time::{interval, Instant};

use crate::protocol::{GamepadState, Packet, FLAG_PARSEC_CONNECTED};
use crate::xinput;

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(16);
const STALENESS_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const PEER_STALE_AFTER: Duration = Duration::from_secs(5);

pub enum Exit {
    PeerLost,
    Io(std::io::Error),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub packets_sent: u64,
    pub last_seq: u8,
}

pub async fn run(
    socket: &UdpSocket,
    peer: SocketAddr,
    mut xinput_rx: watch::Receiver<xinput::Snapshot>,
    stats_tx: watch::Sender<Stats>,
    drop_peer: Option<Arc<Notify>>,
) -> Exit {
    tracing::info!(
        "network: streaming to {peer}; throughput summary every 5 s. Press Ctrl-C to stop."
    );
    let mut seq: u8 = 0;
    let mut packets_sent: u64 = 0;
    let mut heartbeat = interval(HEARTBEAT_INTERVAL);
    let mut staleness = interval(STALENESS_CHECK_INTERVAL);
    let mut last_state = GamepadState::default();
    let mut parsec_connected = false;
    let mut last_slot: Option<u32> = None;
    let mut last_peer_recv = Instant::now();
    let mut buf = [0u8; 64];

    loop {
        tokio::select! {
            change = xinput_rx.changed() => {
                if change.is_err() {
                    // XInput task gone (host shutting down).
                    tracing::info!("network: xinput task exited, returning to discovery");
                    return Exit::PeerLost;
                }
                let snap = *xinput_rx.borrow_and_update();
                if last_slot != snap.slot {
                    match (last_slot, snap.slot) {
                        (None, Some(s)) => tracing::info!(
                            "network: parsec controller bound on XInput slot {s}"
                        ),
                        (Some(s), None) => tracing::info!(
                            "network: parsec controller unbound (was on XInput slot {s})"
                        ),
                        (Some(a), Some(b)) => tracing::info!(
                            "network: parsec controller moved XInput slot {a} -> {b}"
                        ),
                        (None, None) => {}
                    }
                    last_slot = snap.slot;
                }
                last_state = snap.state;
                parsec_connected = snap.slot.is_some();
                let flags = if parsec_connected { FLAG_PARSEC_CONNECTED } else { 0 };
                let pkt = Packet::state(seq, flags, last_state);
                seq = seq.wrapping_add(1);
                if let Err(e) = socket.send_to(&pkt.encode(), peer).await {
                    return Exit::Io(e);
                }
                packets_sent += 1;
                let _ = stats_tx.send(Stats { packets_sent, last_seq: pkt.seq });
                heartbeat.reset();
            }
            _ = heartbeat.tick() => {
                let flags = if parsec_connected { FLAG_PARSEC_CONNECTED } else { 0 };
                let pkt = Packet::heartbeat(seq, flags, last_state);
                seq = seq.wrapping_add(1);
                if let Err(e) = socket.send_to(&pkt.encode(), peer).await {
                    return Exit::Io(e);
                }
                packets_sent += 1;
                let _ = stats_tx.send(Stats { packets_sent, last_seq: pkt.seq });
            }
            r = socket.recv_from(&mut buf) => {
                match r {
                    Ok((n, from)) => {
                        if from != peer {
                            tracing::debug!(
                                "network: dropping {n}-byte pkt from non-peer {from} (latched peer is {peer})"
                            );
                            continue;
                        }
                        match Packet::decode(&buf[..n]) {
                            Ok(_) => {
                                last_peer_recv = Instant::now();
                            }
                            Err(e) => {
                                tracing::debug!("malformed pkt from peer: {e}");
                            }
                        }
                    }
                    Err(e) => return Exit::Io(e),
                }
            }
            _ = staleness.tick() => {
                if last_peer_recv.elapsed() > PEER_STALE_AFTER {
                    tracing::warn!(
                        "peer {peer} stale for over {} s, returning to discovery",
                        PEER_STALE_AFTER.as_secs()
                    );
                    return Exit::PeerLost;
                }
            }
            _ = async {
                match drop_peer.as_ref() {
                    Some(n) => n.notified().await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                tracing::info!("peer {peer} dropped by tunnel command");
                return Exit::PeerLost;
            }
        }
    }
}

