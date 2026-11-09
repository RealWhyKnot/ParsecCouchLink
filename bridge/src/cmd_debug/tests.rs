use super::*;

#[test]
fn debug_action_chosen_keeps_only_requested_actions() {
    let none = DebugAction::chosen(&[(false, DebugAction::Status), (false, DebugAction::ToWifi)]);
    assert_eq!(none, <[DebugAction; 0]>::default());
    let one = DebugAction::chosen(&[(true, DebugAction::Status), (false, DebugAction::ToWifi)]);
    assert_eq!(one, [DebugAction::Status]);
    let two = DebugAction::chosen(&[(true, DebugAction::Status), (true, DebugAction::ToWifi)]);
    assert_eq!(two, [DebugAction::Status, DebugAction::ToWifi]);
}

#[test]
fn board_type_labels_are_user_facing() {
    assert_eq!(board_type_label(protocol::BOARD_PICO_2_W), "Pico 2 W");
    assert_eq!(
        board_type_label(protocol::BOARD_PICO_W_RP2040),
        "Pico W / WH"
    );
    assert_eq!(board_type_label(0xFF), "Pico");
}
