//! `couchlink run` -- the default mode. Reads the Parsec virtual
//! controller via XInput, broadcasts to find the Pico on LAN, then
//! streams state and heartbeats. Survives Pico reboots: on peer staleness
//! it returns to discovery automatically.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::net::UdpSocket;
use tokio::sync::{watch, Notify};

use crate::exec_allowlist::Allowlist;
use crate::telemetry::{
    self, ErrorBody, HeartbeatBody, HostMetadata, OutEvent, RemoteCmd, TelemetryHandle,
};
use crate::{
    cmd_bundle, config, discovery, exec_runner, file_serve, journal, logfile, network, protocol,
    xinput,
};

pub async fn run() -> Result<()> {
    tracing::info!("run: starting, bridge v{}", env!("CARGO_PKG_VERSION"));
    journal!("run", "started bridge v{}", env!("CARGO_PKG_VERSION"));

    let cfg = config::load().unwrap_or_default();
    if !cfg.setup_complete {
        tracing::warn!(
            "setup_complete = false in config. If this is a fresh install, \
             run `couchlink setup` first."
        );
    }

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    tracing::info!("UDP socket bound at {}", socket.local_addr()?);

    let (xinput_tx, xinput_rx) = watch::channel(xinput::Snapshot::default());
    xinput::spawn(xinput_tx);

    let (stats_tx, stats_rx) = watch::channel(network::Stats::default());
    spawn_stats_logger(stats_rx.clone());

    // ---- tunnel telemetry (opt-in via `couchlink tunnel start`) ----
    let drop_peer = Arc::new(Notify::new());
    let shutdown = Arc::new(Notify::new());
    let allowlist = Arc::new(Allowlist::load_or_default());

    let tunnel_active = cfg
        .telemetry
        .as_ref()
        .map(|t| !t.write_token.is_empty() && !t.server.is_empty())
        .unwrap_or(false);

    if tunnel_active {
        if let Some(tcfg) = cfg.telemetry.clone() {
            let meta = HostMetadata {
                board_type: cfg.last_pico.as_ref().map(|p| board_label(p.board_type)),
                firmware_version: cfg
                    .last_pico
                    .as_ref()
                    .map(|p| format!("{}.{}.{}", p.fw_major, p.fw_minor, p.fw_patch)),
                device_uid: cfg
                    .last_pico
                    .as_ref()
                    .map(|p| format!("{:08X}", p.unique_id_short)),
                exec_allowlist: allowlist.entries().map(|s| s.to_string()).collect(),
                file_keys: vec![
                    "config".into(),
                    "state_journal".into(),
                    "pico_diag".into(),
                    "bridge_log".into(),
                ],
            };
            let (handle, cmd_rx) = telemetry::spawn(tcfg.clone(), meta, shutdown.clone());
            telemetry::forward_journal(handle.clone());
            spawn_heartbeat_publisher(handle.clone(), stats_rx.clone());
            spawn_dispatcher(handle.clone(), cmd_rx, allowlist.clone(), drop_peer.clone());
            tracing::info!("tunnel: telemetry active, view URL is {}", tcfg.view_url());
            handle.note(format!("bridge v{} ready", env!("CARGO_PKG_VERSION")));
        }
    } else {
        tracing::info!("tunnel: inactive (no session in config)");
    }

    let supervisor = supervisor_loop(socket, xinput_rx, stats_tx, drop_peer.clone());
    let result = tokio::select! {
        r = supervisor => r,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl-C received, shutting down");
            Ok(())
        }
    };
    shutdown.notify_waiters();
    result
}

async fn supervisor_loop(
    socket: UdpSocket,
    xinput_rx: watch::Receiver<xinput::Snapshot>,
    stats_tx: watch::Sender<network::Stats>,
    drop_peer: Arc<Notify>,
) -> Result<()> {
    loop {
        tracing::info!("run: entering discovery, broadcasting for a Pico on LAN");
        let disc_start = Instant::now();
        // Log once if no Pico has replied after 30 s so the log shows the silence.
        let silence_warn = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            tracing::warn!("run: no Pico discovery reply in 30 s -- still searching");
        });
        let disc_result = discovery::run(&socket).await;
        silence_warn.abort();
        let (peer, info) = disc_result?;
        tracing::info!(
            "run: discovered Pico {} fw v{}.{}.{} uid 0x{:08X} (found after {} s)",
            peer,
            info.fw_major,
            info.fw_minor,
            info.fw_patch,
            info.unique_id_short,
            disc_start.elapsed().as_secs(),
        );
        journal!(
            "run",
            "discovered Pico {peer} fw v{}.{}.{} uid 0x{:08X} after {}s",
            info.fw_major,
            info.fw_minor,
            info.fw_patch,
            info.unique_id_short,
            disc_start.elapsed().as_secs()
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
        if let Err(e) = config::save(&cfg) {
            tracing::warn!(
                "run: could not persist Pico identity after discovery: {e:#}. \
                 Next launch will re-run discovery."
            );
        }

        match network::run(
            &socket,
            peer,
            xinput_rx.clone(),
            stats_tx.clone(),
            Some(drop_peer.clone()),
        )
        .await
        {
            network::Exit::PeerLost => {
                tracing::warn!("peer lost, returning to discovery");
                journal!("run", "peer {peer} lost; returning to discovery");
                continue;
            }
            network::Exit::Io(e) => {
                tracing::error!("network error: {e}");
                journal!("run", "network error against {peer}: {e}");
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

fn spawn_heartbeat_publisher(tele: TelemetryHandle, mut rx: watch::Receiver<network::Stats>) {
    let started = Instant::now();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            let stats = *rx.borrow_and_update();
            // Peer + wifi/rssi info isn't surfaced through the stats channel
            // today; emit the parts we do know. cfg-derived hints (last_ip,
            // wifi flag) come along via the hello frame instead.
            let cfg = config::load().unwrap_or_default();
            let peer = cfg
                .last_pico
                .as_ref()
                .and_then(|p| p.last_ip.clone())
                .map(|ip| format!("{ip}:{}", protocol::PORT));
            tele.publish(OutEvent::Heartbeat(HeartbeatBody {
                uptime_s: started.elapsed().as_secs(),
                peer,
                tx: stats.packets_sent,
                rx: 0,
                wifi: None,
                rssi: None,
                parsec: false,
            }));
        }
    });
}

fn spawn_dispatcher(
    tele: TelemetryHandle,
    mut cmd_rx: tokio::sync::mpsc::Receiver<RemoteCmd>,
    allowlist: Arc<Allowlist>,
    drop_peer: Arc<Notify>,
) {
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            let tele = tele.clone();
            let allowlist = allowlist.clone();
            let drop_peer = drop_peer.clone();
            tokio::spawn(async move {
                dispatch_one(cmd, tele, allowlist, drop_peer).await;
            });
        }
    });
}

async fn dispatch_one(
    cmd: RemoteCmd,
    tele: TelemetryHandle,
    allowlist: Arc<Allowlist>,
    drop_peer: Arc<Notify>,
) {
    match cmd {
        RemoteCmd::Exec { id, argv, cwd } => {
            exec_runner::spawn(id, argv, cwd, allowlist, tele).await;
        }
        RemoteCmd::ReadFile { id, key } => {
            handle_read_file(id, key, tele).await;
        }
        RemoteCmd::Kill { id: _, target_id } => {
            exec_runner::kill(&target_id, &tele);
        }
        RemoteCmd::PullLog { id } => {
            handle_pull_log(id, tele).await;
        }
        RemoteCmd::DropPeer { id } => {
            drop_peer.notify_waiters();
            tele.note(format!("[{id}] peer dropped"));
        }
        RemoteCmd::SetLogFilter { id, directive } => match logfile::set_filter(&directive) {
            Ok(()) => tele.note(format!("[{id}] log filter -> {directive}")),
            Err(e) => tele.publish(OutEvent::Error(ErrorBody {
                id: Some(id),
                message: format!("set_log_filter: {e}"),
            })),
        },
    }
}

async fn handle_read_file(id: String, key: String, tele: TelemetryHandle) {
    let Some(parsed) = file_serve::FileKey::parse(&key) else {
        tele.publish(OutEvent::Error(ErrorBody {
            id: Some(id),
            message: format!("read_file: unknown key '{key}'"),
        }));
        return;
    };
    match file_serve::read_chunks(parsed).await {
        Ok(chunks) => {
            let total = chunks.len() as u32;
            for c in chunks {
                tele.publish(OutEvent::FileChunk(telemetry::FileChunkBody {
                    id: id.clone(),
                    seq: c.seq,
                    b64: c.b64,
                }));
            }
            tele.publish(OutEvent::FileEof(telemetry::FileEofBody {
                id,
                total_chunks: total,
            }));
        }
        Err(e) => {
            tele.publish(OutEvent::Error(ErrorBody {
                id: Some(id),
                message: format!("read_file: {e:#}"),
            }));
        }
    }
}

async fn handle_pull_log(id: String, tele: TelemetryHandle) {
    match cmd_bundle::pull_pico_log_via_udp().await {
        Ok(text) if text.is_empty() => {
            tele.note(format!("[{id}] pull_log: ring empty"));
        }
        Ok(text) => {
            // Emit each line of the log ring as a synthetic journal event so
            // the helper sees it interleaved with their other events.
            let mut count = 0usize;
            for line in text.lines() {
                tele.publish(OutEvent::Journal(telemetry::JournalBody {
                    category: "pico_log".to_string(),
                    message: line.to_string(),
                }));
                count += 1;
                if count >= 256 {
                    tele.note(format!("[{id}] pull_log: truncated at 256 lines"));
                    break;
                }
            }
            tele.note(format!("[{id}] pull_log: {count} lines"));
        }
        Err(e) => {
            tele.publish(OutEvent::Error(ErrorBody {
                id: Some(id),
                message: format!("pull_log: {e}"),
            }));
        }
    }
}

fn board_label(b: u8) -> String {
    match b {
        1 => "pico_w".to_string(),
        2 => "pico2_w".to_string(),
        n => format!("board_{n}"),
    }
}
