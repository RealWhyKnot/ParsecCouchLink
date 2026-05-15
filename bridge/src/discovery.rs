//! Discovery: broadcasts a type=Discover datagram once a second on the
//! local LAN until the Pico replies with an Ack. Returns the Pico's
//! address and version info to the supervisor, which then hands off to
//! the steady-state network task.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::interval;

use crate::protocol::{self, AckInfo, Packet, PacketKind};

const BROADCAST_INTERVAL: Duration = Duration::from_secs(1);

pub async fn run(socket: &UdpSocket) -> std::io::Result<(SocketAddr, AckInfo)> {
    socket.set_broadcast(true)?;
    let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, protocol::PORT));

    let mut seq: u8 = 0;
    let mut tick = interval(BROADCAST_INTERVAL);
    let mut buf = [0u8; 64];

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let pkt = Packet::discover(seq);
                seq = seq.wrapping_add(1);
                if let Err(e) = socket.send_to(&pkt.encode(), broadcast_addr).await {
                    tracing::warn!("discovery broadcast failed: {e}");
                }
            }
            r = socket.recv_from(&mut buf) => {
                let (n, peer) = r?;
                match Packet::decode(&buf[..n]) {
                    Ok(pkt) => {
                        if let PacketKind::Ack(info) = pkt.kind {
                            return Ok((peer, info));
                        }
                    }
                    Err(e) => {
                        tracing::debug!("discovery dropped malformed pkt from {peer}: {e}");
                    }
                }
            }
        }
    }
}
