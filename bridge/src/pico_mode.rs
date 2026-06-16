//! Shared Pico mode-switch helpers used by the setup and debug flows.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::{cdc, cmd_run, net, protocol};

pub async fn request_reboot_to_setup(pico: &cmd_run::PicoTarget) -> Result<()> {
    tracing::info!(
        "pico-mode: request reboot-to-setup for {}",
        pico.short_label()
    );
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP reboot-to-setup socket")?;
    let mut seq = 0xE0u8;
    for _ in 0..8 {
        let req = protocol::encode_reboot_to_setup(seq);
        tracing::debug!(
            "pico-mode: send reboot-to-setup seq=0x{:02X} to {}",
            seq,
            pico.peer,
        );
        seq = seq.wrapping_add(1);
        socket
            .send_to(&req, pico.peer)
            .await
            .with_context(|| format!("sending reboot-to-setup request to {}", pico.peer))?;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}

/// Ask a run-mode Pico to persist a new output persona and reboot into it.
/// The firmware ignores the request when it's already in the requested
/// persona, so this is safe to send unconditionally. Several datagrams go
/// out to cover UDP loss; a transient send error after the Pico starts
/// rebooting is expected and ignored.
pub async fn request_set_persona(
    pico: &cmd_run::PicoTarget,
    persona: protocol::Persona,
) -> Result<()> {
    tracing::info!(
        "pico-mode: request set-persona {} for {}",
        persona.label(),
        pico.short_label(),
    );
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP set-persona socket")?;
    let mut seq = 0xD0u8;
    for _ in 0..6 {
        let req = protocol::encode_set_persona(seq, persona);
        tracing::debug!(
            "pico-mode: send set-persona seq=0x{:02X} persona={} to {}",
            seq,
            persona.label(),
            pico.peer,
        );
        seq = seq.wrapping_add(1);
        match socket.send_to(&req, pico.peer).await {
            Ok(_) => {}
            Err(e) if net::is_transient(&e) => {
                tracing::debug!(
                    "pico-mode: transient set-persona send error after reboot start: {e}"
                );
                break;
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("sending set-persona request to {}", pico.peer))
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}

pub async fn wait_for_setup_port(timeout: Duration) -> Result<String> {
    let started = Instant::now();
    let deadline = started + timeout;
    let mut next_beat = started + Duration::from_secs(10);
    loop {
        if let Ok(port) = cdc::find_setup_port() {
            return Ok(port);
        }
        let now = Instant::now();
        if now >= deadline {
            return cdc::find_setup_port();
        }
        if now >= next_beat {
            let elapsed = now.duration_since(started).as_secs();
            println!(
                "  ... still waiting for setup-mode USB ({elapsed}s/{})",
                timeout.as_secs()
            );
            next_beat = now + Duration::from_secs(10);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
