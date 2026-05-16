//! XInput poll task. Detects which of the four XInput slots is live (Parsec
//! assigns it dynamically), polls that slot at 500 Hz, and publishes
//! state-change snapshots on a watch channel.
//!
//! Polling disconnected slots is surprisingly expensive (the XInput
//! implementation walks USB device trees), so we only scan all four slots
//! when no slot is currently active, and we throttle that rescan to once a
//! second. While a slot is active, we poll only that slot.

use std::time::Duration;

use tokio::sync::watch;
use tokio::time::interval;
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE, XUSER_MAX_COUNT};

use crate::protocol::GamepadState;

const POLL_INTERVAL: Duration = Duration::from_millis(2);
const RESCAN_EVERY_N_TICKS: u32 = 500;

const ERROR_SUCCESS: u32 = 0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub slot: Option<u32>,
    pub state: GamepadState,
    /// Mirrors `XINPUT_STATE.dwPacketNumber`. Kept in the snapshot for
    /// future diagnostics (e.g. support-bundle annotations) even though
    /// downstream consumers don't read it today.
    #[allow(dead_code)]
    pub packet_number: u32,
}

pub fn spawn(tx: watch::Sender<Snapshot>) {
    tokio::spawn(async move {
        let mut tick = interval(POLL_INTERVAL);
        let mut active_slot: Option<u32> = None;
        let mut last_packet: u32 = u32::MAX;
        let mut rescan_counter: u32 = RESCAN_EVERY_N_TICKS;

        loop {
            tick.tick().await;

            if let Some(slot) = active_slot {
                let mut st = XINPUT_STATE::default();
                let r = unsafe { XInputGetState(slot, &mut st) };
                if r != ERROR_SUCCESS {
                    tracing::warn!("XInput slot {slot} disconnected");
                    active_slot = None;
                    last_packet = u32::MAX;
                    let _ = tx.send(Snapshot::default());
                    rescan_counter = RESCAN_EVERY_N_TICKS;
                    continue;
                }
                if st.dwPacketNumber == last_packet {
                    continue;
                }
                last_packet = st.dwPacketNumber;
                let _ = tx.send(Snapshot {
                    slot: Some(slot),
                    state: state_from(&st),
                    packet_number: st.dwPacketNumber,
                });
            } else {
                rescan_counter += 1;
                if rescan_counter < RESCAN_EVERY_N_TICKS {
                    continue;
                }
                rescan_counter = 0;
                for s in 0..XUSER_MAX_COUNT {
                    let mut st = XINPUT_STATE::default();
                    let r = unsafe { XInputGetState(s, &mut st) };
                    if r == ERROR_SUCCESS {
                        tracing::info!("XInput controller detected on slot {s}");
                        active_slot = Some(s);
                        last_packet = st.dwPacketNumber;
                        let _ = tx.send(Snapshot {
                            slot: Some(s),
                            state: state_from(&st),
                            packet_number: st.dwPacketNumber,
                        });
                        break;
                    }
                }
            }
        }
    });
}

fn state_from(st: &XINPUT_STATE) -> GamepadState {
    GamepadState {
        buttons: st.Gamepad.wButtons.0,
        left_trigger: st.Gamepad.bLeftTrigger,
        right_trigger: st.Gamepad.bRightTrigger,
        left_x: st.Gamepad.sThumbLX,
        left_y: st.Gamepad.sThumbLY,
        right_x: st.Gamepad.sThumbRX,
        right_y: st.Gamepad.sThumbRY,
    }
}
