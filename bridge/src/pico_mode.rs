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

/// Ask a run-mode Pico to reboot into `persona` with one-shot USB packet
/// capture active from before `tusb_init()`.
pub async fn request_set_usb_capture_persona(
    pico: &cmd_run::PicoTarget,
    persona: protocol::Persona,
) -> Result<()> {
    tracing::info!(
        "pico-mode: request usb-capture persona {} for {}",
        persona.label(),
        pico.short_label(),
    );
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP usb-capture socket")?;
    let mut seq = 0xC0u8;
    for _ in 0..6 {
        let req = protocol::encode_set_usb_capture(seq, persona, true);
        tracing::debug!(
            "pico-mode: send usb-capture seq=0x{:02X} persona={} to {}",
            seq,
            persona.label(),
            pico.peer,
        );
        seq = seq.wrapping_add(1);
        match socket.send_to(&req, pico.peer).await {
            Ok(_) => {}
            Err(e) if net::is_transient(&e) => {
                tracing::debug!(
                    "pico-mode: transient usb-capture send error after reboot start: {e}"
                );
                break;
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("sending usb-capture request to {}", pico.peer))
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}

/// Clear raw USB packet capture for the current boot. This does not reboot or
/// change the persisted persona.
pub async fn request_clear_usb_capture(pico: &cmd_run::PicoTarget) -> Result<()> {
    tracing::info!(
        "pico-mode: request clear usb-capture for {}",
        pico.short_label()
    );
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP clear usb-capture socket")?;
    let mut seq = 0xB0u8;
    for _ in 0..4 {
        let req = protocol::encode_set_usb_capture(seq, pico.persona, false);
        tracing::debug!(
            "pico-mode: send clear usb-capture seq=0x{:02X} to {}",
            seq,
            pico.peer,
        );
        seq = seq.wrapping_add(1);
        match socket.send_to(&req, pico.peer).await {
            Ok(_) => {}
            Err(e) if net::is_transient(&e) => {
                tracing::debug!("pico-mode: transient clear usb-capture send error: {e}");
                break;
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("sending clear usb-capture request to {}", pico.peer))
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Ok(())
}

/// Ask a run-mode Pico to blink its onboard LED for `blink_seconds` so the
/// user can spot the physical board. Returns true when the Pico confirmed
/// the request with an ACK; false means no confirmation arrived, which
/// usually indicates firmware from before the IDENTIFY command existed
/// (there is no spare ACK capability bit to advertise it).
pub async fn request_identify(pico: &cmd_run::PicoTarget, blink_seconds: u8) -> Result<bool> {
    tracing::info!(
        "pico-mode: request identify blink {}s for {}",
        blink_seconds,
        pico.short_label(),
    );
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP identify socket")?;
    let mut seq = 0xA0u8;
    let mut buf = [0u8; 64];
    for _ in 0..6 {
        let req = protocol::encode_identify(seq, blink_seconds);
        seq = seq.wrapping_add(1);
        socket
            .send_to(&req, pico.peer)
            .await
            .with_context(|| format!("sending identify request to {}", pico.peer))?;

        // The firmware replies with a standard ACK. Its ACK sequence
        // counter is independent of ours, so match on the sender and
        // packet kind instead of the sequence number.
        let deadline = tokio::time::sleep(Duration::from_millis(400));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                r = socket.recv_from(&mut buf) => {
                    let Ok((n, from)) = r else { break };
                    if from != pico.peer {
                        continue;
                    }
                    if matches!(
                        protocol::Packet::decode(&buf[..n]),
                        Ok(protocol::Packet { kind: protocol::PacketKind::Ack(_), .. })
                    ) {
                        return Ok(true);
                    }
                }
                _ = &mut deadline => break,
            }
        }
    }
    Ok(false)
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
