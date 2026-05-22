//! `tracing` setup: stderr layer for the running terminal plus an
//! optional rolling file appender under the per-user data dir. The
//! returned guard must be held for the program's lifetime so pending
//! writes get flushed on exit.

use std::sync::OnceLock;

use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::config;

/// Type-erased filter swappers, registered by `init()` once the layers exist.
/// `set_filter()` invokes each in turn so runtime controls can re-target
/// verbosity at runtime without spelling out the layered subscriber type.
type FilterSetter = Box<dyn Fn(&str) -> Result<()> + Send + Sync>;
static STDERR_SETTER: OnceLock<FilterSetter> = OnceLock::new();
static FILE_SETTER: OnceLock<FilterSetter> = OnceLock::new();

/// Apply a new tracing filter directive at runtime. Both stderr and file
/// filters (when present) are updated. Returns an error if the directive does
/// not parse; missing reload handles (no `init()` yet) are skipped silently.
#[allow(dead_code)] // exported for future runtime-tuning hooks
pub fn set_filter(directive: &str) -> Result<()> {
    // Parse-check first so a typo doesn't half-apply.
    EnvFilter::try_new(directive)
        .map_err(|e| anyhow::anyhow!("invalid filter directive '{directive}': {e}"))?;

    if let Some(setter) = STDERR_SETTER.get() {
        setter(directive)?;
    }
    if let Some(setter) = FILE_SETTER.get() {
        setter(directive)?;
    }
    tracing::info!("tracing filter updated to: {directive}");
    Ok(())
}

pub fn init(verbose: u8, file_logging: bool) -> Result<Option<WorkerGuard>> {
    // The directive must match the binary's crate identifier, which is the
    // `[[bin]] name` in Cargo.toml -- currently `couchlink`. A previous rename
    // left the directive pointed at a crate name (`parsec_couchlink`) that no
    // event target ever matched, so EnvFilter silently dropped every trace
    // and the rolling files stayed at 0 bytes. The regression test below
    // exists to catch the next time someone renames the binary without
    // touching this string.
    let (stderr_filter_str, file_filter_str) = if std::env::var("RUST_LOG").is_ok() {
        let v = std::env::var("RUST_LOG").unwrap();
        (v.clone(), v)
    } else {
        let stderr_lvl = match verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        };
        let file_lvl = match verbose {
            0 => "debug",
            _ => "trace",
        };
        (
            format!("couchlink={stderr_lvl}"),
            format!("couchlink={file_lvl}"),
        )
    };

    let (stderr_filter, stderr_handle) =
        tracing_subscriber::reload::Layer::new(EnvFilter::new(&stderr_filter_str));
    let _ = STDERR_SETTER.set(Box::new(move |directive: &str| -> Result<()> {
        let f = EnvFilter::try_new(directive).map_err(|e| anyhow::anyhow!("parse: {e}"))?;
        stderr_handle
            .reload(f)
            .map_err(|e| anyhow::anyhow!("stderr reload: {e}"))
    }));
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_filter(stderr_filter);

    if !file_logging {
        tracing_subscriber::registry().with(stderr_layer).init();
        return Ok(None);
    }

    config::ensure_dirs()?;
    let log_dir = config::log_dir()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("couchlink")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_dir)?;
    let (nonblock, guard) = tracing_appender::non_blocking(appender);

    let (file_filter, file_handle) =
        tracing_subscriber::reload::Layer::new(EnvFilter::new(&file_filter_str));
    let _ = FILE_SETTER.set(Box::new(move |directive: &str| -> Result<()> {
        let f = EnvFilter::try_new(directive).map_err(|e| anyhow::anyhow!("parse: {e}"))?;
        file_handle
            .reload(f)
            .map_err(|e| anyhow::anyhow!("file reload: {e}"))
    }));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_writer(nonblock)
        .with_filter(file_filter);

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    // Emit a known first line so an empty log file (= filter mismatch, file
    // permission issue, or worker drop) is unambiguous. If this line is
    // missing from a log file on disk, the file layer never accepted a
    // single event and something is wrong with the wiring; if it is
    // present, every subsequent absence is a real signal.
    tracing::info!(
        "logger ready: file_filter={file_filter} dir={dir} crate_version={ver}",
        file_filter = format!(
            "couchlink={}",
            match verbose {
                0 => "debug",
                _ => "trace",
            }
        ),
        dir = log_dir.display(),
        ver = env!("CARGO_PKG_VERSION"),
    );
    Ok(Some(guard))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    /// Catches the failure mode that motivated this whole pass: the EnvFilter
    /// directive uses a crate-name prefix that no event target matches, so
    /// every `tracing::*` call is silently dropped. We build a subscriber
    /// the same way `init()` does, emit a known event, and assert the
    /// captured output contains it. If someone renames the bin target
    /// without touching the directive, this test fails.
    #[test]
    fn filter_directive_matches_this_crate() {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedBufWriter(buf.clone());

        let stderr_lvl = "info";
        let filter = tracing_subscriber::EnvFilter::new(format!("couchlink={stderr_lvl}"));
        let layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_writer(writer)
            .with_filter(filter);

        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("filter_regression_canary");
        });

        let captured = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(
            captured.contains("filter_regression_canary"),
            "EnvFilter directive `couchlink=info` did not match this crate's event \
             target. If the binary was renamed in Cargo.toml, update the directive \
             in logfile.rs to match. Captured output: {captured:?}",
        );
    }

    #[derive(Clone)]
    struct SharedBufWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedBufWriter {
        type Writer = SharedBufWriterGuard;
        fn make_writer(&'a self) -> Self::Writer {
            SharedBufWriterGuard(self.0.clone())
        }
    }

    struct SharedBufWriterGuard(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedBufWriterGuard {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // Bring `Layer::with_filter` into scope for the test.
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Layer;
}
