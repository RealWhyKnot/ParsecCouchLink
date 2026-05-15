//! `couchlink logs` -- print where logs live, or tail the latest one.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tokio::io::AsyncBufReadExt;

use crate::config;

pub async fn run(tail: bool) -> Result<()> {
    let dir = config::log_dir()?;
    println!("log directory: {}", dir.display());

    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("couchlink"))
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort();

    if entries.is_empty() {
        println!("(no log files yet)");
        return Ok(());
    }

    for e in &entries {
        if let Some(name) = e.file_name() {
            println!("  {}", name.to_string_lossy());
        }
    }

    if tail {
        let active = entries.last().unwrap().clone();
        println!();
        println!("tailing {} (Ctrl-C to stop)", active.display());
        println!();
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
        print!("{}", line);
    }
}
