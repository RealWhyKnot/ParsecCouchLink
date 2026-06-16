//! Direct persona commands -- switch a Pico's output persona over Wi-Fi and
//! optionally start streaming to it.
//!
//! The persona is persisted on the Pico and applied at the next boot, so
//! switching reboots the board. Because the Pico lives plugged into the
//! console (not the host), the switch has to happen over UDP -- the same
//! reason `reboot-to-setup` exists. After the switch we wait for the
//! board to rejoin Wi-Fi advertising the new persona before handing off
//! to the streaming loop.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::protocol::Persona;
use crate::{cmd_run, pico_cache, pico_mode, support};

pub(crate) const DISCOVER: Duration = Duration::from_secs(5);
pub(crate) const REBOOT_WAIT: Duration = Duration::from_secs(60);

pub async fn run(desired: Persona, selectors: Vec<String>, all: bool, stream: bool) -> Result<()> {
    tracing::info!(
        "persona: desired={} selectors={} all={} stream={}",
        desired.label(),
        selectors.len(),
        all,
        stream,
    );
    let picos = cmd_run::discover_picos_with_auto_recovery(DISCOVER, false).await?;
    if picos.is_empty() {
        bail!("{}", support::no_pico_wifi_help(DISCOVER.as_secs()));
    }

    let targets = select_targets(&picos, &selectors, all)?;

    let mut switched_uids = Vec::new();
    for t in &targets {
        if t.persona == desired {
            pico_cache::record(
                pico_cache::PicoStateSnapshot::from_target("persona-already", t)
                    .with_outcome(format!("already_{}", desired.label())),
            );
            println!(
                "{} is already in {} mode.",
                t.short_label(),
                desired.label()
            );
            continue;
        }
        println!(
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
            pico_cache::PicoStateSnapshot::from_target("persona-switch-request", t)
                .with_outcome(format!("requested_{}", desired.label())),
        );
        pico_mode::request_set_persona(t, desired).await?;
        switched_uids.push(t.info.unique_id_short);
    }

    let final_targets = if switched_uids.is_empty() {
        targets
    } else {
        println!(
            "Waiting up to {}s for the Pico(s) to reboot into {} mode...",
            REBOOT_WAIT.as_secs(),
            desired.label()
        );
        let reappeared = wait_for_persona(&switched_uids, desired, REBOOT_WAIT).await?;
        merge_targets(targets, reappeared, &switched_uids)
    };

    for t in &final_targets {
        let mark = if t.persona == desired {
            "ok"
        } else {
            "pending"
        };
        println!(
            "  [{mark}] {} is now in {} mode",
            t.short_label(),
            t.persona.label()
        );
        pico_cache::record(
            pico_cache::PicoStateSnapshot::from_target("persona-confirm", t)
                .with_outcome(format!("mark={mark} desired={}", desired.label())),
        );
    }
    let pending: Vec<_> = final_targets
        .iter()
        .filter(|t| t.persona != desired)
        .map(|t| t.uid_hex())
        .collect();
    if !pending.is_empty() {
        println!(
            "Note: {} did not confirm {} mode yet. Give it a moment, then run `couchlink bundle` if it still does not confirm.",
            pending.join(", "),
            desired.label()
        );
    }

    if !stream {
        println!(
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
    let routes = cmd_run::auto_routes(ready, Some((0..4).collect()))?;
    cmd_run::stream_routes(routes, cmd_run::StreamOptions::default()).await
}

pub(crate) fn select_targets(
    picos: &[cmd_run::PicoTarget],
    selectors: &[String],
    all: bool,
) -> Result<Vec<cmd_run::PicoTarget>> {
    if all {
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
    // Keep the originals that weren't switched, then add whatever the
    // re-discovery turned up for the switched ones.
    let mut out: Vec<cmd_run::PicoTarget> = original
        .into_iter()
        .filter(|p| !switched.contains(&p.info.unique_id_short))
        .collect();
    out.extend(reappeared);
    out
}
