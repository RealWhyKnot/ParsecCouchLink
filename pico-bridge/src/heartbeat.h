#pragma once

// One-line-per-N-seconds firmware status summary that lands in the
// diag_log ring. Different content for run mode vs setup mode, but
// both share the same cadence and "hb#N" prefix so a bundle's
// pico-diag.txt has a coarse timeline of what the firmware was doing
// even when nothing else hit the log.
//
// Why: a 16 KiB diag ring with only event-driven entries can sit
// silent for minutes if nothing fails. A bundle pulled in that window
// is unable to answer "was the Pico even alive?". The heartbeat
// guarantees there is always recent state to reason about.

void heartbeat_init(void);
void heartbeat_run_mode_task(void);
void heartbeat_setup_mode_task(void);
