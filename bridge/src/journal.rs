use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use chrono::Local;

use crate::config;

const FILENAME: &str = "state-journal.log";
const MAX_BYTES_KEPT: u64 = 1024 * 1024;

static FILE: LazyLock<Mutex<Option<File>>> = LazyLock::new(|| Mutex::new(None));
static WARNED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));
const LOCK_INVARIANT: &str = "no journal code panics while holding a lock";

pub(crate) fn path() -> Option<PathBuf> {
    config::log_dir().ok().map(|d| d.join(FILENAME))
}

pub(crate) fn init() {
    let Some(p) = path() else {
        return;
    };
    let _ = config::ensure_dirs();
    if let Ok(meta) = std::fs::metadata(&p)
        && meta.len() > MAX_BYTES_KEPT
    {
        let keep: usize = 256 * 1024;
        if let Ok(bytes) = std::fs::read(&p) {
            let start = bytes.len().saturating_sub(keep);
            let start = bytes[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(start, |i| start + i + 1);
            let trimmed = &bytes[start..];
            let _ = std::fs::write(&p, trimmed);
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
    *FILE.lock().expect(LOCK_INVARIANT) = Some(f);
    event("bridge", "started").ok();
}

pub(crate) fn event(category: &str, message: impl AsRef<str>) -> std::io::Result<()> {
    let now = Local::now();
    let message_owned = message.as_ref().to_string();

    let mut guard = FILE.lock().expect(LOCK_INVARIANT);
    let Some(f) = guard.as_mut() else {
        return Ok(());
    };
    let ts = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
    let line = format!("{ts} [{category}] {message_owned}\n");
    match f.write_all(line.as_bytes()).and_then(|()| f.flush()) {
        Ok(()) => Ok(()),
        Err(e) => {
            *guard = None;
            drop(guard);
            let mut warned = WARNED.lock().expect(LOCK_INVARIANT);
            let first_failure = !*warned;
            *warned = true;
            drop(warned);
            if first_failure {
                tracing::warn!(
                    "journal: write failed: {e}. Journal disabled for the rest of this run."
                );
            }
            Err(e)
        }
    }
}

#[macro_export]
macro_rules! journal {
    ($cat:expr_2021, $($arg:tt)+) => {{
        let _ = $crate::journal::event($cat, format!($($arg)+));
    }};
}
