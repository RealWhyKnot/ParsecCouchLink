//! Optional run-mode Pico state probe used by support bundles.

use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::time::{interval, MissedTickBehavior};

use crate::{cmd_run, net, pico_cache, protocol};

const PROBE_INTERVAL: Duration = Duration::from_millis(200);

pub async fn query_pico_state(
    pico: &cmd_run::PicoTarget,
    timeout: Duration,
) -> Result<protocol::PicoStateDiag> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP Pico state socket")?;
    let mut seq = 0xC1u8;
    let started = Instant::now();
    let deadline = started + timeout;
    let mut tick = interval(PROBE_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut buf = [0u8; 256];

    tracing::debug!(
        "pico-state: probing {} timeout={}ms",
        pico.short_label(),
        timeout.as_millis()
    );

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let req = protocol::encode_get_pico_state(seq);
                seq = seq.wrapping_add(1);
                socket
                    .send_to(&req, pico.peer)
                    .await
                    .with_context(|| format!("sending Pico state request to {}", pico.peer))?;
            }
            r = socket.recv_from(&mut buf) => {
                let (n, from) = r.context("receiving Pico state reply")?;
                if from.ip() != pico.peer.ip() {
                    tracing::debug!("pico-state: dropped reply from non-target {from}");
                    continue;
                }
                let state = protocol::PicoStateDiag::decode(&buf[..n])
                    .context("decoding Pico state reply")?;
                pico_cache::record(
                    pico_cache::PicoStateSnapshot::from_target("pico-state", pico)
                        .with_pico_state(&state)
                        .with_outcome("pico_state_captured"),
                );
                return Ok(state);
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                bail!(
                    "no Pico state reply from {} within {} ms",
                    pico.peer,
                    timeout.as_millis()
                );
            }
        }
    }
}
