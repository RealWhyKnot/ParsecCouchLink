//! `ptd-bridge run` -- the default mode. Reads the Parsec virtual
//! controller via XInput, broadcasts to find the Pico on LAN, then
//! streams state and heartbeats. Survives Pico reboots: on peer staleness
//! it returns to discovery automatically.

use std::time::Duration;

use anyhow::Result;
use tokio::net::UdpSocket;
use tokio::sync::watch;

use crate::{config, discovery, network, protocol, xinput};

pub async fn run() -> Result<()> {
    tracing::info!(
        "ptd-bridge v{} starting in run mode",
        env!("CARGO_PKG_VERSION")
    );

    let cfg = config::load().unwrap_or_default();
    if !cfg.setup_complete {
        tracing::warn!(
            "setup_complete = false in config. If this is a fresh install, \
             run `ptd-bridge setup` first."
        );
    }

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    tracing::info!("UDP socket bound at {}", socket.local_addr()?);

    let (xinput_tx, xinput_rx) = watch::channel(xinput::Snapshot::default());
    xinput::spawn(xinput_tx);

    let (stats_tx, stats_rx) = watch::channel(network::Stats::default());
    spawn_stats_logger(stats_rx);

    let supervisor = supervisor_loop(socket, xinput_rx, stats_tx);
    tokio::select! {
        r = supervisor => r,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl-C received, shutting down");
            Ok(())
        }
    }
}

async fn supervisor_loop(
    socket: UdpSocket,
    xinput_rx: watch::Receiver<xinput::Snapshot>,
    stats_tx: watch::Sender<network::Stats>,
) -> Result<()> {
    loop {
        tracing::info!("entering discovery, broadcasting for a Pico on LAN");
        let (peer, info) = discovery::run(&socket).await?;
        tracing::info!(
            "Pico found at {peer} -- proto v{} fw v{}.{}.{} board 0x{:02X} uid 0x{:08X} uptime {}s",
            info.proto_version,
            info.fw_major,
            info.fw_minor,
            info.fw_patch,
            info.board_type,
            info.unique_id_short,
            info.uptime_seconds,
        );

        if info.proto_version != protocol::PROTO_VERSION {
            anyhow::bail!(
                "wire protocol mismatch: Pico speaks v{}, bridge speaks v{}. \
                 Update whichever is older.",
                info.proto_version,
                protocol::PROTO_VERSION,
            );
        }

        // Persist what we just learned so other subcommands (doctor,
        // bundle) can reference it without rediscovering.
        let mut cfg = config::load().unwrap_or_default();
        cfg.last_pico = Some(config::PicoIdentity {
            unique_id_short: info.unique_id_short,
            board_type: info.board_type,
            fw_major: info.fw_major,
            fw_minor: info.fw_minor,
            fw_patch: info.fw_patch,
            last_ip: Some(peer.ip().to_string()),
            device_name: cfg.last_pico.as_ref().and_then(|p| p.device_name.clone()),
        });
        let _ = config::save(&cfg);

        match network::run(&socket, peer, xinput_rx.clone(), stats_tx.clone()).await {
            network::Exit::PeerLost => {
                tracing::warn!("peer lost, returning to discovery");
                continue;
            }
            network::Exit::Io(e) => {
                tracing::error!("network error: {e}");
                return Err(e.into());
            }
        }
    }
}

fn spawn_stats_logger(mut rx: watch::Receiver<network::Stats>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.tick().await; // discard immediate first tick
        let mut last_sent: u64 = 0;
        loop {
            tick.tick().await;
            let stats = *rx.borrow_and_update();
            let delta = stats.packets_sent.saturating_sub(last_sent);
            last_sent = stats.packets_sent;
            if delta > 0 {
                tracing::info!(
                    "streaming {} pkt/s, last seq 0x{:02X}",
                    delta / 5,
                    stats.last_seq,
                );
            }
        }
    });
}
