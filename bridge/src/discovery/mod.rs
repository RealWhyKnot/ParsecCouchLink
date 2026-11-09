use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval, sleep};

use crate::protocol::{self, AckInfo, PacketKind, Persona};
use crate::udp_auth;

const BROADCAST_INTERVAL: Duration = Duration::from_secs(1);
const UNICAST_INTERVAL: Duration = Duration::from_millis(500);
const VERSION_QUERY_TIMEOUT: Duration = Duration::from_millis(900);
const VERSION_QUERY_RETRY_INTERVAL: Duration = Duration::from_millis(125);

pub(crate) async fn collect(
    socket: &UdpSocket,
    duration: Duration,
) -> std::io::Result<Vec<DiscoveryReply>> {
    socket.set_broadcast(true)?;
    let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(
        std::net::Ipv4Addr::BROADCAST,
        protocol::PORT,
    ));

    let extra_sockets = bind_per_interface_sockets().await;

    let (rx_tx, mut rx_rx) = mpsc::channel::<(usize, String, usize, SocketAddr, [u8; 64])>(8);
    let mut iface_tasks = Vec::with_capacity(extra_sockets.len());
    for (source_idx, target) in extra_sockets.iter().enumerate() {
        let s = Arc::clone(&target.socket);
        let tx = rx_tx.clone();
        let label = target.label.clone();
        iface_tasks.push(tokio::spawn(async move {
            let mut buf = [0u8; 64];
            loop {
                match s.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        if tx
                            .send((source_idx, label.clone(), n, peer, buf))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        }));
    }
    drop(rx_tx);

    let mut found = BTreeMap::<u32, FoundPico>::new();
    let mut seq: u8 = 0;
    let mut tick = interval(BROADCAST_INTERVAL);
    let mut main_buf = [0u8; 64];
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let bytes = udp_auth::discover_frame(seq);
                seq = seq.wrapping_add(1);
                let _ = socket.send_to(&bytes, broadcast_addr).await;
                for target in &extra_sockets {
                    let _ = target.socket.send_to(&bytes, target.broadcast_addr).await;
                }
            }
            r = socket.recv_from(&mut main_buf) => {
                if let Some((peer, info, persona, flags)) = handle_recv("main", r, &main_buf) {
                    found.entry(info.unique_id_short).or_insert(FoundPico {
                        peer,
                        info,
                        persona,
                        flags,
                        source_idx: None,
                    });
                }
            }
            iface_msg = rx_rx.recv() => {
                let Some((source_idx, label, n, peer, buf)) = iface_msg else { continue };
                if let Some((peer, info, persona, flags)) = handle_recv(&label, Ok((n, peer)), &buf) {
                    found.entry(info.unique_id_short).or_insert(FoundPico {
                        peer,
                        info,
                        persona,
                        flags,
                        source_idx: Some(source_idx),
                    });
                }
            }
            () = &mut deadline => break,
        }
    }

    for t in iface_tasks {
        t.abort();
        let _ = t.await;
    }

    let mut found: Vec<FoundPico> = found.into_values().collect();
    enrich_full_versions(socket, &extra_sockets, &mut found).await;
    Ok(found
        .into_iter()
        .map(|pico| DiscoveryReply {
            peer: pico.peer,
            info: pico.info,
            persona: pico.persona,
            flags: pico.flags,
        })
        .collect())
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscoveryReply {
    pub peer: SocketAddr,
    pub info: AckInfo,
    pub persona: Persona,
    pub flags: u8,
}

struct InterfaceBroadcastSocket {
    socket: Arc<UdpSocket>,
    broadcast_addr: SocketAddr,
    label: String,
}

#[derive(Clone, Copy, Debug)]
struct FoundPico {
    peer: SocketAddr,
    info: AckInfo,
    persona: Persona,
    flags: u8,
    source_idx: Option<usize>,
}

pub(crate) async fn probe_ip(
    socket: &UdpSocket,
    ip: IpAddr,
    duration: Duration,
) -> std::io::Result<Option<DiscoveryReply>> {
    let target = SocketAddr::new(ip, protocol::PORT);
    let mut seq: u8 = 0;
    let mut tick = interval(UNICAST_INTERVAL);
    let mut buf = [0u8; 64];
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let bytes = udp_auth::discover_frame(seq);
                seq = seq.wrapping_add(1);
                socket.send_to(&bytes, target).await?;
            }
            r = socket.recv_from(&mut buf) => {
                let Some((peer, mut info, persona, flags)) = handle_recv("manual-ip", r, &buf) else {
                    continue;
                };
                if peer.ip() == ip {
                    if flags & protocol::ACK_FLAG_FULL_VERSION_SUPPORTED != 0 {
                        let requests = [(peer, 0xF1)];
                        if let Ok(versions) =
                            query_full_versions(socket, &requests, VERSION_QUERY_TIMEOUT).await
                            && let Some(version) = versions.get(&peer) {
                                info.full_version = Some(version.version);
                            }
                    }
                    return Ok(Some(DiscoveryReply {
                        peer,
                        info,
                        persona,
                        flags,
                    }));
                }
                tracing::debug!("discovery(manual-ip): ignoring reply from non-target {peer}");
            }
            () = &mut deadline => return Ok(None),
        }
    }
}

fn handle_recv(
    src_label: &str,
    r: std::io::Result<(usize, SocketAddr)>,
    buf: &[u8; 64],
) -> Option<(SocketAddr, AckInfo, Persona, u8)> {
    match r {
        Ok((n, peer)) => match udp_auth::decode_reply(peer, &buf[..n]) {
            Ok(pkt) => {
                if let PacketKind::Ack(info) = pkt.kind {
                    tracing::trace!("discovery: ack received via {src_label} from {peer}");
                    Some((peer, info, Persona::from_ack_flags(pkt.flags), pkt.flags))
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

async fn enrich_full_versions(
    main_socket: &UdpSocket,
    iface_sockets: &[InterfaceBroadcastSocket],
    found: &mut [FoundPico],
) {
    let mut by_source = BTreeMap::<Option<usize>, Vec<(SocketAddr, u8)>>::new();
    let mut seq = 0xF0;
    for pico in found.iter() {
        if pico.flags & protocol::ACK_FLAG_FULL_VERSION_SUPPORTED != 0 {
            by_source
                .entry(pico.source_idx)
                .or_default()
                .push((pico.peer, seq));
            seq = seq.wrapping_add(1);
        }
    }

    for (source_idx, requests) in by_source {
        let Some(socket) = socket_for_version_query(main_socket, iface_sockets, source_idx) else {
            tracing::debug!("discovery: missing interface socket for version query");
            continue;
        };
        match query_full_versions(socket, &requests, VERSION_QUERY_TIMEOUT).await {
            Ok(versions) => {
                for pico in found.iter_mut() {
                    if let Some(version) = versions.get(&pico.peer) {
                        pico.info.full_version = Some(version.version);
                    }
                }
            }
            Err(e) => tracing::debug!("discovery: version query failed: {e}"),
        }
    }
}

fn socket_for_version_query<'a>(
    main_socket: &'a UdpSocket,
    iface_sockets: &'a [InterfaceBroadcastSocket],
    source_idx: Option<usize>,
) -> Option<&'a UdpSocket> {
    match source_idx {
        None => Some(main_socket),
        Some(idx) => iface_sockets.get(idx).map(|s| s.socket.as_ref()),
    }
}

async fn query_full_versions(
    socket: &UdpSocket,
    requests: &[(SocketAddr, u8)],
    duration: Duration,
) -> std::io::Result<BTreeMap<SocketAddr, protocol::VersionInfo>> {
    let mut pending: BTreeMap<SocketAddr, u8> = requests.iter().copied().collect();
    let mut versions = BTreeMap::new();
    if pending.is_empty() {
        return Ok(versions);
    }

    send_pending_version_requests(socket, &pending).await;
    let mut retry = interval(VERSION_QUERY_RETRY_INTERVAL);
    retry.set_missed_tick_behavior(MissedTickBehavior::Delay);
    retry.tick().await;

    let mut buf = [0u8; 64];
    let deadline = sleep(duration);
    tokio::pin!(deadline);
    loop {
        if pending.is_empty() {
            break;
        }
        tokio::select! {
            _ = retry.tick() => {
                send_pending_version_requests(socket, &pending).await;
            }
            r = socket.recv_from(&mut buf) => {
                let (n, from) = r?;
                let Some(expected_seq) = pending.get(&from).copied() else {
                    continue;
                };
                match protocol::VersionInfo::decode_with_header(&buf[..n]) {
                    Ok((seq, _flags, version)) if seq == expected_seq => {
                        versions.insert(from, version);
                        pending.remove(&from);
                    }
                    Ok((seq, _flags, _version)) => {
                        tracing::trace!(
                            "discovery: ignored version reply from {from} with stale seq={seq} expected={expected_seq}"
                        );
                    }
                    Err(e) => {
                        tracing::trace!("discovery: ignored non-version reply from {from}: {e}");
                    }
                }
            }
            () = &mut deadline => break,
        }
    }
    Ok(versions)
}

async fn send_pending_version_requests(socket: &UdpSocket, pending: &BTreeMap<SocketAddr, u8>) {
    let requests: Vec<(SocketAddr, u8)> = pending.iter().map(|(peer, seq)| (*peer, *seq)).collect();
    for (peer, seq) in requests {
        let req = protocol::encode_get_version(seq);
        if let Err(e) = socket.send_to(&req, peer).await {
            tracing::debug!("discovery: version request to {peer} failed: {e}");
        }
    }
}

#[cfg(windows)]
async fn bind_per_interface_sockets() -> Vec<InterfaceBroadcastSocket> {
    let addrs = enumerate_ipv4_unicast_addresses();
    let mut out = Vec::new();
    for iface in addrs {
        if iface.addr.is_loopback() || iface.addr.is_unspecified() {
            continue;
        }
        let Some(broadcast) = directed_broadcast_addr(iface.addr, iface.prefix_len) else {
            tracing::debug!(
                "discovery: no directed broadcast for {}/{}; skipping",
                iface.addr,
                iface.prefix_len
            );
            continue;
        };
        let bind = SocketAddr::V4(SocketAddrV4::new(iface.addr, 0));
        match crate::net::bind_udp(bind).await {
            Ok(s) => {
                if let Err(e) = s.set_broadcast(true) {
                    tracing::debug!(
                        "discovery: set_broadcast on {} failed: {e}; skipping",
                        iface.addr
                    );
                    continue;
                }
                let local = s
                    .local_addr()
                    .map_or_else(|_| format!("{}:?", iface.addr), |a| a.to_string());
                out.push(InterfaceBroadcastSocket {
                    socket: Arc::new(s),
                    broadcast_addr: SocketAddr::V4(SocketAddrV4::new(broadcast, protocol::PORT)),
                    label: format!("iface={local}->{broadcast}"),
                });
            }
            Err(e) => {
                tracing::debug!("discovery: bind on {} failed: {e}; skipping", iface.addr);
            }
        }
    }
    out
}

#[cfg(not(windows))]
async fn bind_per_interface_sockets() -> Vec<InterfaceBroadcastSocket> {
    Vec::new()
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug)]
struct Ipv4Interface {
    addr: Ipv4Addr,
    prefix_len: u8,
}

#[cfg(windows)]
fn enumerate_ipv4_unicast_addresses() -> Vec<Ipv4Interface> {
    const IF_OPER_STATUS_UP: i32 = 1;
    use windows::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u8> = vec![0; crate::convert::usize_from_u32(size)];
    let mut out = Vec::new();

    for _ in 0..3 {
        // SAFETY: `buf` holds `size` writable bytes and `size` is a live u32 the call may update.
        let rc = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET.0),
                flags,
                None,
                Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &raw mut size,
            )
        };
        if rc == 0 {
            break;
        }
        if rc == 111 {
            buf.resize(crate::convert::usize_from_u32(size), 0);
            continue;
        }
        tracing::debug!("discovery: GetAdaptersAddresses rc={rc}");
        return out;
    }

    let mut adapter = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !adapter.is_null() {
        // SAFETY: GetAdaptersAddresses built this list inside `buf`, which is alive and unchanged here.
        unsafe {
            let a = &*adapter;
            if a.OperStatus.0 != IF_OPER_STATUS_UP {
                adapter = a.Next;
                continue;
            }
            let mut uni = a.FirstUnicastAddress;
            while !uni.is_null() {
                let u = &*uni;
                let sa = u.Address.lpSockaddr;
                if !sa.is_null() && (*sa).sa_family == AF_INET {
                    let sa_in = sa.cast::<SOCKADDR_IN>().read_unaligned();
                    let octets = sa_in.sin_addr.S_un.S_addr.to_ne_bytes();
                    let addr = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
                    out.push(Ipv4Interface {
                        addr,
                        prefix_len: u.OnLinkPrefixLength,
                    });
                }
                uni = u.Next;
            }
            adapter = a.Next;
        }
    }
    out
}

fn directed_broadcast_addr(addr: Ipv4Addr, prefix_len: u8) -> Option<Ipv4Addr> {
    if prefix_len > 30 {
        return None;
    }
    let host_bits = 32u32.checked_sub(u32::from(prefix_len))?;
    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << host_bits
    };
    let ip = u32::from(addr);
    Some(Ipv4Addr::from((ip & mask) | !mask))
}

#[cfg(test)]
mod tests;
