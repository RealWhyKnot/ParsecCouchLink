//! Discovery: broadcasts a type=Discover datagram once a second on the
//! local LAN until the Pico replies with an Ack. Returns the Pico's
//! address and version info to the supervisor, which then hands off to
//! the steady-state network task.
//!
//! Multi-homed PCs are a recurring real-world failure mode: when both
//! Ethernet and Wi-Fi are up, the OS picks one interface for any
//! 255.255.255.255 broadcast, and that pick often goes out the wrong
//! NIC for a Pico that's on the other one. To work around that, we
//! enumerate the host's IPv4 interfaces and -- best effort -- bind one
//! extra UDP socket per interface, broadcast on each, and listen on
//! each via a fan-in channel. The first reply on any socket wins.

use std::collections::BTreeMap;
#[cfg(windows)]
use std::net::Ipv4Addr;
use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::protocol::{self, AckInfo, Packet, PacketKind};

const BROADCAST_INTERVAL: Duration = Duration::from_secs(1);
const UNICAST_INTERVAL: Duration = Duration::from_millis(500);

pub async fn run(socket: &UdpSocket) -> std::io::Result<(SocketAddr, AckInfo)> {
    socket.set_broadcast(true)?;
    let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(
        std::net::Ipv4Addr::BROADCAST,
        protocol::PORT,
    ));

    let extra_sockets: Vec<Arc<UdpSocket>> = bind_per_interface_sockets()
        .await
        .into_iter()
        .map(Arc::new)
        .collect();
    if !extra_sockets.is_empty() {
        let addrs: Vec<String> = extra_sockets
            .iter()
            .filter_map(|s| s.local_addr().ok())
            .map(|a| a.to_string())
            .collect();
        tracing::info!(
            "discovery: broadcasting on main socket + {} interface(s): [{}]",
            extra_sockets.len(),
            addrs.join(", "),
        );
    }

    // Fan-in: each per-interface socket has a task that forwards each
    // received packet through this single channel. The discovery loop
    // selects between (tick, main socket recv, per-iface recv).
    let (rx_tx, mut rx_rx) = mpsc::channel::<(String, usize, SocketAddr, [u8; 64])>(8);
    let mut iface_tasks = Vec::with_capacity(extra_sockets.len());
    for s in &extra_sockets {
        let s = Arc::clone(s);
        let tx = rx_tx.clone();
        let label = s
            .local_addr()
            .ok()
            .map(|a| format!("iface={a}"))
            .unwrap_or_else(|| "iface=?".to_string());
        iface_tasks.push(tokio::spawn(async move {
            let mut buf = [0u8; 64];
            loop {
                match s.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        if tx.send((label.clone(), n, peer, buf)).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("discovery({label}): recv error: {e}");
                        return;
                    }
                }
            }
        }));
    }
    drop(rx_tx); // only the per-iface tasks hold senders now

    let mut seq: u8 = 0;
    let mut tick = interval(BROADCAST_INTERVAL);
    let mut main_buf = [0u8; 64];
    let mut send_fail_warned = false;

    let result = loop {
        tokio::select! {
            _ = tick.tick() => {
                let pkt = Packet::discover(seq);
                seq = seq.wrapping_add(1);
                let bytes = pkt.encode();
                if let Err(e) = socket.send_to(&bytes, broadcast_addr).await {
                    tracing::warn!("discovery: broadcast on main socket failed: {e}");
                }
                let mut any_iface_ok = false;
                for s in &extra_sockets {
                    match s.send_to(&bytes, broadcast_addr).await {
                        Ok(_) => { any_iface_ok = true; }
                        Err(e) => {
                            tracing::debug!(
                                "discovery: broadcast on {:?} failed: {e}",
                                s.local_addr().ok(),
                            );
                        }
                    }
                }
                if !any_iface_ok && !extra_sockets.is_empty() && !send_fail_warned {
                    tracing::warn!(
                        "discovery: every per-interface broadcast send failed. \
                         Windows Firewall is the most likely cause. Allow inbound \
                         UDP/{} with: New-NetFirewallRule -DisplayName couchlink \
                         -Direction Inbound -Protocol UDP -LocalPort {} -Action Allow \
                         (admin required).",
                        protocol::PORT,
                        protocol::PORT,
                    );
                    send_fail_warned = true;
                }
            }
            r = socket.recv_from(&mut main_buf) => {
                if let Some(reply) = handle_recv("main", r, &main_buf) {
                    break Ok(reply);
                }
            }
            iface_msg = rx_rx.recv() => {
                let Some((label, n, peer, buf)) = iface_msg else { continue };
                if let Some(reply) = handle_recv(&label, Ok((n, peer)), &buf) {
                    break Ok(reply);
                }
            }
        }
    };

    // Drop the sockets first so the spawned recv_from loops error out
    // and the tasks exit cleanly. Then abort any stragglers and await.
    drop(extra_sockets);
    for t in iface_tasks {
        t.abort();
        let _ = t.await;
    }
    result
}

pub async fn collect(
    socket: &UdpSocket,
    duration: Duration,
) -> std::io::Result<Vec<(SocketAddr, AckInfo)>> {
    socket.set_broadcast(true)?;
    let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(
        std::net::Ipv4Addr::BROADCAST,
        protocol::PORT,
    ));

    let extra_sockets: Vec<Arc<UdpSocket>> = bind_per_interface_sockets()
        .await
        .into_iter()
        .map(Arc::new)
        .collect();

    let (rx_tx, mut rx_rx) = mpsc::channel::<(String, usize, SocketAddr, [u8; 64])>(8);
    let mut iface_tasks = Vec::with_capacity(extra_sockets.len());
    for s in &extra_sockets {
        let s = Arc::clone(s);
        let tx = rx_tx.clone();
        let label = s
            .local_addr()
            .ok()
            .map(|a| format!("iface={a}"))
            .unwrap_or_else(|| "iface=?".to_string());
        iface_tasks.push(tokio::spawn(async move {
            let mut buf = [0u8; 64];
            loop {
                match s.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        if tx.send((label.clone(), n, peer, buf)).await.is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        }));
    }
    drop(rx_tx);

    let mut found = BTreeMap::<u32, (SocketAddr, AckInfo)>::new();
    let mut seq: u8 = 0;
    let mut tick = interval(BROADCAST_INTERVAL);
    let mut main_buf = [0u8; 64];
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let pkt = Packet::discover(seq);
                seq = seq.wrapping_add(1);
                let bytes = pkt.encode();
                let _ = socket.send_to(&bytes, broadcast_addr).await;
                for s in &extra_sockets {
                    let _ = s.send_to(&bytes, broadcast_addr).await;
                }
            }
            r = socket.recv_from(&mut main_buf) => {
                if let Some((peer, info)) = handle_recv("main", r, &main_buf) {
                    found.entry(info.unique_id_short).or_insert((peer, info));
                }
            }
            iface_msg = rx_rx.recv() => {
                let Some((label, n, peer, buf)) = iface_msg else { continue };
                if let Some((peer, info)) = handle_recv(&label, Ok((n, peer)), &buf) {
                    found.entry(info.unique_id_short).or_insert((peer, info));
                }
            }
            _ = &mut deadline => break,
        }
    }

    drop(extra_sockets);
    for t in iface_tasks {
        t.abort();
        let _ = t.await;
    }

    Ok(found.into_values().collect())
}

pub async fn probe_ip(
    socket: &UdpSocket,
    ip: IpAddr,
    duration: Duration,
) -> std::io::Result<Option<(SocketAddr, AckInfo)>> {
    let target = SocketAddr::new(ip, protocol::PORT);
    let mut seq: u8 = 0;
    let mut tick = interval(UNICAST_INTERVAL);
    let mut buf = [0u8; 64];
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let pkt = Packet::discover(seq);
                seq = seq.wrapping_add(1);
                socket.send_to(&pkt.encode(), target).await?;
            }
            r = socket.recv_from(&mut buf) => {
                let Some((peer, info)) = handle_recv("manual-ip", r, &buf) else {
                    continue;
                };
                if peer.ip() == ip {
                    return Ok(Some((peer, info)));
                }
                tracing::debug!("discovery(manual-ip): ignoring reply from non-target {peer}");
            }
            _ = &mut deadline => return Ok(None),
        }
    }
}

fn handle_recv(
    src_label: &str,
    r: std::io::Result<(usize, SocketAddr)>,
    buf: &[u8; 64],
) -> Option<(SocketAddr, AckInfo)> {
    match r {
        Ok((n, peer)) => match Packet::decode(&buf[..n]) {
            Ok(pkt) => {
                if let PacketKind::Ack(info) = pkt.kind {
                    tracing::trace!("discovery: ack received via {src_label} from {peer}");
                    Some((peer, info))
                } else {
                    tracing::debug!("discovery({src_label}): non-ack packet from {peer}, ignoring");
                    None
                }
            }
            Err(e) => {
                tracing::debug!("discovery({src_label}): dropped malformed pkt from {peer}: {e}");
                None
            }
        },
        Err(e) => {
            tracing::debug!("discovery({src_label}): recv error: {e}");
            None
        }
    }
}

#[cfg(windows)]
async fn bind_per_interface_sockets() -> Vec<UdpSocket> {
    let addrs = enumerate_ipv4_unicast_addresses();
    let mut out = Vec::new();
    for addr in addrs {
        if addr.is_loopback() || addr.is_unspecified() {
            continue;
        }
        let bind = SocketAddr::V4(SocketAddrV4::new(addr, 0));
        match UdpSocket::bind(bind).await {
            Ok(s) => {
                if let Err(e) = s.set_broadcast(true) {
                    tracing::debug!("discovery: set_broadcast on {addr} failed: {e}; skipping");
                    continue;
                }
                out.push(s);
            }
            Err(e) => {
                tracing::debug!("discovery: bind on {addr} failed: {e}; skipping");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Layer;

    use super::*;

    #[test]
    fn ack_receive_trace_is_hidden_at_default_info_level() {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedBufWriter(buf.clone());
        let filter = tracing_subscriber::EnvFilter::new("couchlink=info");
        let layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_writer(writer)
            .with_filter(filter);
        let subscriber = tracing_subscriber::registry().with(layer);

        let packet = Packet::ack(
            1,
            AckInfo {
                proto_version: 1,
                fw_major: 26,
                fw_minor: 5,
                fw_patch: 30,
                board_type: protocol::BOARD_PICO_2_W,
                uptime_seconds: 12,
                unique_id_short: 0x1234ABCD,
            },
        )
        .encode();
        let mut rx_buf = [0u8; 64];
        rx_buf[..packet.len()].copy_from_slice(&packet);
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)), protocol::PORT);

        let reply = tracing::subscriber::with_default(subscriber, || {
            handle_recv("main", Ok((packet.len(), peer)), &rx_buf)
        });

        assert!(reply.is_some());
        let captured = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(
            !captured.contains("ack received"),
            "per-packet discovery ACKs should not print at the default info level: {captured:?}",
        );
    }

    #[derive(Clone)]
    struct SharedBufWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedBufWriter {
        type Writer = SharedBufWriterGuard;
        fn make_writer(&'a self) -> Self::Writer {
            SharedBufWriterGuard(self.0.clone())
        }
    }

    struct SharedBufWriterGuard(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedBufWriterGuard {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
async fn bind_per_interface_sockets() -> Vec<UdpSocket> {
    Vec::new()
}

#[cfg(windows)]
fn enumerate_ipv4_unicast_addresses() -> Vec<Ipv4Addr> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u8> = vec![0; size as usize];
    let mut out = Vec::new();

    // Two-pass: GetAdaptersAddresses returns ERROR_BUFFER_OVERFLOW with
    // the required size on the first call. The Windows docs recommend
    // starting with 15 KB and growing if needed.
    for _ in 0..3 {
        let rc = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        if rc == 0 {
            break;
        }
        if rc == 111 {
            // ERROR_BUFFER_OVERFLOW
            buf.resize(size as usize, 0);
            continue;
        }
        tracing::debug!("discovery: GetAdaptersAddresses rc={rc}");
        return out;
    }

    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        unsafe {
            let a = &*adapter;
            // Skip adapters that aren't operationally up.
            const IF_OPER_STATUS_UP: i32 = 1;
            if (a.OperStatus.0 as i32) != IF_OPER_STATUS_UP {
                adapter = a.Next;
                continue;
            }
            let mut uni = a.FirstUnicastAddress;
            while !uni.is_null() {
                let u = &*uni;
                let sa = u.Address.lpSockaddr;
                if !sa.is_null() && (*sa).sa_family == AF_INET {
                    let sa_in = sa as *const SOCKADDR_IN;
                    let octets = (*sa_in).sin_addr.S_un.S_addr.to_ne_bytes();
                    let addr = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
                    out.push(addr);
                }
                uni = u.Next;
            }
            adapter = a.Next;
        }
    }
    out
}
