use super::*;

#[test]
fn parse_selectors_accepts_uid_ip_and_board_names() {
    assert_eq!(
        parse_selector("07D37EB6").unwrap(),
        PicoSelector::Uid(0x07D37EB6)
    );
    assert!(matches!(
        parse_selector("192.168.50.4").unwrap(),
        PicoSelector::Ip(_)
    ));
    assert_eq!(
        parse_selector("pico2w").unwrap(),
        PicoSelector::Board(protocol::BOARD_PICO_2_W)
    );
    assert_eq!(
        parse_selector("rp2040").unwrap(),
        PicoSelector::Board(protocol::BOARD_PICO_W_RP2040)
    );
    assert!(parse_selector("not a pico").is_err());
}

#[test]
fn report_counts_step_statuses() {
    let opts = LabOptions {
        all: true,
        picos: Vec::new(),
        scenario: LabScenario::Full,
        cycles: 1,
        power: LabPower::Auto,
        uf2: None,
        json: None,
        no_flash: false,
    };
    let mut report = LabReport::new(&opts);
    report.pass("a", None, "ok", 1);
    report.fail("b", Some(0x1234ABCD), "bad", 2);
    report.skip("c", None, "skip", 3);
    assert_eq!(report.fail_count(), 1);
    assert_eq!(report.steps[1].uid.as_deref(), Some("1234ABCD"));
}

#[test]
fn power_backend_auto_prefers_reset_without_external_config() {
    let opts = LabOptions {
        all: true,
        picos: Vec::new(),
        scenario: LabScenario::PowerCycle,
        cycles: 1,
        power: LabPower::Auto,
        uf2: None,
        json: None,
        no_flash: false,
    };
    let mut report = LabReport::new(&opts);
    let selected = select_power_backend(LabPower::Auto, None, &mut report);
    assert_eq!(selected.kind, SelectedPowerKind::Reset);
    assert_eq!(selected.name(), "reset");
}

#[test]
fn power_backend_accepts_pnp_remove() {
    let opts = LabOptions {
        all: true,
        picos: Vec::new(),
        scenario: LabScenario::PowerCycle,
        cycles: 1,
        power: LabPower::PnpRemove,
        uf2: None,
        json: None,
        no_flash: false,
    };
    let mut report = LabReport::new(&opts);
    let selected = select_power_backend(LabPower::PnpRemove, None, &mut report);
    assert_eq!(selected.kind, SelectedPowerKind::PnpRemove);
    assert_eq!(selected.name(), "pnp-remove");
}

#[test]
fn lab_signal_states_are_distinct() {
    let states: Vec<_> = (0..4).map(lab_signal_state).collect();
    for (idx, state) in states.iter().enumerate() {
        assert_ne!(*state, protocol::GamepadState::default());
        assert!(!states[..idx].contains(state));
    }
}

#[test]
fn usb_serial_prefix_maps_to_short_uid() {
    assert_eq!(uid_from_usb_serial("B67ED307F4C44A3E"), Some(0x07D37EB6));
    assert_eq!(uid_from_usb_serial("E6613852837C242C"), Some(0x523861E6));
    assert_eq!(uid_from_usb_serial("123"), None);
    assert_eq!(uid_from_usb_serial("not-hex-id"), None);
}

#[test]
fn pnp_parser_selects_only_pico_parent_instances() {
    let text = r#"
Instance ID:                USB\VID_2E8A&PID_CAF0\B67ED307F4C44A3E
Instance ID:                USB\VID_2E8A&PID_CAF0&MI_00\8&22cf742d&0&0000
Instance ID:                USB\VID_045E&PID_028E\E6613852837C242C
Instance ID:                USB\VID_0000&PID_0008\8&18b4ea2b&0&3
"#;
    let instances = parse_pico_pnp_instances(text);
    assert_eq!(
        instances,
        vec![
            PnpInstance {
                uid: 0x07D37EB6,
                persona: PnpPersona::Setup,
                instance_id: r"USB\VID_2E8A&PID_CAF0\B67ED307F4C44A3E".to_string(),
            },
            PnpInstance {
                uid: 0x523861E6,
                persona: PnpPersona::Xinput,
                instance_id: r"USB\VID_045E&PID_028E\E6613852837C242C".to_string(),
            },
        ]
    );
}

#[test]
fn validates_only_pico_pnp_instance_ids() {
    assert!(
        validate_pnp_instance_ids(&[r"USB\VID_045E&PID_028E\E6613852837C242C".to_string()]).is_ok()
    );
    assert!(
        validate_pnp_instance_ids(&[r"USB\VID_0000&PID_0008\8&18b4ea2b&0&3".to_string()]).is_err()
    );
}

#[test]
fn pnp_remove_args_remove_the_device_subtree() {
    assert_eq!(
        pnputil_remove_device_args(r"USB\VID_2E8A&PID_CAF0\B67ED307F4C44A3E", false),
        vec![
            "/remove-device".to_string(),
            r"USB\VID_2E8A&PID_CAF0\B67ED307F4C44A3E".to_string(),
            "/subtree".to_string(),
        ]
    );
    assert_eq!(
        pnputil_remove_device_args(r"USB\VID_045E&PID_028E\E6613852837C242C", true),
        vec![
            "/remove-device".to_string(),
            r"USB\VID_045E&PID_028E\E6613852837C242C".to_string(),
            "/subtree".to_string(),
            "/force".to_string(),
        ]
    );
}

#[test]
fn problem_usb_filter_ignores_unrelated_usb_failures() {
    let text = r#"
Instance ID:                USB\VID_0000&PID_0008\8&18b4ea2b&0&3
Instance ID:                USB\VID_2E8A&PID_CAF0&MI_02\7&222E62A5&0&0002
Instance ID:                USB\VID_045E&PID_028E&E6613852837C242C
"#;
    assert_eq!(
        parse_problem_usb_devices(text),
        vec![
            r"Instance ID:                USB\VID_2E8A&PID_CAF0&MI_02\7&222E62A5&0&0002"
                .to_string(),
            r"Instance ID:                USB\VID_045E&PID_028E&E6613852837C242C".to_string(),
        ]
    );
}

#[test]
fn pnputil_failure_text_is_detected_even_with_zero_exit_status() {
    assert!(pnputil_output_failed("Access is denied."));
    assert!(pnputil_output_failed("Failed to restart device."));
    assert!(!pnputil_output_failed("Device restarted successfully."));
}

#[test]
fn command_label_preserves_argv_order() {
    let cmd = vec![
        "hubctl.exe".to_string(),
        "--port".to_string(),
        "2".to_string(),
        "off".to_string(),
    ];
    assert_eq!(command_label(&cmd), "hubctl.exe --port 2 off");
}
