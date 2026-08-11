use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::ValueEnum;

const PNP_RECONNECT_HOLD: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum PnpHelperAction {
    DisableEnable,
    RemoveRescan,
}

impl PnpHelperAction {
    fn cli_value(self) -> &'static str {
        match self {
            PnpHelperAction::DisableEnable => "disable-enable",
            PnpHelperAction::RemoveRescan => "remove-rescan",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PnpHelperAction::DisableEnable => "PnP disable/enable",
            PnpHelperAction::RemoveRescan => "PnP remove/rescan",
        }
    }
}

pub fn run_pnp_helper(
    action: PnpHelperAction,
    instance_ids: Vec<String>,
    hold_seconds: u64,
    result_file: Option<PathBuf>,
) -> Result<()> {
    let result = (|| {
        let ids = validate_pnp_instance_ids(&instance_ids)?;
        run_pnp_action(&ids, action, Duration::from_secs(hold_seconds), true)
    })();
    if let Some(path) = result_file {
        let text = match &result {
            Ok(()) => "ok\n".to_string(),
            Err(e) => format!("error: {e:#}\n"),
        };
        let _ = fs::write(path, text);
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::cmd_lab) enum PnpPersona {
    Setup,
    Xinput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::cmd_lab) struct PnpInstance {
    pub(in crate::cmd_lab) uid: u32,
    pub(in crate::cmd_lab) persona: PnpPersona,
    pub(in crate::cmd_lab) instance_id: String,
}

pub(in crate::cmd_lab) fn pnp_instances_for_uids(
    uids: &BTreeSet<u32>,
    personas: &[PnpPersona],
) -> Result<Vec<PnpInstance>> {
    let output = Command::new("pnputil")
        .args(["/enum-devices", "/connected", "/ids"])
        .output()
        .context("starting pnputil /enum-devices")?;
    if !output.status.success() {
        bail!(
            "pnputil /enum-devices failed with {}: {}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_pico_pnp_instances(&text)
        .into_iter()
        .filter(|instance| uids.contains(&instance.uid) && personas.contains(&instance.persona))
        .collect())
}

pub(in crate::cmd_lab) fn pnp_instance_ids(instances: &[PnpInstance]) -> Vec<String> {
    instances
        .iter()
        .map(|instance| instance.instance_id.clone())
        .collect()
}

pub(in crate::cmd_lab) fn parse_pico_pnp_instances(text: &str) -> Vec<PnpInstance> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let id = trimmed
                .strip_prefix("Instance ID:")
                .or_else(|| trimmed.strip_prefix("Instance Id:"))?
                .trim();
            pnp_instance_from_id(id)
        })
        .collect()
}

fn pnp_instance_from_id(id: &str) -> Option<PnpInstance> {
    let upper = id.to_ascii_uppercase();
    let (persona, serial) = if let Some(serial) = upper.strip_prefix(r"USB\VID_2E8A&PID_CAF0\") {
        (PnpPersona::Setup, serial)
    } else {
        let serial = upper.strip_prefix(r"USB\VID_045E&PID_028E\")?;
        (PnpPersona::Xinput, serial)
    };
    let uid = uid_from_usb_serial(serial)?;
    Some(PnpInstance {
        uid,
        persona,
        instance_id: id.to_string(),
    })
}

pub(in crate::cmd_lab) fn uid_from_usb_serial(serial: &str) -> Option<u32> {
    if serial.len() < 8 {
        return None;
    }
    let mut bytes = [0u8; 4];
    for (idx, byte) in bytes.iter_mut().enumerate() {
        let start = idx * 2;
        *byte = u8::from_str_radix(&serial[start..start + 2], 16).ok()?;
    }
    Some(u32::from_le_bytes(bytes))
}

pub(in crate::cmd_lab) fn validate_pnp_instance_ids(
    instance_ids: &[String],
) -> Result<Vec<String>> {
    if instance_ids.is_empty() {
        bail!("no PnP instance IDs supplied");
    }
    let mut validated = Vec::with_capacity(instance_ids.len());
    for id in instance_ids {
        if pnp_instance_from_id(id).is_none() {
            bail!("refusing to touch non-CouchLink Pico PnP instance ID: {id}");
        }
        validated.push(id.clone());
    }
    Ok(validated)
}

pub(in crate::cmd_lab) fn pnp_disable_enable_with_elevation(instance_ids: &[String]) -> Result<()> {
    run_pnp_action_with_elevation(instance_ids, PnpHelperAction::DisableEnable)
}

pub(in crate::cmd_lab) fn pnp_remove_rescan_with_elevation(instance_ids: &[String]) -> Result<()> {
    run_pnp_action_with_elevation(instance_ids, PnpHelperAction::RemoveRescan)
}

fn run_pnp_action_with_elevation(instance_ids: &[String], action: PnpHelperAction) -> Result<()> {
    let ids = validate_pnp_instance_ids(instance_ids)?;
    match run_pnp_action(&ids, action, PNP_RECONNECT_HOLD, false) {
        Ok(()) => Ok(()),
        Err(first) => match run_elevated_pnp_helper(&ids, action) {
            Ok(()) => Ok(()),
            Err(elevated) => bail!(
                "{} failed without elevation ({first:#}); elevated helper also failed ({elevated:#})",
                action.label()
            ),
        },
    }
}

fn run_pnp_action(
    instance_ids: &[String],
    action: PnpHelperAction,
    hold: Duration,
    allow_force: bool,
) -> Result<()> {
    match action {
        PnpHelperAction::DisableEnable => run_pnp_disable_enable(instance_ids, hold, allow_force),
        PnpHelperAction::RemoveRescan => run_pnp_remove_rescan(instance_ids, hold, allow_force),
    }
}

fn run_pnp_disable_enable(
    instance_ids: &[String],
    hold: Duration,
    allow_force_disable: bool,
) -> Result<()> {
    if instance_ids.is_empty() {
        bail!("no PnP instance IDs supplied");
    }

    let mut disabled = Vec::new();
    let mut first_error = None;
    for id in instance_ids {
        match run_pnputil_device_action("/disable-device", id, allow_force_disable) {
            Ok(()) => disabled.push(id.clone()),
            Err(e) => {
                let _ = run_pnputil_device_action("/enable-device", id, false);
                first_error = Some(e);
                break;
            }
        }
    }

    if let Some(e) = first_error {
        for id in disabled.iter().rev() {
            let _ = run_pnputil_device_action("/enable-device", id, false);
        }
        return Err(e);
    }

    thread::sleep(hold);

    for id in disabled.iter().rev() {
        run_pnputil_device_action("/enable-device", id, false)?;
    }
    Ok(())
}

fn run_pnp_remove_rescan(instance_ids: &[String], hold: Duration, allow_force: bool) -> Result<()> {
    if instance_ids.is_empty() {
        bail!("no PnP instance IDs supplied");
    }

    let result = (|| {
        for id in instance_ids {
            run_pnputil_remove_device(id, allow_force)?;
        }
        Ok::<_, anyhow::Error>(())
    })();

    thread::sleep(hold);
    let scan = run_pnputil_scan_devices();
    result?;
    scan?;
    Ok(())
}

fn run_pnputil_device_action(action: &str, instance_id: &str, allow_force: bool) -> Result<()> {
    match run_pnputil_device_action_once(action, instance_id, false) {
        Ok(()) => Ok(()),
        Err(first) if allow_force && action == "/disable-device" => {
            run_pnputil_device_action_once(action, instance_id, true)
                .with_context(|| format!("normal {action} failed first: {first:#}"))
        }
        Err(e) => Err(e),
    }
}

fn run_pnputil_device_action_once(action: &str, instance_id: &str, force: bool) -> Result<()> {
    let mut args = vec![action, instance_id];
    if force {
        args.push("/force");
    }
    let output = Command::new("pnputil")
        .args(&args)
        .output()
        .with_context(|| format!("starting pnputil {action}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    if !output.status.success() || pnputil_output_failed(&combined) {
        bail!(
            "pnputil {action} {} failed with {}: {}",
            instance_id,
            output.status,
            combined.trim()
        );
    }
    Ok(())
}

fn run_pnputil_remove_device(instance_id: &str, allow_force: bool) -> Result<()> {
    match run_pnputil_remove_device_once(instance_id, false) {
        Ok(()) => Ok(()),
        Err(first) if allow_force => run_pnputil_remove_device_once(instance_id, true)
            .with_context(|| format!("normal /remove-device failed first: {first:#}")),
        Err(e) => Err(e),
    }
}

fn run_pnputil_remove_device_once(instance_id: &str, force: bool) -> Result<()> {
    let args = pnputil_remove_device_args(instance_id, force);
    let output = Command::new("pnputil")
        .args(&args)
        .output()
        .context("starting pnputil /remove-device")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    if !output.status.success() || pnputil_output_failed(&combined) {
        bail!(
            "pnputil /remove-device {} failed with {}: {}",
            instance_id,
            output.status,
            combined.trim()
        );
    }
    Ok(())
}

pub(in crate::cmd_lab) fn pnputil_remove_device_args(
    instance_id: &str,
    force: bool,
) -> Vec<String> {
    let mut args = vec![
        "/remove-device".to_string(),
        instance_id.to_string(),
        "/subtree".to_string(),
    ];
    if force {
        args.push("/force".to_string());
    }
    args
}

fn run_pnputil_scan_devices() -> Result<()> {
    let output = Command::new("pnputil")
        .args(["/scan-devices"])
        .output()
        .context("starting pnputil /scan-devices")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    if !output.status.success() || pnputil_output_failed(&combined) {
        bail!(
            "pnputil /scan-devices failed with {}: {}",
            output.status,
            combined.trim()
        );
    }
    Ok(())
}

pub(in crate::cmd_lab) fn pnputil_output_failed(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("access is denied")
        || lower.contains("generic failure")
        || lower.contains("failed to ")
        || lower.contains("failed.")
}

fn run_elevated_pnp_helper(instance_ids: &[String], action: PnpHelperAction) -> Result<()> {
    let exe = env::current_exe().context("locating current executable")?;
    let result_file = env::temp_dir().join(format!(
        "couchlink-pnp-helper-{}-{}.txt",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    let mut args = vec![
        "--no-log-file".to_string(),
        "lab-pnp-helper".to_string(),
        "--action".to_string(),
        action.cli_value().to_string(),
        "--hold-seconds".to_string(),
        PNP_RECONNECT_HOLD.as_secs().to_string(),
        "--result-file".to_string(),
        result_file.display().to_string(),
    ];
    for id in instance_ids {
        args.push("--instance-id".to_string());
        args.push(id.clone());
    }

    let script = format!(
        "$p = Start-Process -FilePath {} -ArgumentList @({}) -Verb RunAs -Wait -PassThru; if ($null -eq $p) {{ exit 1 }}; exit $p.ExitCode",
        powershell_quote(&exe.display().to_string()),
        args.iter()
            .map(|arg| powershell_quote(arg))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
        .context("starting elevated PnP helper")?;
    if !status.success() {
        let helper_result = fs::read_to_string(&result_file)
            .unwrap_or_else(|e| format!("could not read helper result file: {e}"));
        bail!(
            "elevated PnP helper exited with {status}; helper result: {}",
            helper_result.trim()
        );
    }
    let _ = fs::remove_file(&result_file);
    Ok(())
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(in crate::cmd_lab) fn problem_usb_devices() -> Vec<String> {
    #[cfg(windows)]
    {
        let output = Command::new("pnputil")
            .args(["/enum-devices", "/connected", "/problem", "/ids"])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&output.stdout);
        parse_problem_usb_devices(&text)
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub(in crate::cmd_lab) fn parse_problem_usb_devices(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            let upper = line.to_ascii_uppercase();
            upper.contains("VID_2E8A&PID_CAF0") || upper.contains("VID_045E&PID_028E")
        })
        .map(|line| line.trim().to_string())
        .collect()
}
