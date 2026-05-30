//! Shared Pico mode-switch helpers used by the setup and debug flows.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::net::UdpSocket;

use crate::{cdc, cmd_run, protocol};

pub async fn request_reboot_to_setup(pico: &cmd_run::PicoTarget) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("binding UDP reboot-to-setup socket")?;
    let mut seq = 0xE0u8;
    for _ in 0..8 {
        let req = protocol::encode_reboot_to_setup(seq);
        seq = seq.wrapping_add(1);
        socket
            .send_to(&req, pico.peer)
            .await
            .with_context(|| format!("sending reboot-to-setup request to {}", pico.peer))?;
        tokio::time::sleep(Duration::from_millis(250)).await;
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
