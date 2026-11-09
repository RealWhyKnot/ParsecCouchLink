use super::{SIGNAL_NEUTRALIZE_DURATION, SIGNAL_SEND_INTERVAL, SIGNAL_WAIT_TIMEOUT};
use crate::{cmd_run, net, protocol, udp_auth, xinput};
use anyhow::{Context, Result, bail};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub(super) async fn neutralize_targets(targets: &[cmd_run::PicoTarget]) {
    let Ok(socket) = net::bind_udp("0.0.0.0:0").await else {
        return;
    };
    for target in targets {
        let _ = send_state_burst(
            &socket,
            target.peer,
            protocol::GamepadState::default(),
            0,
            SIGNAL_NEUTRALIZE_DURATION,
        )
        .await;
    }
}

pub(super) async fn verify_signal_roundtrip(
    target: &cmd_run::PicoTarget,
    state: protocol::GamepadState,
) -> Result<u32> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP signal-test socket")?;
    let matched = send_until_xinput_match(&socket, target.peer, state).await;
    let neutralized = send_state_burst(
        &socket,
        target.peer,
        protocol::GamepadState::default(),
        0,
        SIGNAL_NEUTRALIZE_DURATION,
    )
    .await;

    let slot = matched?;
    neutralized?;
    wait_for_slot_state(
        slot,
        protocol::GamepadState::default(),
        Duration::from_secs(2),
    )
    .await?;
    Ok(slot)
}

pub(super) async fn send_until_xinput_match(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    state: protocol::GamepadState,
) -> Result<u32> {
    let started = Instant::now();
    let mut seq = 0u8;
    loop {
        send_state_once(socket, target, seq, state, protocol::FLAG_PARSEC_CONNECTED).await?;
        seq = seq.wrapping_add(1);

        let matches = slots_matching_state(state);
        if matches.len() == 1 {
            return Ok(matches[0]);
        }
        if matches.len() > 1 {
            bail!("multiple XInput slots matched synthetic state: {matches:?}");
        }

        if started.elapsed() >= SIGNAL_WAIT_TIMEOUT {
            bail!(
                "no XInput slot reported synthetic state within {} s; live slots: {}",
                SIGNAL_WAIT_TIMEOUT.as_secs(),
                slot_state_summary()
            );
        }
        tokio::time::sleep(SIGNAL_SEND_INTERVAL).await;
    }
}

pub(super) async fn send_state_burst(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    state: protocol::GamepadState,
    flags: u8,
    duration: Duration,
) -> Result<()> {
    let started = Instant::now();
    let mut seq = 0u8;
    while started.elapsed() < duration {
        send_state_once(socket, target, seq, state, flags).await?;
        seq = seq.wrapping_add(1);
        tokio::time::sleep(SIGNAL_SEND_INTERVAL).await;
    }
    Ok(())
}

pub(super) async fn send_state_once(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    seq: u8,
    state: protocol::GamepadState,
    flags: u8,
) -> Result<()> {
    let packet = udp_auth::seal(target, &protocol::Packet::state(seq, flags, state).encode());
    socket
        .send_to(&packet, target)
        .await
        .with_context(|| format!("sending synthetic state to {target}"))?;
    Ok(())
}

pub(super) async fn wait_for_slot_state(
    slot: u32,
    state: protocol::GamepadState,
    timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    loop {
        if let Some(snapshot) = xinput::read_slot(slot)
            && snapshot.state == state
        {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            bail!(
                "{} did not return to neutral within {} s; live slots: {}",
                xinput::user_slot_label(slot),
                timeout.as_secs(),
                slot_state_summary()
            );
        }
        tokio::time::sleep(SIGNAL_SEND_INTERVAL).await;
    }
}

pub(super) fn slots_matching_state(state: protocol::GamepadState) -> Vec<u32> {
    xinput::connected_slots()
        .into_iter()
        .filter(|slot| slot.state == state)
        .map(|slot| slot.slot)
        .collect()
}

pub(super) fn slot_state_summary() -> String {
    let slots = xinput::connected_slots();
    if slots.is_empty() {
        return "none".to_string();
    }
    slots
        .iter()
        .map(|slot| {
            format!(
                "{} buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
                xinput::user_slot_label(slot.slot),
                slot.state.buttons,
                slot.state.left_trigger,
                slot.state.right_trigger,
                slot.state.left_x,
                slot.state.left_y,
                slot.state.right_x,
                slot.state.right_y
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn lab_signal_state(index: usize) -> protocol::GamepadState {
    let states = [
        protocol::GamepadState {
            buttons: 0x1100,
            left_trigger: 77,
            right_trigger: 133,
            left_x: 12000,
            left_y: -8000,
            right_x: 6000,
            right_y: -4000,
        },
        protocol::GamepadState {
            buttons: 0x2200,
            left_trigger: 33,
            right_trigger: 201,
            left_x: -16000,
            left_y: 7000,
            right_x: -9000,
            right_y: 15000,
        },
        protocol::GamepadState {
            buttons: 0x0043,
            left_trigger: 149,
            right_trigger: 19,
            left_x: 21000,
            left_y: 9000,
            right_x: -12000,
            right_y: -16000,
        },
        protocol::GamepadState {
            buttons: 0x208C,
            left_trigger: 8,
            right_trigger: 240,
            left_x: -23000,
            left_y: -3000,
            right_x: 13000,
            right_y: 4000,
        },
    ];
    states[index % states.len()]
}
