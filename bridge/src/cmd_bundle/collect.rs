//! Filesystem collection helpers: rotated logs, crash dumps, and setup
//! transcript discovery for the bundle zip.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::config;

pub(super) fn bundle_log_prefix(
    log_dir: &std::path::Path,
    prefix: &str,
    zip: &mut ZipWriter<std::fs::File>,
    opts: SimpleFileOptions,
) -> Result<()> {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "bundle: could not enumerate log dir {}: {e}",
                log_dir.display()
            );
            return Ok(());
        }
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                let p = e.path();
                let ok = p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(prefix) && n.ends_with(".log"))
                        .unwrap_or(false);
                if ok {
                    paths.push(p);
                }
            }
            Err(e) => {
                tracing::debug!("bundle: could not read entry in {}: {e}", log_dir.display());
            }
        }
    }
    paths.sort();
    let take = 3.min(paths.len());
    let recent = &paths[paths.len() - take..];
    for p in recent {
        let Some(name) = p.file_name() else { continue };
        match std::fs::read(p) {
            Ok(bytes) => {
                zip.start_file(format!("logs/{}", name.to_string_lossy()), opts)?;
                zip.write_all(&bytes)?;
            }
            Err(e) => {
                tracing::debug!("bundle: could not read log file {}: {e}", p.display());
            }
        }
    }
    Ok(())
}

pub(super) fn collect_crash_file_names() -> Vec<String> {
    let Ok(crash_dir) = config::crash_dir() else {
        return vec![];
    };
    if !crash_dir.is_dir() {
        return vec![];
    }
    let entries = match std::fs::read_dir(&crash_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "bundle: could not enumerate crash dir {}: {e}",
                crash_dir.display()
            );
            return vec![];
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                if e.path().is_file() {
                    if let Ok(name) = e.file_name().into_string() {
                        out.push(name);
                    }
                }
            }
            Err(e) => tracing::debug!("bundle: skip crash entry: {e}"),
        }
    }
    out
}

pub(super) fn collect_setup_transcript_names() -> Vec<String> {
    let Ok(log_dir) = config::log_dir() else {
        return vec![];
    };
    let entries = match std::fs::read_dir(&log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "bundle: could not enumerate log dir {}: {e}",
                log_dir.display()
            );
            return vec![];
        }
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => {
                if e.path().is_file() {
                    if let Ok(name) = e.file_name().into_string() {
                        if name.starts_with("setup-") && name.ends_with(".log") {
                            names.push(name);
                        }
                    }
                }
            }
            Err(e) => tracing::debug!("bundle: skip log entry: {e}"),
        }
    }
    names.sort();
    let take = 3.min(names.len());
    names[names.len() - take..].to_vec()
}
