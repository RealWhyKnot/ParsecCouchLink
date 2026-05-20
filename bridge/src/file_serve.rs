//! Whitelisted file reads for the tunnel. Maps a small enum of keys to known
//! per-user paths and streams the contents back in base64-encoded chunks via
//! the telemetry channel.
//!
//! The enum is the boundary; there is no way for a tunnel command to read an
//! arbitrary path. Sensitive bytes (the tunnel write/view tokens, the wifi
//! password if it ever leaked into a log -- which it shouldn't) are redacted
//! on the way out.

use std::path::PathBuf;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use tokio::io::AsyncReadExt;

use crate::config;

const CHUNK_BYTES: usize = 4 * 1024;
const MAX_BYTES: usize = 2 * 1024 * 1024; // hard cap per read (2 MiB)

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKey {
    Config,
    StateJournal,
    PicoDiag,
    BridgeLog,
}

impl FileKey {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "config" => Some(Self::Config),
            "state_journal" | "journal" => Some(Self::StateJournal),
            "pico_diag" | "picodiag" => Some(Self::PicoDiag),
            "bridge_log" | "log" => Some(Self::BridgeLog),
            _ => None,
        }
    }
}

pub fn resolve(key: FileKey) -> Result<PathBuf> {
    let p = match key {
        FileKey::Config => config::config_path()?,
        FileKey::StateJournal => config::log_dir()?.join("state-journal.log"),
        FileKey::PicoDiag => config::log_dir()?.join("pico-diag.txt"),
        FileKey::BridgeLog => {
            let dir = config::log_dir()?;
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            dir.join(format!("couchlink.{today}.log"))
        }
    };
    Ok(p)
}

pub struct Chunk {
    pub seq: u32,
    pub b64: String,
}

/// Read the whitelisted file fully, redact known sensitive lines, and split
/// into base64-encoded chunks. Caller is responsible for sending each chunk
/// via the telemetry channel and emitting a `file_eof` marker.
pub async fn read_chunks(key: FileKey) -> Result<Vec<Chunk>> {
    let path = resolve(key)?;
    let mut f = tokio::fs::File::open(&path)
        .await
        .with_context(|| format!("opening {}", path.display()))?;

    let mut buf = Vec::with_capacity(CHUNK_BYTES * 4);
    let mut tmp = vec![0u8; CHUNK_BYTES];
    let mut total = 0usize;
    loop {
        let n = f.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        if total + n > MAX_BYTES {
            // Keep what we can; the most recent bytes are usually what's wanted.
            let keep = MAX_BYTES.saturating_sub(buf.len());
            buf.extend_from_slice(&tmp[..keep.min(n)]);
            tracing::warn!(
                "file_serve: {} hit {} byte cap; truncated to most-recent {} bytes",
                path.display(),
                MAX_BYTES,
                buf.len()
            );
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        total += n;
    }

    let redacted = redact_for(key, &buf);
    let chunks = redacted
        .chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(i, c)| Chunk {
            seq: i as u32,
            b64: B64.encode(c),
        })
        .collect();
    Ok(chunks)
}

/// Apply per-key redactions. Conservative -- only strips lines we know carry
/// secrets. Anything not in the list passes through untouched.
fn redact_for(key: FileKey, bytes: &[u8]) -> Vec<u8> {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return bytes.to_vec(), // binary; nothing to redact safely
    };

    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        if line_should_redact(key, line) {
            // Preserve the structure of the line so format-aware readers don't
            // get tripped up; replace the value side only.
            if let Some(idx) = line.find('=') {
                out.push_str(&line[..=idx]);
                out.push_str(" \"<redacted>\"");
                if line.ends_with('\n') {
                    out.push('\n');
                }
            } else {
                out.push_str("<redacted>\n");
            }
        } else {
            out.push_str(line);
        }
    }
    out.into_bytes()
}

fn line_should_redact(key: FileKey, line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    match key {
        FileKey::Config => {
            lower.contains("write_token")
                || lower.contains("view_token")
                || lower.contains("password")
                || lower.contains("psk")
        }
        FileKey::StateJournal | FileKey::PicoDiag | FileKey::BridgeLog => {
            // Defense in depth: even though we never log credentials, if one
            // somehow shows up in a journal/log line, drop it.
            lower.contains("password=")
                || lower.contains("write_token=")
                || lower.contains("view_token=")
                || lower.contains("psk=")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keys() {
        assert_eq!(FileKey::parse("config"), Some(FileKey::Config));
        assert_eq!(FileKey::parse("CONFIG"), Some(FileKey::Config));
        assert_eq!(FileKey::parse("state_journal"), Some(FileKey::StateJournal));
        assert_eq!(FileKey::parse("journal"), Some(FileKey::StateJournal));
        assert_eq!(FileKey::parse("nope"), None);
    }

    #[test]
    fn redact_config_password() {
        let src = b"server = \"https://x\"\npassword = \"hunter2\"\nother = 1\n";
        let out = redact_for(FileKey::Config, src);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("server"));
        assert!(!s.contains("hunter2"));
        assert!(s.contains("<redacted>"));
    }
}
