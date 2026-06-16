//! Filesystem collection helpers: rotated logs, crash dumps, and setup
//! transcript discovery for the bundle zip.

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::config;

use super::redact::redact_bundle_text;

pub(super) const BUNDLE_LOG_FILES_PER_PREFIX: usize = 5;

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
    let take = BUNDLE_LOG_FILES_PER_PREFIX.min(paths.len());
    let recent = &paths[paths.len() - take..];
    for p in recent {
        let Some(name) = p.file_name() else { continue };
        match std::fs::read(p) {
            Ok(bytes) => {
                zip.start_file(format!("logs/{}", name.to_string_lossy()), opts)?;
                let text = String::from_utf8_lossy(&bytes);
                zip.write_all(redact_bundle_text(&text).as_bytes())?;
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
    let take = BUNDLE_LOG_FILES_PER_PREFIX.min(names.len());
    names[names.len() - take..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_log_prefix_includes_five_recent_logs() {
        let root =
            std::env::temp_dir().join(format!("couchlink-log-bundle-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for day in 1..=7 {
            let path = root.join(format!("couchlink.2026-06-{day:02}.log"));
            std::fs::write(path, format!("log day {day}\n")).unwrap();
        }
        std::fs::write(root.join("other.2026-06-08.log"), "ignored\n").unwrap();

        let zip_path = root.join("bundle.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        bundle_log_prefix(&root, "couchlink.", &mut zip, opts).unwrap();
        zip.finish().unwrap();

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        names.sort();
        assert_eq!(
            names,
            vec![
                "logs/couchlink.2026-06-03.log",
                "logs/couchlink.2026-06-04.log",
                "logs/couchlink.2026-06-05.log",
                "logs/couchlink.2026-06-06.log",
                "logs/couchlink.2026-06-07.log",
            ]
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
