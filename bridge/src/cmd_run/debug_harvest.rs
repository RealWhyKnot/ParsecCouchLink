use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::protocol::Persona;
use crate::{debug_packets, pico_cache};

use super::{PicoTarget, RouteRuntime};

pub(in crate::cmd_run) const DEBUG_PACKET_HARVEST_EVERY: Duration = Duration::from_millis(500);
const DEBUG_PACKET_HARVEST_TIMEOUT: Duration = Duration::from_millis(1200);

#[derive(Clone, Debug)]
pub(in crate::cmd_run) struct DebugPacketHarvestResult {
    target: PicoTarget,
    duration_ms: u64,
    outcome: Result<DebugPacketHarvestOk, String>,
}

#[derive(Clone, Debug)]
struct DebugPacketHarvestOk {
    lines: Vec<String>,
    raw_packet_lines: usize,
    stats_lines: usize,
    event_lines: usize,
    snapshot: debug_packets::DiagLogSnapshot,
}

pub(in crate::cmd_run) fn ensure_debug_packet_sinks(
    routes: &[RouteRuntime],
    sinks: &mut HashMap<u32, debug_packets::DebugPacketSink>,
    disabled: &mut HashSet<u32>,
    quiet: bool,
) {
    for route in routes
        .iter()
        .filter(|route| route.route.pico.persona == Persona::Debug)
    {
        ensure_debug_packet_sink_for_target(&route.route.pico, sinks, disabled, quiet);
    }
}

fn ensure_debug_packet_sink_for_target(
    target: &PicoTarget,
    sinks: &mut HashMap<u32, debug_packets::DebugPacketSink>,
    disabled: &mut HashSet<u32>,
    quiet: bool,
) {
    let uid = target.info.unique_id_short;
    if sinks.contains_key(&uid) || disabled.contains(&uid) {
        return;
    }
    match debug_packets::DebugPacketSink::create(&target.uid_hex(), target.peer) {
        Ok(sink) => {
            tracing::info!(
                "debug-packets: capturing {} from {} into {}",
                target.uid_hex(),
                target.peer,
                sink.path().display()
            );
            if !quiet {
                println!(
                    "Debug USB packet capture: {} -> {}",
                    target.short_label(),
                    sink.path().display()
                );
            }
            sinks.insert(uid, sink);
        }
        Err(e) => {
            disabled.insert(uid);
            tracing::warn!(
                "debug-packets: disabled for {}: {e:#}",
                target.short_label()
            );
            if !quiet {
                println!(
                    "Debug USB packet capture could not open a retained log for {}: {e:#}",
                    target.short_label()
                );
            }
        }
    }
}

pub(in crate::cmd_run) fn has_debug_packet_routes(
    routes: &[RouteRuntime],
    disabled: &HashSet<u32>,
) -> bool {
    routes.iter().any(|route| {
        route.route.pico.persona == Persona::Debug
            && !disabled.contains(&route.route.pico.info.unique_id_short)
    })
}

pub(in crate::cmd_run) fn debug_packet_harvest_targets(
    routes: &[RouteRuntime],
    disabled: &HashSet<u32>,
) -> Vec<PicoTarget> {
    routes
        .iter()
        .filter(|route| {
            route.route.pico.persona == Persona::Debug
                && !disabled.contains(&route.route.pico.info.unique_id_short)
        })
        .map(|route| route.route.pico.clone())
        .collect()
}

pub(in crate::cmd_run) async fn collect_debug_packet_harvests(
    targets: Vec<PicoTarget>,
) -> Vec<DebugPacketHarvestResult> {
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let started = Instant::now();
        let outcome =
            match debug_packets::capture_run_diag_log(target.peer, DEBUG_PACKET_HARVEST_TIMEOUT)
                .await
            {
                Ok(snapshot) => {
                    let lines = debug_packets::extract_usb_packet_lines(&snapshot.text);
                    Ok(DebugPacketHarvestOk {
                        raw_packet_lines: lines
                            .iter()
                            .filter(|line| line.starts_with("usb-packet "))
                            .count(),
                        stats_lines: lines
                            .iter()
                            .filter(|line| line.starts_with("usb-packet-stats "))
                            .count(),
                        event_lines: lines
                            .iter()
                            .filter(|line| line.starts_with("usb-event "))
                            .count(),
                        lines,
                        snapshot,
                    })
                }
                Err(e) => Err(format!("{e:#}")),
            };
        out.push(DebugPacketHarvestResult {
            target,
            duration_ms: duration_ms_u64(started.elapsed()),
            outcome,
        });
    }
    out
}

pub(in crate::cmd_run) fn apply_debug_packet_harvests(
    results: Vec<DebugPacketHarvestResult>,
    sinks: &mut HashMap<u32, debug_packets::DebugPacketSink>,
    disabled: &mut HashSet<u32>,
    quiet: bool,
) {
    for result in results {
        let uid = result.target.info.unique_id_short;
        ensure_debug_packet_sink_for_target(&result.target, sinks, disabled, quiet);
        let Some(sink) = sinks.get_mut(&uid) else {
            continue;
        };
        let duration_ms = result.duration_ms;
        match result.outcome {
            Ok(ok) => {
                let written = match sink.append_lines(&ok.lines) {
                    Ok(written) => written,
                    Err(e) => {
                        tracing::warn!(
                            "debug-packets: write failed for {}: {e:#}",
                            result.target.short_label()
                        );
                        disabled.insert(uid);
                        continue;
                    }
                };
                let harvest_record = debug_packets::HarvestOkRecord {
                    duration_ms,
                    snapshot: ok.snapshot,
                    packet_lines: ok.lines.len(),
                    raw_packet_lines: ok.raw_packet_lines,
                    stats_lines: ok.stats_lines,
                    event_lines: ok.event_lines,
                    new_lines: written,
                };
                let lost_bytes = harvest_record.snapshot.lost_bytes;
                let chunk_count = harvest_record.snapshot.chunk_count;
                let missing_chunk_count = harvest_record.snapshot.missing_chunks.len();
                let duplicate_chunk_count = harvest_record.snapshot.duplicate_chunk_count;
                if let Err(e) = sink.append_harvest_ok(harvest_record) {
                    tracing::warn!(
                        "debug-packets: harvest metadata write failed for {}: {e:#}",
                        result.target.short_label()
                    );
                    disabled.insert(uid);
                    continue;
                }
                tracing::debug!(
                    "debug-packets: harvest {} duration_ms={} chunks={} lost={} packets={} new={} total={}",
                    result.target.short_label(),
                    duration_ms,
                    chunk_count,
                    lost_bytes,
                    ok.lines.len(),
                    written,
                    sink.total_written()
                );
                if missing_chunk_count > 0 || duplicate_chunk_count > 0 {
                    tracing::debug!(
                        "debug-packets: harvest {} chunk health missing={} duplicate={}",
                        result.target.short_label(),
                        missing_chunk_count,
                        duplicate_chunk_count
                    );
                }
                if written > 0 || lost_bytes > 0 || missing_chunk_count > 0 {
                    pico_cache::record(
                        pico_cache::PicoStateSnapshot::from_target(
                            "debug-packet-harvest",
                            &result.target,
                        )
                        .with_outcome(format!(
                            "new_packets={written}; total_packets={}; lost_bytes={}; chunks={}; missing_chunks={}",
                            sink.total_written(),
                            lost_bytes,
                            chunk_count,
                            missing_chunk_count
                        )),
                    );
                }
            }
            Err(e) => {
                if let Err(write_error) = sink.append_harvest_error(duration_ms, &e) {
                    tracing::warn!(
                        "debug-packets: harvest failure metadata write failed for {}: {write_error:#}",
                        result.target.short_label()
                    );
                    disabled.insert(uid);
                    continue;
                }
                tracing::debug!(
                    "debug-packets: harvest failed for {} duration_ms={duration_ms}: {e}",
                    result.target.short_label(),
                );
            }
        }
    }
    debug_packets::prune_packet_files();
}

fn duration_ms_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
