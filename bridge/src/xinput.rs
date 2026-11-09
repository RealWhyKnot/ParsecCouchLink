use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState, XUSER_MAX_COUNT};

use crate::protocol::GamepadState;

const ERROR_SUCCESS: u32 = 0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlotSnapshot {
    pub slot: u32,
    pub state: GamepadState,
    pub packet_number: u32,
}

pub(crate) fn user_slot_label(slot: u32) -> String {
    format!("Controller {}", slot + 1)
}

pub(crate) fn read_slot(slot: u32) -> Option<SlotSnapshot> {
    if slot >= XUSER_MAX_COUNT {
        return None;
    }
    let mut st = XINPUT_STATE::default();
    // SAFETY: `st` is a live XINPUT_STATE that the call fills in.
    let r = unsafe { XInputGetState(slot, &raw mut st) };
    if r != ERROR_SUCCESS {
        return None;
    }
    Some(SlotSnapshot {
        slot,
        state: state_from(&st),
        packet_number: st.dwPacketNumber,
    })
}

pub(crate) fn connected_slots() -> Vec<SlotSnapshot> {
    let mut slots = Vec::new();
    for slot in 0..XUSER_MAX_COUNT {
        if let Some(snapshot) = read_slot(slot) {
            slots.push(snapshot);
        }
    }
    slots
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
