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

use crate::config;

pub fn init(verbose: u8, file_logging: bool) -> Result<Option<WorkerGuard>> {
    let default_level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let make_filter = || {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("ptd_bridge={default_level}")))
    };

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(std::io::stderr);

    if !file_logging {
        tracing_subscriber::registry()
            .with(make_filter())
            .with(stderr_layer)
            .init();
        return Ok(None);
    }

    config::ensure_dirs()?;
    let log_dir = config::log_dir()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("ptd-bridge")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_dir)?;
    let (nonblock, guard) = tracing_appender::non_blocking(appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_writer(nonblock);

    tracing_subscriber::registry()
        .with(make_filter())
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("log dir: {}", log_dir.display());
    Ok(Some(guard))
}
