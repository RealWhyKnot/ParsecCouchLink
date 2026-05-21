//! Tunnel exec allowlist. The bridge will only spawn child processes whose
//! `argv[0]` resolves (by basename, case-insensitive on Windows) to one of the
//! entries here.
//!
//! Default list intentionally excludes shells, package managers, and editors.
//! The host can extend it by editing
//! `%APPDATA%\ParsecCouchLink\exec_allowlist.toml` and restarting the bridge.
//!
//! Resolution rule: the literal name `couchlink` is rewritten to
//! `std::env::current_exe()` so the tunnel always runs the same binary that's
//! hosting it -- no `PATH` ambiguity, no reliance on a separate install.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config;

const FILE_NAME: &str = "exec_allowlist.toml";

const DEFAULT_ENTRIES: &[&str] = &["couchlink", "cmake", "ninja", "dir", "ls", "type", "cat"];

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OnDisk {
    allowed: Vec<String>,
}

/// In-memory allowlist. Built once at bridge start, shared by every exec
/// request. Cheap to clone via Arc on the caller side.
#[derive(Clone, Debug)]
pub struct Allowlist {
    set: BTreeSet<String>,
}

impl Allowlist {
    pub fn defaults() -> Self {
        Self {
            set: DEFAULT_ENTRIES.iter().map(|s| normalize(s)).collect(),
        }
    }

    /// Load from `%APPDATA%\ParsecCouchLink\exec_allowlist.toml`, falling back
    /// to defaults if the file is absent. Writes a default file on first use
    /// so the host knows where to edit.
    pub fn load_or_default() -> Self {
        let Ok(path) = allowlist_path() else {
            return Self::defaults();
        };
        if !path.exists() {
            let _ = write_default_file(&path);
            return Self::defaults();
        }
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<OnDisk>(&s) {
                Ok(disk) => Self {
                    set: disk.allowed.iter().map(|x| normalize(x)).collect(),
                },
                Err(e) => {
                    tracing::warn!(
                        "exec_allowlist: {} did not parse ({e}); using defaults",
                        path.display()
                    );
                    Self::defaults()
                }
            },
            Err(e) => {
                tracing::warn!(
                    "exec_allowlist: could not read {} ({e}); using defaults",
                    path.display()
                );
                Self::defaults()
            }
        }
    }

    /// Decide whether `argv[0]` may be spawned. On allow, returns the path the
    /// runner should actually execute. On deny, returns `None`.
    ///
    /// The literal name `couchlink` resolves to the current executable so a
    /// tunnel-driven invocation always points at the same binary hosting the
    /// tunnel.
    pub fn resolve(&self, argv0: &str) -> Option<PathBuf> {
        let key = normalize(argv0);
        if !self.set.contains(&key) {
            return None;
        }
        if key == "couchlink" {
            if let Ok(p) = std::env::current_exe() {
                return Some(p);
            }
        }
        Some(PathBuf::from(argv0))
    }

    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.set.iter().map(|s| s.as_str())
    }
}

fn allowlist_path() -> Result<PathBuf> {
    Ok(config::config_dir()?.join(FILE_NAME))
}

fn write_default_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let disk = OnDisk {
        allowed: DEFAULT_ENTRIES.iter().map(|s| s.to_string()).collect(),
    };
    let s = toml::to_string_pretty(&disk).context("serializing default allowlist")?;
    let header = "# CouchLink Tunnel exec allowlist.\n\
                  # `argv[0]` for any remote-exec request must basename-match an entry below.\n\
                  # Restart the bridge after editing. Defaults are conservative on purpose -- adding\n\
                  # shells (pwsh, bash, cmd) effectively grants the holder of the view URL\n\
                  # shell on this machine. Only add what you actually need.\n\n";
    std::fs::write(path, format!("{header}{s}"))
        .with_context(|| format!("writing {}", path.display()))?;
    tracing::info!("exec_allowlist: wrote default {}", path.display());
    Ok(())
}

/// Basename + lowercase. Strips an `.exe` suffix on Windows so `cmake` and
/// `cmake.exe` match the same entry.
fn normalize(s: &str) -> String {
    let basename = Path::new(s)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| s.to_string());
    let lower = basename.to_lowercase();
    lower
        .strip_suffix(".exe")
        .map(|x| x.to_string())
        .unwrap_or(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_allow_couchlink_and_cmake() {
        let a = Allowlist::defaults();
        assert!(a.resolve("couchlink").is_some());
        assert!(a.resolve("cmake").is_some());
        assert!(a.resolve("pwsh").is_none());
        assert!(a.resolve("bash").is_none());
    }

    #[test]
    fn case_and_extension_insensitive() {
        let a = Allowlist::defaults();
        assert!(a.resolve("CMake.exe").is_some());
        assert!(a
            .resolve("C:\\Program Files\\CMake\\bin\\cmake.exe")
            .is_some());
    }

    #[test]
    fn couchlink_resolves_to_current_exe() {
        let a = Allowlist::defaults();
        let resolved = a.resolve("couchlink").unwrap();
        let cur = std::env::current_exe().unwrap();
        assert_eq!(resolved, cur);
    }
}
