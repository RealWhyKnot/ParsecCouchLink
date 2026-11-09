use super::{BLUETOOTH_SOURCE_POLL, BLUETOOTH_SOURCE_WAIT, StreamRoute};
use crate::{cmd_lab, xinput};
use anyhow::{Result, bail};
use std::collections::BTreeSet;
use std::time::Instant;
use tokio::time::sleep;

pub(super) async fn prepare_bluetooth_routes(
    mut routes: Vec<StreamRoute>,
    quiet: bool,
) -> Result<Vec<StreamRoute>> {
    if !routes.iter().any(|route| route.pico.persona.is_bluetooth()) {
        return Ok(routes);
    }
    verify_bluetooth_picos_are_not_xinput_sources(&routes, quiet)?;
    wait_for_bluetooth_source_slots(&mut routes, quiet).await?;
    Ok(routes)
}

pub(super) fn verify_bluetooth_picos_are_not_xinput_sources(
    routes: &[StreamRoute],
    quiet: bool,
) -> Result<()> {
    let uids: BTreeSet<u32> = routes
        .iter()
        .filter(|route| route.pico.persona.is_bluetooth())
        .map(|route| route.pico.info.unique_id_short)
        .collect();
    if uids.is_empty() {
        return Ok(());
    }

    match cmd_lab::connected_xinput_instance_ids_for_uids(&uids) {
        Ok(instance_ids) if instance_ids.is_empty() => Ok(()),
        Ok(instance_ids) => bail!("{}", selected_pico_xinput_source_error(&instance_ids)),
        Err(e) => {
            tracing::warn!(
                "stream: could not verify selected Bluetooth Pico XInput removal: {e:#}"
            );
            if !quiet {
                outln!(
                    "Note: could not verify whether the selected Pico is still exposed as a Windows XInput source: {e:#}"
                );
                outln!("  Choose the Parsec or local controller source, not the selected Pico.");
            }
            Ok(())
        }
    }
}

pub(super) fn selected_pico_xinput_source_error(instance_ids: &[String]) -> String {
    let details = if instance_ids.is_empty() {
        "no instance ID captured".to_string()
    } else {
        instance_ids.join(", ")
    };
    format!(
        "the selected Bluetooth Pico is still exposed as a local Windows XInput device ({details}). Unplug/replug the Pico or wait for Windows to remove the old controller, then run the Bluetooth command again."
    )
}

pub(super) async fn wait_for_bluetooth_source_slots(
    routes: &mut [StreamRoute],
    quiet: bool,
) -> Result<()> {
    if !routes.iter().any(|route| route.pico.persona.is_bluetooth()) {
        return Ok(());
    }

    let allow_auto_switch = should_auto_switch_bluetooth_source(routes);
    let deadline = Instant::now() + BLUETOOTH_SOURCE_WAIT;
    let mut announced_wait = false;
    loop {
        let connected = xinput::connected_slots();
        let live_slots: Vec<u32> = connected.iter().map(|slot| slot.slot).collect();
        let mut missing = Vec::new();
        let mut auto_switched = Vec::new();

        for route in routes
            .iter_mut()
            .filter(|route| route.pico.persona.is_bluetooth())
        {
            match bluetooth_source_slot_decision(route.source_slot, &connected, allow_auto_switch) {
                BluetoothSourceSlotDecision::Ready => {}
                BluetoothSourceSlotDecision::AutoSwitch { from, to } => {
                    route.source_slot = to;
                    auto_switched.push((route.pico.uid_hex(), from, to));
                }
                BluetoothSourceSlotDecision::Missing => {
                    missing.push(MissingBluetoothSource {
                        pico_uid: route.pico.uid_hex(),
                        selected_slot: route.source_slot,
                        live_slots: live_slots.clone(),
                    });
                }
            }
        }

        for (uid, from, to) in auto_switched {
            tracing::info!(
                "stream: Bluetooth source auto-switched pico={} from {} to {}",
                uid,
                xinput::user_slot_label(from),
                xinput::user_slot_label(to)
            );
            if !quiet {
                outln!(
                    "Bluetooth source auto-selected: {} was not live, using the only live source {} for Pico {}.",
                    xinput::user_slot_label(from),
                    xinput::user_slot_label(to),
                    uid
                );
            }
        }

        if missing.is_empty() {
            for route in routes
                .iter()
                .filter(|route| route.pico.persona.is_bluetooth())
            {
                if let Some(snapshot) = connected
                    .iter()
                    .find(|snapshot| snapshot.slot == route.source_slot)
                {
                    tracing::info!(
                        "stream: Bluetooth source ready pico={} source={} packet={} {}",
                        route.pico.uid_hex(),
                        route.source_label(),
                        snapshot.packet_number,
                        format_xinput_snapshot(snapshot)
                    );
                    if !quiet {
                        outln!(
                            "Bluetooth source ready: {} feeds Pico {} ({}).",
                            route.source_label(),
                            route.pico.uid_hex(),
                            format_xinput_snapshot(snapshot)
                        );
                    }
                }
            }
            return Ok(());
        }

        if Instant::now() >= deadline {
            bail!("{}", bluetooth_source_preflight_error(&missing));
        }

        if !announced_wait && !quiet {
            outln!(
                "Waiting up to {}s for the Bluetooth source controller to appear...",
                BLUETOOTH_SOURCE_WAIT.as_secs()
            );
            for item in &missing {
                outln!(
                    "  {} for Pico {} is not live. Live XInput sources: {}.",
                    xinput::user_slot_label(item.selected_slot),
                    item.pico_uid,
                    format_live_slot_labels(&item.live_slots)
                );
            }
            announced_wait = true;
        }
        sleep(BLUETOOTH_SOURCE_POLL).await;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BluetoothSourceSlotDecision {
    Ready,
    AutoSwitch { from: u32, to: u32 },
    Missing,
}

pub(super) fn bluetooth_source_slot_decision(
    selected_slot: u32,
    connected: &[xinput::SlotSnapshot],
    allow_auto_switch: bool,
) -> BluetoothSourceSlotDecision {
    if connected.iter().any(|slot| slot.slot == selected_slot) {
        return BluetoothSourceSlotDecision::Ready;
    }
    if allow_auto_switch && connected.len() == 1 {
        return BluetoothSourceSlotDecision::AutoSwitch {
            from: selected_slot,
            to: connected[0].slot,
        };
    }
    BluetoothSourceSlotDecision::Missing
}

pub(super) fn should_auto_switch_bluetooth_source(routes: &[StreamRoute]) -> bool {
    routes.len() == 1 && routes[0].pico.persona.is_bluetooth()
}

#[derive(Clone, Debug)]
pub(super) struct MissingBluetoothSource {
    pub(super) pico_uid: String,
    pub(super) selected_slot: u32,
    pub(super) live_slots: Vec<u32>,
}

pub(super) fn bluetooth_source_preflight_error(missing: &[MissingBluetoothSource]) -> String {
    let details = missing
        .iter()
        .map(|item| {
            format!(
                "{} for Pico {} is not live; live XInput sources: {}",
                xinput::user_slot_label(item.selected_slot),
                item.pico_uid,
                format_live_slot_labels(&item.live_slots)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "Bluetooth mode needs a live Windows source controller before streaming. {details}. Connect the Parsec or local controller first, or choose a live source with `couchlink run --route N=UID`."
    )
}

pub(super) fn format_live_slot_labels(slots: &[u32]) -> String {
    if slots.is_empty() {
        return "none".to_string();
    }
    slots
        .iter()
        .map(|slot| xinput::user_slot_label(*slot))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn format_xinput_snapshot(snapshot: &xinput::SlotSnapshot) -> String {
    format!(
        "buttons=0x{:04X} lt={} rt={} lx={} ly={} rx={} ry={}",
        snapshot.state.buttons,
        snapshot.state.left_trigger,
        snapshot.state.right_trigger,
        snapshot.state.left_x,
        snapshot.state.left_y,
        snapshot.state.right_x,
        snapshot.state.right_y
    )
}
