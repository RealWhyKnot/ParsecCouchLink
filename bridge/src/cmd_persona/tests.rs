use super::*;

#[test]
fn selected_pico_xinput_release_error_names_instance_ids() {
    let message =
        selected_pico_xinput_release_error(
            &[r"USB\VID_045E&PID_028E\E6613852837C242C".to_string()],
        );

    assert!(message.contains("still exposed as a local XInput device"));
    assert!(message.contains(r"USB\VID_045E&PID_028E\E6613852837C242C"));
    assert!(message.contains("run the Bluetooth command again"));
}

#[test]
fn bluetooth_persona_commands_reselect_live_source_slots() {
    assert_eq!(
        preferred_source_slots_for_persona(Persona::BluetoothHid),
        None
    );
    assert_eq!(
        preferred_source_slots_for_persona(Persona::BluetoothXbox),
        None
    );
    assert_eq!(
        preferred_source_slots_for_persona(Persona::Xinput),
        Some(vec![0, 1, 2, 3])
    );
}
