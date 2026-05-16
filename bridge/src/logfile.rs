//! `tracing` setup: stderr layer for the running terminal plus an
//! optional rolling file appender under the per-user data dir. The
//! returned guard must be held for the program's lifetime so pending
//! writes get flushed on exit.

use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::config;

pub fn init(verbose: u8, file_logging: bool) -> Result<Option<WorkerGuard>> {
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
            format!("parsec_couchlink={stderr_lvl}"),
            format!("parsec_couchlink={file_lvl}"),
        )
    };

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_filter(EnvFilter::new(stderr_filter_str));

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

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_writer(nonblock)
        .with_filter(EnvFilter::new(file_filter_str));

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("log dir: {}", log_dir.display());
    Ok(Some(guard))
}
