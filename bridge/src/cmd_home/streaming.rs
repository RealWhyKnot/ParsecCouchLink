use super::cards::titled;
use super::{InputModeChoice, menu_item};
use crate::tui::{confirm, select};
use crate::{cmd_auto, cmd_persona, cmd_run, config, pico_mode, protocol, xinput};
use anyhow::Result;

pub(super) fn print_xinput_sources() {
    outln!();
    outln!("Source controllers Windows can currently see:");
    let slots = xinput::connected_slots();
    if slots.is_empty() {
        outln!("  No XInput controllers are connected yet.");
        outln!(
            "  You can still route Controller 1-4; CouchLink will start sending neutral state until that slot appears."
        );
    } else {
        for slot in slots {
            outln!(
                "  {} live  buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
                xinput::user_slot_label(slot.slot),
                slot.state.buttons,
                slot.state.left_trigger,
                slot.state.right_trigger,
                slot.state.left_x,
                slot.state.left_y,
                slot.state.right_x,
                slot.state.right_y,
            );
        }
    }
    outln!("  USB-output modes plug the Pico into the console adapter.");
    outln!("  Bluetooth mode keeps the Pico plugged into this PC for USB input.");
}

pub(super) async fn route_one(picos: Vec<cmd_run::PicoTarget>) -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    let pico_items: Vec<String> = picos
        .iter()
        .map(|p| titled(cfg.nickname_for(p.info.unique_id_short), p.detail_label()))
        .collect();
    let pico_index = select("Which Pico should receive the controller?", &pico_items, 0).await?;
    let pico = picos[pico_index].clone();
    let mode = choose_input_mode().await?;
    if mode.bluetooth_persona().is_some() {
        let routes = vec![cmd_run::StreamRoute {
            source_slot: 0,
            pico,
        }];
        let routes = prepare_routes_for_input_mode(routes, mode).await?;
        let routes = choose_bluetooth_source_slots(routes).await?;
        return start_stream(routes, true).await;
    }

    let source_slot =
        choose_source_slot("Which Windows controller should feed that Pico?", None).await?;
    let routes = vec![cmd_run::StreamRoute { source_slot, pico }];
    stream_with_mode(routes, mode, true).await
}

pub(super) async fn choose_source_slot(prompt: &str, default_slot: Option<u32>) -> Result<u32> {
    let connected = xinput::connected_slots();
    let items = vec![
        slot_item(0, &connected),
        slot_item(1, &connected),
        slot_item(2, &connected),
        slot_item(3, &connected),
    ];
    let default = crate::convert::usize_from_u32(preferred_source_slot(default_slot, &connected));
    Ok(u32::try_from(select(prompt, &items, default).await?)?)
}

pub(super) fn preferred_source_slot(
    default_slot: Option<u32>,
    connected: &[xinput::SlotSnapshot],
) -> u32 {
    let live_slots: Vec<u32> = connected
        .iter()
        .map(|snapshot| snapshot.slot)
        .filter(|slot| *slot < 4)
        .collect();
    if let Some(slot) = default_slot.filter(|slot| live_slots.contains(slot)) {
        return slot;
    }
    if live_slots.len() == 1 {
        return live_slots[0];
    }
    default_slot.unwrap_or(0).min(3)
}

pub(super) fn slot_item(slot: u32, connected: &[xinput::SlotSnapshot]) -> String {
    if connected.iter().any(|snapshot| snapshot.slot == slot) {
        format!("{} (live)", xinput::user_slot_label(slot))
    } else {
        format!("{} (waiting)", xinput::user_slot_label(slot))
    }
}

pub(super) async fn stream(routes: Vec<cmd_run::StreamRoute>, save: bool) -> Result<()> {
    let mode = choose_input_mode().await?;
    stream_with_mode(routes, mode, save).await
}

pub(super) async fn stream_with_mode(
    routes: Vec<cmd_run::StreamRoute>,
    mode: InputModeChoice,
    save: bool,
) -> Result<()> {
    let routes = prepare_routes_for_input_mode(routes, mode).await?;
    let routes = if mode.bluetooth_persona().is_some() {
        choose_bluetooth_source_slots(routes).await?
    } else {
        routes
    };
    start_stream(routes, save).await
}

pub(super) async fn start_stream(routes: Vec<cmd_run::StreamRoute>, save: bool) -> Result<()> {
    outln!();
    outln!("Ready to stream:");
    for route in &routes {
        outln!("  {}", route.label());
    }
    if !confirm("Start streaming now?", true).await? {
        return Ok(());
    }
    cmd_run::stream_routes(
        routes,
        cmd_run::StreamOptions {
            status_seconds: 2,
            quiet: false,
            save_routes: save,
        },
    )
    .await
}

pub(super) async fn choose_bluetooth_source_slots(
    mut routes: Vec<cmd_run::StreamRoute>,
) -> Result<Vec<cmd_run::StreamRoute>> {
    let targets: Vec<_> = routes.iter().map(|route| route.pico.clone()).collect();
    cmd_persona::wait_for_bluetooth_xinput_release(&targets, false).await?;

    print_xinput_sources();
    if routes.len() == 1 {
        routes[0].source_slot = choose_source_slot(
            "Which Windows controller should feed that Bluetooth Pico?",
            Some(routes[0].source_slot),
        )
        .await?;
        return Ok(routes);
    }

    for route in &mut routes {
        route.source_slot = choose_source_slot(
            &format!(
                "Which Windows controller should feed {}?",
                route.pico.short_label()
            ),
            Some(route.source_slot),
        )
        .await?;
    }
    Ok(routes)
}

pub(super) async fn choose_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item(
            "Auto",
            "try gamepad USB modes until the adapter accepts reports",
        ),
        menu_item("Xbox", "choose Xbox 360 or Xbox One USB mode"),
        menu_item(
            "DInput / PlayStation",
            "choose PS3, generic HID, or PS4 mode",
        ),
        menu_item(
            "Bluetooth",
            "PC USB input with Bluetooth output to a wireless receiver",
        ),
        menu_item(
            "Maple",
            "Xbox-compatible mode labelled for Dreamcast adapters",
        ),
        menu_item("Keyboard", "USB HID keyboard mode"),
        menu_item("Debug", "XInput mode with raw USB packet capture"),
    ];
    match select("Pico input mode", &choices, 0).await? {
        0 => Ok(InputModeChoice::Auto),
        1 => choose_xbox_input_mode().await,
        2 => choose_playstation_input_mode().await,
        3 => choose_bluetooth_input_mode().await,
        4 => Ok(InputModeChoice::Persona(protocol::Persona::Maple)),
        5 => Ok(InputModeChoice::Persona(protocol::Persona::Keyboard)),
        _ => Ok(InputModeChoice::Persona(protocol::Persona::Debug)),
    }
}

pub(super) async fn choose_xbox_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item("Auto Xbox", "try Xbox 360, then Xbox One"),
        menu_item("Xbox 360", "wired Xbox 360 / XInput USB mode"),
        menu_item("Xbox One", "Xbox One-compatible USB mode"),
    ];
    match select("Xbox input mode", &choices, 0).await? {
        0 => Ok(InputModeChoice::Family(cmd_auto::XBOX_FAMILY)),
        1 => Ok(InputModeChoice::Persona(protocol::Persona::Xinput)),
        _ => Ok(InputModeChoice::Persona(protocol::Persona::XboxOne)),
    }
}

pub(super) async fn choose_playstation_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item("Auto DInput", "try PS3, generic HID, then PS4"),
        menu_item("PS3", "DualShock 3 / PS3 HID mode"),
        menu_item("Generic HID", "unknown-HID gamepad mode"),
        menu_item("PS4", "DualShock 4 / PS4 HID mode"),
    ];
    match select("DInput input mode", &choices, 0).await? {
        0 => Ok(InputModeChoice::Family(cmd_auto::PLAYSTATION_FAMILY)),
        1 => Ok(InputModeChoice::Persona(protocol::Persona::Ps3)),
        2 => Ok(InputModeChoice::Persona(protocol::Persona::GenericHid)),
        _ => Ok(InputModeChoice::Persona(protocol::Persona::Ps4)),
    }
}

pub(super) async fn choose_bluetooth_input_mode() -> Result<InputModeChoice> {
    let choices = vec![
        menu_item("Generic HID", "CouchLink Bluetooth HID gamepad"),
        menu_item(
            "Xbox Wireless Controller",
            "advertise as a supported Xbox Bluetooth controller",
        ),
        menu_item(
            "DualShock 4",
            "advertise as a supported PlayStation Wireless Controller",
        ),
    ];
    let persona = match select("Bluetooth controller to mimic", &choices, 0).await? {
        0 => protocol::Persona::BluetoothHid,
        1 => protocol::Persona::BluetoothXbox,
        _ => protocol::Persona::BluetoothPlaystation,
    };
    cmd_run::print_bluetooth_pairing_help(persona);
    Ok(InputModeChoice::Persona(persona))
}

pub(super) async fn prepare_routes_for_input_mode(
    routes: Vec<cmd_run::StreamRoute>,
    mode: InputModeChoice,
) -> Result<Vec<cmd_run::StreamRoute>> {
    match mode {
        InputModeChoice::Auto => {
            let targets: Vec<_> = routes.iter().map(|route| route.pico.clone()).collect();
            let ready = cmd_auto::select_gamepad_targets(targets).await?;
            let routes = replace_route_targets(routes, &ready);
            require_all_routes_in_targets(routes, &ready)
        }
        InputModeChoice::Family(candidates) => {
            let targets: Vec<_> = routes.iter().map(|route| route.pico.clone()).collect();
            let ready =
                cmd_auto::select_gamepad_targets_from_candidates(targets, candidates).await?;
            let routes = replace_route_targets(routes, &ready);
            require_all_routes_in_targets(routes, &ready)
        }
        InputModeChoice::Persona(persona) => switch_routes_to_persona(routes, persona).await,
    }
}

pub(super) async fn switch_routes_to_persona(
    mut routes: Vec<cmd_run::StreamRoute>,
    desired: protocol::Persona,
) -> Result<Vec<cmd_run::StreamRoute>> {
    let mut switched_uids = Vec::new();
    for route in &routes {
        if route.pico.persona == desired {
            continue;
        }
        outln!(
            "Switching {} to {} mode...",
            route.pico.short_label(),
            desired.label()
        );
        pico_mode::request_set_persona(&route.pico, desired).await?;
        switched_uids.push(route.pico.info.unique_id_short);
    }

    if !switched_uids.is_empty() {
        outln!(
            "Waiting up to {}s for the Pico(s) to reboot into {} mode...",
            cmd_persona::REBOOT_WAIT.as_secs(),
            desired.label()
        );
        let reappeared =
            cmd_persona::wait_for_persona(&switched_uids, desired, cmd_persona::REBOOT_WAIT)
                .await?;
        routes = replace_route_targets(routes, &reappeared);
    }

    let pending: Vec<_> = routes
        .iter()
        .filter(|route| route.pico.persona != desired)
        .map(|route| route.pico.uid_hex())
        .collect();
    if !pending.is_empty() {
        anyhow::bail!(
            "{} did not confirm {} mode",
            pending.join(", "),
            desired.label()
        );
    }
    Ok(routes)
}

pub(super) fn replace_route_targets(
    mut routes: Vec<cmd_run::StreamRoute>,
    targets: &[cmd_run::PicoTarget],
) -> Vec<cmd_run::StreamRoute> {
    for route in &mut routes {
        if let Some(target) = targets
            .iter()
            .find(|target| target.info.unique_id_short == route.pico.info.unique_id_short)
        {
            route.pico = target.clone();
        }
    }
    routes
}

pub(super) fn require_all_routes_in_targets(
    routes: Vec<cmd_run::StreamRoute>,
    targets: &[cmd_run::PicoTarget],
) -> Result<Vec<cmd_run::StreamRoute>> {
    let missing: Vec<_> = routes
        .iter()
        .filter(|route| {
            !targets
                .iter()
                .any(|target| target.info.unique_id_short == route.pico.info.unique_id_short)
        })
        .map(|route| route.pico.uid_hex())
        .collect();
    if !missing.is_empty() {
        anyhow::bail!(
            "{} did not return in the selected input mode",
            missing.join(", ")
        );
    }
    Ok(routes)
}
