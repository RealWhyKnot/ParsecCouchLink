use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tokio::io::AsyncBufReadExt;

use crate::config;

pub(crate) async fn run(tail: bool) -> Result<()> {
    let dir = config::log_dir()?;
    outln!("log directory: {}", dir.display());

    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("couchlink"))
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort();

    if entries.is_empty() {
        outln!("(no log files yet)");
        return Ok(());
    }

    for e in &entries {
        if let Some(name) = e.file_name() {
            outln!("  {}", name.to_string_lossy());
        }
    }

    if tail {
        let Some(active) = entries.last().cloned() else {
            return Ok(());
        };
        outln!();
        outln!("tailing {} (Ctrl-C to stop)", active.display());
        outln!();
        tail_file(&active).await?;
    }
    Ok(())
}

async fn tail_file(path: &PathBuf) -> Result<()> {
    let f = tokio::fs::File::open(path).await?;
    let mut r = tokio::io::BufReader::new(f);
    let mut line = String::new();
    loop {
        line.clear();
        let n = r.read_line(&mut line).await?;
        if n == 0 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }
        out!("{}", line);
    }
}
