use clap::CommandFactory;

use super::*;

#[test]
fn lower_level_diagnostic_commands_are_hidden_from_help() {
    let mut command = Cli::command();
    let help = command.render_long_help().to_string();

    assert!(help_lists_command(&help, "bundle"));
    for hidden in [
        "debug-input",
        "doctor",
        "recover",
        "bootsel",
        "debug",
        "test",
        "lab",
        "logs",
    ] {
        assert!(
            !help_lists_command(&help, hidden),
            "{hidden} should stay hidden from top-level help:\n{help}"
        );
    }
}

fn help_lists_command(help: &str, command: &str) -> bool {
    let command_with_space = format!("{command} ");
    help.lines().any(|line| {
        let line = line.trim_start();
        line == command || line.starts_with(&command_with_space)
    })
}

#[test]
fn lab_command_parses_defaults_and_overrides() {
    let cli = Cli::try_parse_from([
        "couchlink",
        "lab",
        "--scenario",
        "power-cycle",
        "--cycles",
        "3",
        "--power",
        "pnp-restart",
        "--pico",
        "07D37EB6",
        "--no-flash",
    ])
    .unwrap();

    match cli.command {
        Some(Command::Lab {
            all,
            picos,
            scenario,
            cycles,
            power,
            no_flash,
            ..
        }) => {
            assert!(!all);
            assert_eq!(picos, vec!["07D37EB6"]);
            assert_eq!(scenario, cmd_lab::LabScenario::PowerCycle);
            assert_eq!(cycles, 3);
            assert_eq!(power, cmd_lab::LabPower::PnpRestart);
            assert!(no_flash);
        }
        other => panic!("expected lab command, got {other:?}"),
    }
}

#[test]
fn lab_command_parses_pnp_remove_power() {
    let cli = Cli::try_parse_from(["couchlink", "lab", "--power", "pnp-remove"]).unwrap();

    match cli.command {
        Some(Command::Lab { power, .. }) => {
            assert_eq!(power, cmd_lab::LabPower::PnpRemove);
        }
        other => panic!("expected lab command, got {other:?}"),
    }
}

#[test]
fn lab_pnp_helper_command_parses_instance_ids() {
    let cli = Cli::try_parse_from([
        "couchlink",
        "lab-pnp-helper",
        "--action",
        "remove-rescan",
        "--hold-seconds",
        "3",
        "--instance-id",
        r"USB\VID_045E&PID_028E\E6613852837C242C",
    ])
    .unwrap();

    match cli.command {
        Some(Command::LabPnpHelper {
            action,
            instance_ids,
            hold_seconds,
            result_file,
        }) => {
            assert_eq!(action, cmd_lab::PnpHelperAction::RemoveRescan);
            assert_eq!(hold_seconds, 3);
            assert_eq!(result_file, None);
            assert_eq!(
                instance_ids,
                vec![r"USB\VID_045E&PID_028E\E6613852837C242C"]
            );
        }
        other => panic!("expected lab pnp helper command, got {other:?}"),
    }
}

#[test]
fn maple_command_parses_target_and_no_stream() {
    let cli =
        Cli::try_parse_from(["couchlink", "maple", "--pico", "07D37EB6", "--no-stream"]).unwrap();

    match cli.command {
        Some(Command::Maple {
            picos,
            all,
            no_stream,
        }) => {
            assert_eq!(picos, vec!["07D37EB6"]);
            assert!(!all);
            assert!(no_stream);
        }
        other => panic!("expected maple command, got {other:?}"),
    }
}

#[test]
fn auto_command_parses_target_all_and_no_stream() {
    let cli = Cli::try_parse_from([
        "couchlink",
        "auto",
        "--pico",
        "07D37EB6",
        "--all",
        "--no-stream",
    ])
    .unwrap();

    match cli.command {
        Some(Command::Auto {
            picos,
            all,
            no_stream,
        }) => {
            assert_eq!(picos, vec!["07D37EB6"]);
            assert!(all);
            assert!(no_stream);
        }
        other => panic!("expected auto command, got {other:?}"),
    }
}

#[test]
fn debug_input_command_parses_target_and_no_stream() {
    let cli = Cli::try_parse_from([
        "couchlink",
        "debug-input",
        "--pico",
        "07D37EB6",
        "--no-stream",
    ])
    .unwrap();

    match cli.command {
        Some(Command::DebugInput {
            picos,
            all,
            no_stream,
        }) => {
            assert_eq!(picos, vec!["07D37EB6"]);
            assert!(!all);
            assert!(no_stream);
        }
        other => panic!("expected debug-input command, got {other:?}"),
    }
}

#[test]
fn xinput_command_parses_target_and_no_stream() {
    let cli =
        Cli::try_parse_from(["couchlink", "xinput", "--pico", "07D37EB6", "--no-stream"]).unwrap();

    match cli.command {
        Some(Command::Xinput {
            picos,
            all,
            no_stream,
        }) => {
            assert_eq!(picos, vec!["07D37EB6"]);
            assert!(!all);
            assert!(no_stream);
        }
        other => panic!("expected xinput command, got {other:?}"),
    }
}

#[test]
fn controller_command_still_parses_as_hidden_alias() {
    let cli = Cli::try_parse_from([
        "couchlink",
        "controller",
        "--pico",
        "07D37EB6",
        "--no-stream",
    ])
    .unwrap();

    match cli.command {
        Some(Command::Controller {
            picos,
            all,
            no_stream,
        }) => {
            assert_eq!(picos, vec!["07D37EB6"]);
            assert!(!all);
            assert!(no_stream);
        }
        other => panic!("expected controller alias command, got {other:?}"),
    }
}

#[test]
fn dinput_command_parses_target_and_no_stream() {
    let cli =
        Cli::try_parse_from(["couchlink", "dinput", "--pico", "07D37EB6", "--no-stream"]).unwrap();

    match cli.command {
        Some(Command::Dinput {
            picos,
            all,
            no_stream,
        }) => {
            assert_eq!(picos, vec!["07D37EB6"]);
            assert!(!all);
            assert!(no_stream);
        }
        other => panic!("expected dinput command, got {other:?}"),
    }
}

#[test]
fn specific_gamepad_persona_commands_parse_target_and_no_stream() {
    for command in [
        "xbox",
        "xbox360",
        "xboxone",
        "ps3",
        "ps4",
        "generic-hid",
        "bluetooth",
        "bluetooth-hid",
        "bluetooth-xbox",
        "bluetooth-playstation",
    ] {
        let cli = Cli::try_parse_from(["couchlink", command, "--pico", "07D37EB6", "--no-stream"])
            .unwrap();
        match (command, cli.command) {
            (
                "xbox",
                Some(Command::Xbox {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "xbox360",
                Some(Command::Xbox360 {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "xboxone",
                Some(Command::Xboxone {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "ps3",
                Some(Command::Ps3 {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "ps4",
                Some(Command::Ps4 {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "generic-hid",
                Some(Command::GenericHid {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "bluetooth" | "bluetooth-hid",
                Some(Command::BluetoothHid {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "bluetooth-xbox",
                Some(Command::BluetoothXbox {
                    picos,
                    all,
                    no_stream,
                }),
            )
            | (
                "bluetooth-playstation",
                Some(Command::BluetoothPlaystation {
                    picos,
                    all,
                    no_stream,
                }),
            ) => {
                assert_eq!(picos, vec!["07D37EB6"]);
                assert!(!all);
                assert!(no_stream);
            }
            other => panic!("unexpected parse result: {other:?}"),
        }
    }
}
