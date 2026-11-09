use super::log::CaptureLog;
use super::{PicoCaptureSeed, sanitize_path_component};
use crate::{cmd_run, config, pico_cache};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub(super) async fn seed_from_broadcast_discovery(
    seeds: &mut BTreeMap<String, PicoCaptureSeed>,
    capture_log: &mut CaptureLog,
) {
    let started = Instant::now();
    match cmd_run::discover_picos(Duration::from_secs(2)).await {
        Ok(picos) => {
            capture_log.record(
                "per_pico.broadcast_discovery",
                started,
                "ok",
                picos.len(),
                format!("targets={}", picos.len()),
            );
            for target in picos {
                let uid = target.uid_hex();
                seeds
                    .entry(uid.clone())
                    .and_modify(|seed| {
                        seed.target = Some(target.clone());
                        seed.source = "broadcast discovery".to_string();
                    })
                    .or_insert(PicoCaptureSeed {
                        uid,
                        target: Some(target),
                        saved: None,
                        source: "broadcast discovery".to_string(),
                        cached_state_json: None,
                    });
            }
        }
        Err(e) => {
            capture_log.record(
                "per_pico.broadcast_discovery",
                started,
                "error",
                0,
                format!("{e:#}"),
            );
        }
    }
}

pub(super) async fn seed_from_saved_config(
    seeds: &mut BTreeMap<String, PicoCaptureSeed>,
    capture_log: &mut CaptureLog,
) {
    let cfg = config::load().unwrap_or_default();
    for saved in saved_picos_from_config(&cfg) {
        let uid = saved.uid_hex();
        seeds
            .entry(uid.clone())
            .and_modify(|seed| {
                seed.saved = Some(saved.clone());
            })
            .or_insert(PicoCaptureSeed {
                uid: uid.clone(),
                target: None,
                saved: Some(saved.clone()),
                source: "saved config".to_string(),
                cached_state_json: None,
            });

        let already_live = seeds
            .get(&uid)
            .and_then(|seed| seed.target.as_ref())
            .is_some();
        if already_live {
            continue;
        }
        let Some(last_ip) = saved.last_ip.as_deref() else {
            continue;
        };
        let Some(ip) = cmd_run::parse_ip_selector(last_ip) else {
            capture_log.record(
                format!("per_pico.{uid}.last_known_ip_probe"),
                Instant::now(),
                "invalid_saved_ip",
                0,
                last_ip,
            );
            continue;
        };

        let started = Instant::now();
        match cmd_run::probe_pico_ip(ip, Duration::from_secs(2)).await {
            Ok(target) => {
                capture_log.record(
                    format!("per_pico.{uid}.last_known_ip_probe"),
                    started,
                    "ok",
                    1,
                    target.peer.to_string(),
                );
                seeds
                    .entry(uid.clone())
                    .and_modify(|seed| {
                        seed.target = Some(target.clone());
                        seed.source = "last-known IP probe".to_string();
                    })
                    .or_insert(PicoCaptureSeed {
                        uid,
                        target: Some(target),
                        saved: Some(saved),
                        source: "last-known IP probe".to_string(),
                        cached_state_json: None,
                    });
            }
            Err(e) => {
                capture_log.record(
                    format!("per_pico.{uid}.last_known_ip_probe"),
                    started,
                    "not_reachable",
                    0,
                    format!("{e:#}"),
                );
            }
        }
    }
}

pub(super) fn seed_from_diagnostic_cache(seeds: &mut BTreeMap<String, PicoCaptureSeed>) {
    if let Some(cache) = pico_cache::read_current()
        && let Some(uid) = uid_from_cache_json(&cache)
    {
        seeds
            .entry(uid.clone())
            .and_modify(|seed| {
                seed.cached_state_json = Some(cache.clone());
            })
            .or_insert(PicoCaptureSeed {
                uid,
                target: None,
                saved: None,
                source: "diagnostic cache".to_string(),
                cached_state_json: Some(cache),
            });
    }
}

pub(super) fn saved_picos_from_config(cfg: &config::Config) -> Vec<config::PicoIdentity> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for pico in &cfg.picos {
        if seen.insert(pico.unique_id_short) {
            out.push(pico.clone());
        }
    }
    if let Some(pico) = cfg.last_pico.as_ref()
        && seen.insert(pico.unique_id_short)
    {
        out.push(pico.clone());
    }
    out
}

pub(super) fn uid_from_cache_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(uid) = value.get("uid").and_then(|v| v.as_str()) {
        return Some(sanitize_path_component(uid));
    }
    let uid = value
        .get("unique_id_short")
        .and_then(serde_json::Value::as_u64)?;
    Some(format!("{:08X}", uid & 0xFFFF_FFFF))
}
