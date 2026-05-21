//! Spawn allowlisted child processes for the tunnel, stream their stdout +
//! stderr back as telemetry events, mirror every line to the bridge's own
//! stderr/log so the host can watch what's running.
//!
//! Live children are kept in a process-wide registry so a remote `kill`
//! command can terminate them by id.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::exec_allowlist::Allowlist;
use crate::telemetry::{
    ErrorBody, ExecExitBody, ExecLineBody, ExecStartedBody, OutEvent, TelemetryHandle,
};

struct Live {
    child: Child,
    started_at: Instant,
    #[allow(dead_code)] // kept for ad-hoc inspection of the registry
    argv: Vec<String>,
    _stdout: JoinHandle<()>,
    _stderr: JoinHandle<()>,
}

static REGISTRY: LazyLock<Mutex<HashMap<String, Live>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Spawn `argv` as a child process and stream its output as telemetry events.
///
/// The function returns as soon as the child is spawned; the actual stdio
/// pumping runs on background tasks. The exit event is emitted by the wait
/// task when the child finishes.
pub async fn spawn(
    id: String,
    argv: Vec<String>,
    cwd: Option<PathBuf>,
    allowlist: Arc<Allowlist>,
    tele: TelemetryHandle,
) {
    if argv.is_empty() {
        emit_error(&tele, &id, "exec called with empty argv");
        return;
    }
    let argv0 = &argv[0];
    let Some(resolved) = allowlist.resolve(argv0) else {
        emit_error(
            &tele,
            &id,
            format!("argv[0] '{argv0}' not in exec allowlist"),
        );
        emit_exit(&tele, &id, 126, 0);
        return;
    };

    let mut cmd = Command::new(&resolved);
    cmd.args(&argv[1..]);
    if let Some(ref c) = cwd {
        cmd.current_dir(c);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(Stdio::null());

    // On Windows, avoid spawning a console window for child GUI-less tools.
    set_no_console_window(&mut cmd);

    let started_at = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            emit_error(
                &tele,
                &id,
                format!("spawn '{}' failed: {e}", resolved.display()),
            );
            emit_exit(&tele, &id, 127, 0);
            return;
        }
    };

    // Surface the resolved exec to both the helper and the host.
    tele.publish(OutEvent::ExecStarted(ExecStartedBody {
        id: id.clone(),
        argv: argv.clone(),
        resolved_exe: resolved.display().to_string(),
        cwd: cwd.as_ref().map(|p| p.display().to_string()),
    }));
    tracing::info!(
        "tunnel exec [{id}] -> {} {}",
        resolved.display(),
        argv[1..].join(" ")
    );

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let id_for_stdout = id.clone();
    let tele_for_stdout = tele.clone();
    let stdout_task = tokio::spawn(async move {
        let Some(s) = stdout else {
            return;
        };
        let mut reader = BufReader::new(s).lines();
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    tracing::info!("[{id_for_stdout}] {line}");
                    tele_for_stdout.publish(OutEvent::ExecStdout(ExecLineBody {
                        id: id_for_stdout.clone(),
                        line,
                    }));
                }
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("tunnel exec [{id_for_stdout}] stdout read: {e}");
                    return;
                }
            }
        }
    });

    let id_for_stderr = id.clone();
    let tele_for_stderr = tele.clone();
    let stderr_task = tokio::spawn(async move {
        let Some(s) = stderr else {
            return;
        };
        let mut reader = BufReader::new(s).lines();
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    tracing::info!("[{id_for_stderr}!] {line}");
                    tele_for_stderr.publish(OutEvent::ExecStderr(ExecLineBody {
                        id: id_for_stderr.clone(),
                        line,
                    }));
                }
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("tunnel exec [{id_for_stderr}] stderr read: {e}");
                    return;
                }
            }
        }
    });

    // Insert into the registry so a remote kill can find this child. The
    // wait task below pulls the entry back out when the child exits.
    let id_for_wait = id.clone();
    let tele_for_wait = tele.clone();
    let argv_for_wait = argv.clone();
    REGISTRY.lock().unwrap().insert(
        id.clone(),
        Live {
            child,
            started_at,
            argv,
            _stdout: stdout_task,
            _stderr: stderr_task,
        },
    );

    tokio::spawn(async move {
        let entry = REGISTRY.lock().unwrap().remove(&id_for_wait);
        let Some(mut entry) = entry else {
            return; // killed before we could wait
        };
        let res = entry.child.wait().await;
        let dur = entry.started_at.elapsed().as_millis() as u64;
        match res {
            Ok(status) => {
                let code = status.code().unwrap_or(-1);
                tracing::info!(
                    "tunnel exec [{id_for_wait}] exit {code} ({dur} ms): {}",
                    argv_for_wait.join(" ")
                );
                crate::journal!("tunnel", "exec [{id_for_wait}] exit {code} ({dur} ms)");
                emit_exit(&tele_for_wait, &id_for_wait, code, dur);
            }
            Err(e) => {
                tracing::warn!("tunnel exec [{id_for_wait}] wait: {e}");
                emit_error(&tele_for_wait, &id_for_wait, format!("wait: {e}"));
                emit_exit(&tele_for_wait, &id_for_wait, -1, dur);
            }
        }
    });
}

/// Kill the child registered under `target_id`. Emits a system note via the
/// telemetry handle when the kill is applied; the exit event itself is emitted
/// by the existing wait task once the OS reports the process gone.
pub fn kill(target_id: &str, tele: &TelemetryHandle) {
    let mut reg = REGISTRY.lock().unwrap();
    if let Some(e) = reg.get_mut(target_id) {
        let pid = e.child.id();
        let _ = e.child.start_kill();
        tracing::info!("tunnel exec [{target_id}] kill issued (pid={pid:?})");
        crate::journal!("tunnel", "kill [{target_id}] (pid={pid:?})");
        tele.note(format!("kill [{target_id}] sent"));
    } else {
        tracing::info!("tunnel exec kill: no such target_id '{target_id}'");
        tele.publish(OutEvent::Error(ErrorBody {
            id: Some(target_id.to_string()),
            message: format!("kill: no running process with id '{target_id}'"),
        }));
    }
}

#[cfg(windows)]
fn set_no_console_window(cmd: &mut Command) {
    // tokio::process::Command exposes creation_flags directly on Windows.
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn set_no_console_window(_cmd: &mut Command) {}

fn emit_error(tele: &TelemetryHandle, id: &str, message: impl Into<String>) {
    let m = message.into();
    tracing::warn!("tunnel exec [{id}] error: {m}");
    tele.publish(OutEvent::Error(ErrorBody {
        id: Some(id.to_string()),
        message: m,
    }));
}

fn emit_exit(tele: &TelemetryHandle, id: &str, code: i32, dur_ms: u64) {
    tele.publish(OutEvent::ExecExit(ExecExitBody {
        id: id.to_string(),
        code,
        duration_ms: dur_ms,
    }));
}
