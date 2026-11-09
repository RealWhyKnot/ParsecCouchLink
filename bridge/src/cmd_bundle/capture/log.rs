use crate::pico_cache;
use chrono::Local;
use std::time::Instant;

#[derive(Default)]
pub(in crate::cmd_bundle) struct CaptureLog {
    pub(in crate::cmd_bundle) lines: Vec<String>,
}

impl CaptureLog {
    pub(in crate::cmd_bundle) fn record_duration(
        &mut self,
        step: impl AsRef<str>,
        duration_ms: u64,
        status: impl AsRef<str>,
        bytes: usize,
        reason: impl AsRef<str>,
    ) {
        let step = sanitize_log_field(step.as_ref());
        let status = sanitize_log_field(status.as_ref());
        let reason = sanitize_log_field(reason.as_ref());
        tracing::debug!(
            "bundle-capture: step={} duration_ms={} status={} bytes={} reason={}",
            step,
            duration_ms,
            status,
            bytes,
            reason
        );
        self.lines.push(format!(
            "{}\t{step}\t{duration_ms}\t{status}\t{bytes}\t{reason}",
            Local::now().to_rfc3339()
        ));
    }

    pub(in crate::cmd_bundle) fn record(
        &mut self,
        step: impl AsRef<str>,
        started: Instant,
        status: impl AsRef<str>,
        bytes: usize,
        reason: impl AsRef<str>,
    ) {
        let duration_ms = pico_cache::duration_ms(started.elapsed());
        self.record_duration(step, duration_ms, status, bytes, reason);
    }

    pub(in crate::cmd_bundle) fn text(&self) -> String {
        let mut out = String::from("captured_at\tstep\tduration_ms\tstatus\tbytes\treason\n");
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

pub(super) fn sanitize_log_field(value: &str) -> String {
    value.replace(['\r', '\n', '\t'], " ").trim().to_string()
}
