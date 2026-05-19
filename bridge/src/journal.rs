//! Append-only operator-readable timeline of bridge events.
//!
//! The journal is a parallel system to the rotating tracing log. The
//! tracing log is verbose and structured for full-fidelity debugging;
//! this journal carries one short line per high-signal event in a
//! format an operator can scan without a tool. Lives at
//! `<log_dir>/state-journal.log`, append-only, no rotation.
//!
//! Operations are best-effort: a journal write failure never blocks
//! the caller and emits a single `tracing::warn!` per process so the
//! support bundle still surfaces it. Concurrency is handled by a
//! global `Mutex<Option<File>>` -- the journal sees one line per
//! operator-visible event at most, so contention is irrelevant.
//!
//! Bundle picks up the file by name; survives across program
//! restarts.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use chrono::Local;

use crate::config;

const FILENAME: &str = "state-journal.log";
const MAX_BYTES_KEPT: u64 = 1024 * 1024; // 1 MiB -- truncate on init when over

static FILE: LazyLock<Mutex<Option<File>>> = LazyLock::new(|| Mutex::new(None));
static WARNED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

pub fn path() -> Option<PathBuf> {
    config::log_dir().ok().map(|d| d.join(FILENAME))
}

/// Open (or create) the journal file. Truncates if over the size cap
/// so a long-running bridge doesn't grow it unboundedly -- the journal
/// captures recent operator-visible events, not full history. The
/// rotating tracing log is the full-fidelity record.
pub fn init() {
    let Some(p) = path() else {
        return;
    };
    let _ = config::ensure_dirs();
    if let Ok(meta) = std::fs::metadata(&p) {
        if meta.len() > MAX_BYTES_KEPT {
            // Half-truncate by reading the last ~256 KiB and rewriting.
            // Simpler than a copy-and-rename. If anything errors, the
            // file just stays and gets appended to.
            let keep: usize = 256 * 1024;
            if let Ok(bytes) = std::fs::read(&p) {
                let start = bytes.len().saturating_sub(keep);
                // Find a newline boundary so we don't start mid-line.
                let start = bytes[start..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|i| start + i + 1)
                    .unwrap_or(start);
                let trimmed = &bytes[start..];
                let _ = std::fs::write(&p, trimmed);
            }
        }
    }
    let f = match OpenOptions::new().create(true).append(true).open(&p) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(
                "journal: could not open {}: {e}. State journal disabled this run.",
                p.display()
            );
            return;
        }
    };
    *FILE.lock().unwrap() = Some(f);
    event("bridge", "started").ok();
}

/// Write a single journal entry. Category is a short tag (5-10 chars)
/// describing the subsystem (setup, cdc, udp, doctor, bundle, run,
/// fw_state). Message should fit on one line. Trailing newlines are
/// added automatically.
pub fn event(category: &str, message: impl AsRef<str>) -> std::io::Result<()> {
    let mut guard = FILE.lock().unwrap();
    let Some(f) = guard.as_mut() else {
        return Ok(()); // disabled this run
    };
    let ts = Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
    let line = format!("{ts} [{category}] {}\n", message.as_ref());
    match f.write_all(line.as_bytes()).and_then(|_| f.flush()) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Disable further writes; log the warning once.
            let mut warned = WARNED.lock().unwrap();
            if !*warned {
                tracing::warn!(
                    "journal: write failed: {e}. Journal disabled for the rest of this run."
                );
                *warned = true;
            }
            *guard = None;
            Err(e)
        }
    }
}

/// Convenience macro: `journal!("category", "fmt {} {}", a, b)`. Errors
/// are swallowed because journal writes are diagnostic, not load-bearing.
#[macro_export]
macro_rules! journal {
    ($cat:expr, $($arg:tt)+) => {{
        let _ = $crate::journal::event($cat, format!($($arg)+));
    }};
}
