use std::sync::OnceLock;

use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config;

type FilterSetter = Box<dyn Fn(&str) -> Result<()> + Send + Sync>;
static STDERR_SETTER: OnceLock<FilterSetter> = OnceLock::new();
static FILE_SETTER: OnceLock<FilterSetter> = OnceLock::new();
pub(crate) const LOG_FILE_RETENTION: usize = 5;

pub(crate) fn init(verbose: u8, file_logging: bool) -> Result<Option<WorkerGuard>> {
    let (stderr_filter_str, file_filter_str) = if let Ok(v) = std::env::var("RUST_LOG") {
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
        .max_log_files(LOG_FILE_RETENTION)
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
        ver = crate::VERSION,
    );
    Ok(Some(guard))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    #[test]
    fn filter_directive_matches_this_crate() {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer =
            SharedBufWriter(std::sync::Arc::<std::sync::Mutex<std::vec::Vec<u8>>>::clone(&buf));

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
            SharedBufWriterGuard(
                std::sync::Arc::<std::sync::Mutex<std::vec::Vec<u8>>>::clone(&self.0),
            )
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

    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
}
