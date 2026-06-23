use std::path::PathBuf;

use serde::Serialize;

use crate::{cmd_flash, cmd_run};

use super::{LabOptions, LabPower, LabScenario, SetupLabProbe};

#[derive(Clone, Debug, Serialize)]
pub(in crate::cmd_lab) struct LabReport {
    started_utc: String,
    scenario: LabScenario,
    all: bool,
    cycles: u32,
    power_requested: LabPower,
    pub(in crate::cmd_lab) power_selected: String,
    no_flash: bool,
    pub(in crate::cmd_lab) steps: Vec<LabStep>,
    pub(in crate::cmd_lab) devices: Vec<LabDevice>,
}

impl LabReport {
    pub(in crate::cmd_lab) fn new(options: &LabOptions) -> Self {
        Self {
            started_utc: chrono::Utc::now().to_rfc3339(),
            scenario: options.scenario,
            all: options.all,
            cycles: options.cycles,
            power_requested: options.power,
            power_selected: "unknown".to_string(),
            no_flash: options.no_flash,
            steps: Vec::new(),
            devices: Vec::new(),
        }
    }

    pub(in crate::cmd_lab) fn pass(
        &mut self,
        name: impl Into<String>,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.push_step(name, StepStatus::Pass, uid, detail, elapsed_ms);
    }

    pub(in crate::cmd_lab) fn fail(
        &mut self,
        name: impl Into<String>,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.push_step(name, StepStatus::Fail, uid, detail, elapsed_ms);
    }

    pub(in crate::cmd_lab) fn skip(
        &mut self,
        name: impl Into<String>,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.push_step(name, StepStatus::Skip, uid, detail, elapsed_ms);
    }

    fn push_step(
        &mut self,
        name: impl Into<String>,
        status: StepStatus,
        uid: Option<u32>,
        detail: impl Into<String>,
        elapsed_ms: u128,
    ) {
        self.steps.push(LabStep {
            name: name.into(),
            status,
            uid: uid.map(|uid| format!("{uid:08X}")),
            detail: detail.into(),
            elapsed_ms,
        });
    }

    pub(in crate::cmd_lab) fn fail_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == StepStatus::Fail)
            .count()
    }

    pub(in crate::cmd_lab) fn print_summary(&self) {
        println!();
        println!("Lab summary");
        for step in &self.steps {
            let uid = step
                .uid
                .as_ref()
                .map(|u| format!(" {u}"))
                .unwrap_or_default();
            println!(
                "  {:<4} {:<24}{}  {}",
                step.status.as_str(),
                step.name,
                uid,
                step.detail
            );
        }
        let pass = self
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Pass)
            .count();
        let skip = self
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Skip)
            .count();
        println!(
            "summary: {} pass, {} fail, {} skip",
            pass,
            self.fail_count(),
            skip
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(in crate::cmd_lab) enum StepStatus {
    Pass,
    Fail,
    Skip,
}

impl StepStatus {
    fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Pass => "PASS",
            StepStatus::Fail => "FAIL",
            StepStatus::Skip => "SKIP",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(in crate::cmd_lab) struct LabStep {
    name: String,
    status: StepStatus,
    pub(in crate::cmd_lab) uid: Option<String>,
    detail: String,
    elapsed_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
pub(in crate::cmd_lab) struct LabDevice {
    pub(in crate::cmd_lab) mode: String,
    pub(in crate::cmd_lab) uid: Option<String>,
    pub(in crate::cmd_lab) board: Option<String>,
    pub(in crate::cmd_lab) address: Option<String>,
    pub(in crate::cmd_lab) detail: Option<String>,
}

impl LabDevice {
    pub(in crate::cmd_lab) fn from_setup_probe(probe: &SetupLabProbe) -> Self {
        Self {
            mode: "setup-usb".to_string(),
            uid: Some(probe.uid_hex()),
            board: Some(probe.board_label().to_string()),
            address: Some(probe.port.clone()),
            detail: Some(format!(
                "fw v{} creds={} self_test={} log_bytes={} lost={}",
                probe.hello.firmware_version(),
                if probe.hello.creds_present() {
                    "present"
                } else {
                    "absent"
                },
                if probe.self_test.passed {
                    "pass"
                } else {
                    "fail"
                },
                probe.log_bytes,
                probe.log_lost
            )),
        }
    }

    pub(in crate::cmd_lab) fn from_wifi_target(target: &cmd_run::PicoTarget) -> Self {
        Self {
            mode: "wifi-run".to_string(),
            uid: Some(format!("{:08X}", target.info.unique_id_short)),
            board: Some(target.board_label().to_string()),
            address: Some(target.peer.to_string()),
            detail: Some(format!(
                "fw v{} uptime={}s",
                target.info.firmware_version(),
                target.info.uptime_seconds
            )),
        }
    }

    pub(in crate::cmd_lab) fn from_bootsel(
        (mount, board): &(PathBuf, cmd_flash::BootselBoard),
    ) -> Self {
        Self {
            mode: "bootsel".to_string(),
            uid: None,
            board: Some(board.label().to_string()),
            address: Some(mount.display().to_string()),
            detail: Some("ROM bootloader does not expose CouchLink UID".to_string()),
        }
    }
}
