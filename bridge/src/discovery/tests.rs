use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use tracing_subscriber::Layer;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

use super::*;
use crate::protocol::Packet;

#[test]
fn ack_receive_trace_is_hidden_at_default_info_level() {
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let writer =
        SharedBufWriter(std::sync::Arc::<std::sync::Mutex<std::vec::Vec<u8>>>::clone(&buf));
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
            unique_id_short: 0x1234_ABCD,
            full_version: None,
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

#[test]
fn directed_broadcast_uses_interface_prefix() {
    assert_eq!(
        directed_broadcast_addr(Ipv4Addr::new(192, 168, 50, 100), 24),
        Some(Ipv4Addr::new(192, 168, 50, 255))
    );
    assert_eq!(
        directed_broadcast_addr(Ipv4Addr::new(172, 16, 5, 20), 20),
        Some(Ipv4Addr::new(172, 16, 15, 255))
    );
    assert_eq!(
        directed_broadcast_addr(Ipv4Addr::new(10, 73, 20, 2), 32),
        None
    );
}

#[tokio::test]
async fn version_query_retries_and_requires_matching_seq() {
    let bridge = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let pico = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let peer = pico.local_addr().unwrap();
    let expected_seq: u8 = 0x42;

    let responder = tokio::spawn(async move {
        let mut buf = [0u8; 64];

        let (_n, _from) = pico.recv_from(&mut buf).await.unwrap();

        let (_n, from) = pico.recv_from(&mut buf).await.unwrap();
        let wrong_seq = protocol::VersionInfo {
            version: crate::firmware_version::FirmwareVersion::Release {
                year: 2026,
                month: 6,
                day: 15,
                revision: Some(1),
                suffix: Some(*b"4204"),
            },
        }
        .encode(expected_seq.wrapping_add(1), 0);
        pico.send_to(&wrong_seq, from).await.unwrap();

        let (_n, from) = pico.recv_from(&mut buf).await.unwrap();
        let right_seq = protocol::VersionInfo {
            version: crate::firmware_version::FirmwareVersion::Release {
                year: 2026,
                month: 6,
                day: 15,
                revision: Some(1),
                suffix: Some(*b"4204"),
            },
        }
        .encode(expected_seq, 0);
        pico.send_to(&right_seq, from).await.unwrap();
    });

    let got = query_full_versions(&bridge, &[(peer, expected_seq)], Duration::from_millis(700))
        .await
        .unwrap();
    responder.await.unwrap();

    assert_eq!(
        got.get(&peer).map(|v| v.version.to_string()),
        Some("2026.6.15.1-4204".to_string())
    );
}

#[derive(Clone)]
struct SharedBufWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for SharedBufWriter {
    type Writer = SharedBufWriterGuard;
    fn make_writer(&'a self) -> Self::Writer {
        SharedBufWriterGuard(std::sync::Arc::<std::sync::Mutex<std::vec::Vec<u8>>>::clone(&self.0))
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
