use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::modes::{AfterSwitch, PicoScope};
use crate::protocol::Persona;
use crate::{cmd_lab, cmd_run, pico_cache, pico_mode, support};

pub(crate) const DISCOVER: Duration = Duration::from_secs(5);
pub(crate) const REBOOT_WAIT: Duration = Duration::from_secs(60);
const XINPUT_RELEASE_WAIT: Duration = Duration::from_secs(8);
const XINPUT_RELEASE_POLL: Duration = Duration::from_millis(500);

pub(crate) async fn run(
    desired: Persona,
    selectors: Vec<String>,
    scope: PicoScope,
    after: AfterSwitch,
) -> Result<()> {
    tracing::info!(
        "persona: desired={} selectors={} all={} stream={}",
        desired.label(),
        selectors.len(),
        scope.is_all(),
        after.streams(),
    );
    let picos = cmd_run::discover_picos_with_auto_recovery(DISCOVER, false).await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(DISCOVER.as_secs()));
    }

    let targets = select_targets(&picos, &selectors, scope)?;
    let switched_uids = request_persona_switches(&targets, desired).await?;

    let final_targets = if switched_uids.is_empty() {
        targets
    } else {
        outln!(
            "Waiting up to {}s for the Pico(s) to reboot into {} mode...",
            REBOOT_WAIT.as_secs(),
            desired.label()
        );
        let reappeared = wait_for_persona(&switched_uids, desired, REBOOT_WAIT).await?;
        merge_targets(targets, reappeared, &switched_uids)
    };
    report_final_personas(&final_targets, desired);

    if !after.streams() {
        outln!(
            "Run `couchlink run` to start streaming, or run this command again without --no-stream."
        );
        return Ok(());
    }

    let ready: Vec<_> = final_targets
        .into_iter()
        .filter(|t| t.persona == desired)
        .collect();
    if ready.is_empty() {
        bail!(
            "no Pico is in {} mode yet; nothing to stream",
            desired.label()
        );
    }
    if desired.is_bluetooth() {
        wait_for_bluetooth_xinput_release(&ready, false).await?;
    }
    let routes = cmd_run::auto_routes(ready, preferred_source_slots_for_persona(desired))?;
    cmd_run::stream_routes(routes, cmd_run::StreamOptions::default()).await
}

async fn request_persona_switches(
    targets: &[cmd_run::PicoTarget],
    desired: Persona,
) -> Result<Vec<u32>> {
    let mut switched_uids = Vec::new();
    for t in targets {
        if t.persona == desired {
            pico_cache::record(
                &pico_cache::PicoStateSnapshot::from_target("persona-already", t)
                    .with_outcome(format!("already_{}", desired.label())),
            );
            outln!(
                "{} is already in {} mode.",
                t.short_label(),
                desired.label()
            );
            continue;
        }
        outln!(
            "Switching {} to {} mode...",
            t.short_label(),
            desired.label()
        );
        tracing::info!(
            "persona: switching {} from {} to {}",
            t.short_label(),
            t.persona.label(),
            desired.label(),
        );
        pico_cache::record(
            &pico_cache::PicoStateSnapshot::from_target("persona-switch-request", t)
                .with_outcome(format!("requested_{}", desired.label())),
        );
        pico_mode::request_set_persona(t, desired).await?;
        switched_uids.push(t.info.unique_id_short);
    }
    Ok(switched_uids)
}

fn report_final_personas(final_targets: &[cmd_run::PicoTarget], desired: Persona) {
    for t in final_targets {
        let mark = if t.persona == desired {
            "ok"
        } else {
            "pending"
        };
        outln!(
            "  [{mark}] {} is now in {} mode",
            t.short_label(),
            t.persona.label()
        );
        pico_cache::record(
            &pico_cache::PicoStateSnapshot::from_target("persona-confirm", t)
                .with_outcome(format!("mark={mark} desired={}", desired.label())),
        );
    }
    if desired.is_bluetooth() {
        cmd_run::print_bluetooth_pairing_help(desired);
    }
    let pending: Vec<_> = final_targets
        .iter()
        .filter(|t| t.persona != desired)
        .map(super::cmd_run::PicoTarget::uid_hex)
        .collect();
    if !pending.is_empty() {
        outln!(
            "Note: {} did not confirm {} mode yet. Give it a moment, then run `couchlink bundle` if it still does not confirm.",
            pending.join(", "),
            desired.label()
        );
    }
}

fn preferred_source_slots_for_persona(desired: Persona) -> Option<Vec<u32>> {
    if desired.is_bluetooth() {
        None
    } else {
        Some((0..4).collect())
    }
}

pub(crate) async fn wait_for_bluetooth_xinput_release(
    targets: &[cmd_run::PicoTarget],
    quiet: bool,
) -> Result<()> {
    let uids: BTreeSet<u32> = targets
        .iter()
        .map(|target| target.info.unique_id_short)
        .collect();
    if uids.is_empty() {
        return Ok(());
    }

    let deadline = Instant::now() + XINPUT_RELEASE_WAIT;
    let mut announced = false;
    loop {
        let instance_ids = match cmd_lab::connected_xinput_instance_ids_for_uids(&uids) {
            Ok(instance_ids) if instance_ids.is_empty() => {
                if announced && !quiet {
                    outln!("Selected Pico XInput endpoint is gone.");
                }
                return Ok(());
            }
            Ok(instance_ids) => instance_ids,
            Err(e) => {
                if !quiet {
                    outln!(
                        "Note: could not verify whether the selected Pico is still exposed as a Windows XInput source: {e:#}"
                    );
                    outln!("Choose the Parsec or local controller source, not the selected Pico.");
                }
                return Ok(());
            }
        };

        if Instant::now() >= deadline {
            bail!("{}", selected_pico_xinput_release_error(&instance_ids));
        }

        if !announced && !quiet {
            outln!("Waiting for Windows to remove the selected Pico's old XInput source...");
            announced = true;
        }
        tokio::time::sleep(XINPUT_RELEASE_POLL).await;
    }
}

fn selected_pico_xinput_release_error(instance_ids: &[String]) -> String {
    let details = if instance_ids.is_empty() {
        "no instance ID captured".to_string()
    } else {
        instance_ids.join(", ")
    };
    format!(
        "the selected Pico is still exposed as a local XInput device after switching to Bluetooth ({details}). Unplug/replug the Pico or wait for Windows to remove the old controller, then run the Bluetooth command again."
    )
}

pub(crate) fn select_targets(
    picos: &[cmd_run::PicoTarget],
    selectors: &[String],
    scope: PicoScope,
) -> Result<Vec<cmd_run::PicoTarget>> {
    if scope.is_all() {
        return Ok(picos.to_vec());
    }
    if !selectors.is_empty() {
        let mut out: Vec<cmd_run::PicoTarget> = Vec::new();
        for s in selectors {
            let p = cmd_run::match_pico_selector(s, picos)?;
            if !out
                .iter()
                .any(|q| q.info.unique_id_short == p.info.unique_id_short)
            {
                out.push(p);
            }
        }
        return Ok(out);
    }
    match picos {
        [one] => Ok(vec![one.clone()]),
        [] => bail!("no Pico found on Wi-Fi"),
        _ => bail!("more than one Pico found; pass a UID/IP/board selector or --all"),
    }
}

pub(crate) async fn wait_for_persona(
    uids: &[u32],
    desired: Persona,
    timeout: Duration,
) -> Result<Vec<cmd_run::PicoTarget>> {
    let want: HashSet<u32> = uids.iter().copied().collect();
    let deadline = Instant::now() + timeout;
    loop {
        let picos = cmd_run::discover_picos(Duration::from_secs(2)).await?;
        let matched: Vec<cmd_run::PicoTarget> = picos
            .into_iter()
            .filter(|p| want.contains(&p.info.unique_id_short))
            .collect();
        let all_confirmed = want.iter().all(|uid| {
            matched
                .iter()
                .any(|p| p.info.unique_id_short == *uid && p.persona == desired)
        });
        if all_confirmed {
            return Ok(matched);
        }
        if Instant::now() >= deadline {
            return Ok(matched);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn merge_targets(
    original: Vec<cmd_run::PicoTarget>,
    reappeared: Vec<cmd_run::PicoTarget>,
    switched_uids: &[u32],
) -> Vec<cmd_run::PicoTarget> {
    let switched: HashSet<u32> = switched_uids.iter().copied().collect();
    let mut out: Vec<cmd_run::PicoTarget> = original
        .into_iter()
        .filter(|p| !switched.contains(&p.info.unique_id_short))
        .collect();
    out.extend(reappeared);
    out
}

#[cfg(test)]
mod tests;
