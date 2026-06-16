use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Local;

use crate::{config, net, protocol};

pub(crate) const DEBUG_PACKET_FILE_RETENTION: usize = 5;

#[derive(Clone, Debug)]
pub(crate) struct DiagLogSnapshot {
    pub text: String,
    pub lost_bytes: u32,
    pub chunk_count: usize,
}

pub(crate) struct DebugPacketSink {
    path: PathBuf,
    file: File,
    seen: HashSet<String>,
    total_written: usize,
}

impl DebugPacketSink {
    pub(crate) fn create(uid: &str, peer: SocketAddr) -> Result<Self> {
        let dir = packet_log_dir()?;
        Self::create_in(&dir, uid, peer)
    }

    fn create_in(dir: &Path, uid: &str, peer: SocketAddr) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        prune_packet_files_in(dir, DEBUG_PACKET_FILE_RETENTION.saturating_sub(1));
        let now = Local::now();
        let stamp = now.format("%Y%m%d-%H%M%S-%3f");
        let uid = sanitize_filename(uid);
        let path = dir.join(format!("usb-packets-{stamp}-{uid}.log"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        writeln!(file, "# CouchLink debug input USB packet capture")?;
        writeln!(file, "# started_at={}", now.to_rfc3339())?;
        writeln!(file, "# uid={uid}")?;
        writeln!(file, "# peer={peer}")?;
        writeln!(
            file,
            "# source=stream debug-input periodic Pico diag-log harvest"
        )?;
        writeln!(file)?;
        Ok(Self {
            path,
            file,
            seen: HashSet::new(),
            total_written: 0,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn total_written(&self) -> usize {
        self.total_written
    }

    pub(crate) fn append_lines(&mut self, lines: &[String]) -> Result<usize> {
        let mut written = 0usize;
        for line in lines {
            if self.seen.insert(line.clone()) {
                writeln!(self.file, "{line}")?;
                written += 1;
                self.total_written += 1;
            }
        }
        if written > 0 {
            self.file.flush()?;
        }
        Ok(written)
    }
}

pub(crate) fn packet_log_dir() -> Result<PathBuf> {
    Ok(config::log_dir()?.join("debug-packets"))
}

pub(crate) fn recent_packet_files(limit: usize) -> Vec<PathBuf> {
    let Ok(dir) = packet_log_dir() else {
        return Vec::new();
    };
    recent_packet_files_in(&dir, limit)
}

pub(crate) fn recent_packet_files_in(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_packet_file(path))
        .collect();
    paths.sort();
    let take = limit.min(paths.len());
    paths[paths.len() - take..].to_vec()
}

pub(crate) fn prune_packet_files() {
    let Ok(dir) = packet_log_dir() else {
        return;
    };
    prune_packet_files_in(&dir, DEBUG_PACKET_FILE_RETENTION);
}

fn prune_packet_files_in(dir: &Path, keep: usize) {
    let paths = recent_packet_files_in(dir, usize::MAX);
    let remove_count = paths.len().saturating_sub(keep);
    for path in paths.into_iter().take(remove_count) {
        if let Err(e) = fs::remove_file(&path) {
            tracing::debug!("debug-packets: could not remove {}: {e}", path.display());
        }
    }
}

pub(crate) fn extract_usb_packet_lines(diag_text: &str) -> Vec<String> {
    diag_text
        .lines()
        .filter_map(|line| {
            line.find("usb-packet ")
                .or_else(|| line.find("usb-packet-stats "))
                .map(|idx| line[idx..].to_string())
        })
        .collect()
}

pub(crate) async fn capture_run_diag_log(
    peer: SocketAddr,
    timeout: Duration,
) -> Result<DiagLogSnapshot> {
    let socket = net::bind_udp("0.0.0.0:0")
        .await
        .context("binding UDP debug packet harvest socket")?;
    let started = Instant::now();
    let req = protocol::encode_get_log(0);
    socket
        .send_to(&req, peer)
        .await
        .with_context(|| format!("sending GET_LOG to {peer}"))?;

    let mut chunks: BTreeMap<u8, protocol::LogChunk> = BTreeMap::new();
    let mut got_last = false;
    let mut buf = [0u8; 1024];
    let deadline = started + timeout;
    while !got_last {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero());
        let Some(remaining) = remaining else { break };
        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if from != peer {
                    tracing::trace!("debug-packets: dropped packet from non-target {from}");
                    continue;
                }
                match protocol::LogChunk::decode(&buf[..n]) {
                    Ok(chunk) => {
                        got_last = got_last || chunk.is_last();
                        chunks.insert(chunk.chunk_index, chunk);
                    }
                    Err(e) => {
                        tracing::debug!("debug-packets: malformed log chunk from {from}: {e}");
                    }
                }
            }
            Ok(Err(e)) => return Err(e).context("receiving log chunk"),
            Err(_) => break,
        }
    }

    if chunks.is_empty() {
        anyhow::bail!(
            "no log chunks received from {peer} within {} ms",
            timeout.as_millis()
        );
    }

    let lost_bytes = chunks.get(&0).map(|chunk| chunk.lost_bytes).unwrap_or(0);
    let mut bytes = Vec::new();
    for chunk in chunks.values() {
        bytes.extend_from_slice(&chunk.payload);
    }
    Ok(DiagLogSnapshot {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        lost_bytes,
        chunk_count: chunks.len(),
    })
}

fn is_packet_file(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("usb-packets-") && name.ends_with(".log"))
            .unwrap_or(false)
}

fn sanitize_filename(value: &str) -> String {
    let out: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "couchlink-debug-packets-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn peer() -> SocketAddr {
        "10.0.0.24:4242".parse().unwrap()
    }

    #[test]
    fn extracts_packet_lines_from_timestamped_diag() {
        let text = "[  10] boot\n[  11] usb-packet seq=7 dir=out data=0102\nplain usb-packet seq=8 dir=in data=03\n[  12] usb-packet seq=9 dir=setup data=C020000000000040\n[  13] usb-packet-stats total=64 in=10 out=50\n";
        assert_eq!(
            extract_usb_packet_lines(text),
            vec![
                "usb-packet seq=7 dir=out data=0102".to_string(),
                "usb-packet seq=8 dir=in data=03".to_string(),
                "usb-packet seq=9 dir=setup data=C020000000000040".to_string(),
                "usb-packet-stats total=64 in=10 out=50".to_string(),
            ]
        );
    }

    #[test]
    fn sink_dedupes_packet_lines() {
        let root = temp_root("sink");
        let mut sink = DebugPacketSink::create_in(&root, "02:E2/2D\\A9", peer()).unwrap();
        assert!(sink
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap()
            .contains("02E22DA9"));
        let lines = vec![
            "usb-packet seq=1 dir=out data=01".to_string(),
            "usb-packet seq=1 dir=out data=01".to_string(),
            "usb-packet seq=2 dir=out data=02".to_string(),
        ];
        assert_eq!(sink.append_lines(&lines).unwrap(), 2);
        assert_eq!(sink.total_written(), 2);
        drop(sink);
        let text = fs::read_to_string(recent_packet_files_in(&root, 1)[0].clone()).unwrap();
        assert_eq!(text.matches("usb-packet seq=1").count(), 1);
        assert_eq!(text.matches("usb-packet seq=2").count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retained_packet_files_keep_newest_five() {
        let root = temp_root("retention");
        for idx in 1..=7 {
            fs::write(
                root.join(format!("usb-packets-20260615-00000{idx}-UID.log")),
                "packet\n",
            )
            .unwrap();
        }
        fs::write(root.join("other.log"), "ignored\n").unwrap();
        prune_packet_files_in(&root, 5);
        let names: Vec<String> = recent_packet_files_in(&root, 10)
            .into_iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "usb-packets-20260615-000003-UID.log",
                "usb-packets-20260615-000004-UID.log",
                "usb-packets-20260615-000005-UID.log",
                "usb-packets-20260615-000006-UID.log",
                "usb-packets-20260615-000007-UID.log",
            ]
        );
        let _ = fs::remove_dir_all(root);
    }
}
