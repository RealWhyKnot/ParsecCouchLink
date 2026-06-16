use std::time::{Duration, Instant};

use tokio::process::Command;

use super::manifest::ManifestHostSnapshot;
use super::redact::redact_bundle_text;
use crate::config;

const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug)]
pub(super) struct HostSnapshotFile {
    pub manifest: ManifestHostSnapshot,
    pub text: String,
    pub duration_ms: u64,
}

pub(super) async fn capture_host_snapshots() -> Vec<HostSnapshotFile> {
    let mut out = Vec::new();
    out.push(capture_redacted_config());
    for (name, path, command) in windows_snapshot_commands() {
        out.push(capture_command_snapshot(name, path, command).await);
    }
    out
}

fn capture_redacted_config() -> HostSnapshotFile {
    let path = "host/config-redacted.txt";
    let started = Instant::now();
    let (captured, status, text) = match config::config_path() {
        Ok(config_path) => match std::fs::read_to_string(&config_path) {
            Ok(text) => (
                true,
                format!("captured in {} ms", started.elapsed().as_millis()),
                format!(
                    "# config path: {}\n\n{}",
                    config_path.display(),
                    redact_bundle_text(&text)
                ),
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
                false,
                "config file not found".to_string(),
                format!(
                    "Config file was not present at bundle time.\npath={}\n",
                    config_path.display()
                ),
            ),
            Err(e) => (
                false,
                format!("read failed: {e}"),
                format!(
                    "Config file could not be read.\npath={}\nerror={e}\n",
                    config_path.display()
                ),
            ),
        },
        Err(e) => (
            false,
            format!("path lookup failed: {e:#}"),
            format!("Config path could not be resolved.\nerror={e:#}\n"),
        ),
    };
    snapshot_file(
        "redacted_config",
        path,
        captured,
        status,
        text,
        duration_ms(started.elapsed()),
    )
}

async fn capture_command_snapshot(
    name: &'static str,
    path: &'static str,
    command: &'static str,
) -> HostSnapshotFile {
    #[cfg(windows)]
    {
        let started = Instant::now();
        let child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .output();
        match tokio::time::timeout(SNAPSHOT_TIMEOUT, child).await {
            Ok(Ok(output)) => {
                let elapsed_ms = duration_ms(started.elapsed());
                let mut text = String::new();
                text.push_str(&format!("# command: {command}\n"));
                text.push_str(&format!("# exit: {}\n\n", output.status));
                text.push_str(&String::from_utf8_lossy(&output.stdout));
                if !output.stderr.is_empty() {
                    text.push_str("\n\n# stderr\n");
                    text.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                let text = redact_bundle_text(&text);
                let status = if output.status.success() {
                    format!("captured in {elapsed_ms} ms")
                } else {
                    format!(
                        "command exited with {} after {elapsed_ms} ms",
                        output.status
                    )
                };
                snapshot_file(
                    name,
                    path,
                    output.status.success(),
                    status,
                    text,
                    elapsed_ms,
                )
            }
            Ok(Err(e)) => snapshot_file(
                name,
                path,
                false,
                format!("spawn failed: {e}"),
                format!("Snapshot command could not be started.\ncommand={command}\nerror={e}\n"),
                duration_ms(started.elapsed()),
            ),
            Err(_) => snapshot_file(
                name,
                path,
                false,
                format!("timed out after {} ms", SNAPSHOT_TIMEOUT.as_millis()),
                format!(
                    "Snapshot command timed out.\ncommand={command}\ntimeout_ms={}\n",
                    SNAPSHOT_TIMEOUT.as_millis()
                ),
                duration_ms(SNAPSHOT_TIMEOUT),
            ),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = command;
        snapshot_file(
            name,
            path,
            false,
            "not supported on this OS".to_string(),
            "Windows host snapshot is not supported on this OS.\n".to_string(),
            0,
        )
    }
}

fn snapshot_file(
    name: impl Into<String>,
    path: impl Into<String>,
    captured: bool,
    status: String,
    text: String,
    duration_ms: u64,
) -> HostSnapshotFile {
    let path = path.into();
    let text = if text.is_empty() {
        "(snapshot returned no text)\n".to_string()
    } else {
        text
    };
    HostSnapshotFile {
        manifest: ManifestHostSnapshot {
            name: name.into(),
            path,
            captured,
            bytes: text.len(),
            status,
        },
        text,
        duration_ms,
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn windows_snapshot_commands() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "pnp_devices",
            "host/pnp-devices.txt",
            "Get-PnpDevice -PresentOnly | Sort-Object Class,FriendlyName | Format-Table -AutoSize Class,Status,InstanceId,FriendlyName",
        ),
        (
            "usb_hid_devices",
            "host/usb-hid-devices.txt",
            "Get-PnpDevice -PresentOnly | Where-Object { $_.Class -in @('USB','HIDClass') -or $_.InstanceId -match 'USB|HID|VID_' } | Sort-Object Class,FriendlyName | Format-Table -AutoSize Class,Status,InstanceId,FriendlyName",
        ),
        (
            "pnp_events_120m",
            "host/pnp-events-120m.txt",
            "$start = (Get-Date).AddMinutes(-120); Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=$start} -ErrorAction SilentlyContinue | Where-Object { $_.ProviderName -match 'USB|Kernel-PnP|UserPnp|DriverFrameworks' -or $_.Message -match 'USB|HID|Pico|RP2|WinUSB|usbser' } | Select-Object -First 200 TimeCreated,ProviderName,Id,LevelDisplayName,Message | Format-List",
        ),
        (
            "udp_endpoints",
            "host/udp-endpoints.txt",
            "netstat -ano -p udp",
        ),
        (
            "process_status",
            "host/process-status.txt",
            "Get-Process | Where-Object { $_.ProcessName -match '^(couchlink|parsec|parsecd)$' } | Select-Object ProcessName,Id,Path,StartTime,Responding,CPU,WorkingSet64 | Format-List",
        ),
        (
            "firewall_summary",
            "host/firewall-summary.txt",
            "netsh advfirewall show allprofiles; Write-Output ''; Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -match 'CouchLink|Parsec' } | Format-List DisplayName,Enabled,Direction,Action,Profile",
        ),
        (
            "network_routes",
            "host/network-routes.txt",
            "Get-NetIPConfiguration | Format-List; Write-Output ''; Get-NetRoute -AddressFamily IPv4 | Sort-Object RouteMetric,InterfaceMetric | Select-Object -First 120 DestinationPrefix,NextHop,InterfaceAlias,RouteMetric,InterfaceMetric,PolicyStore | Format-Table -AutoSize",
        ),
        (
            "bootsel_drives",
            "host/bootsel-drives.txt",
            "Get-PSDrive -PSProvider FileSystem | ForEach-Object { $info = Join-Path $_.Root 'INFO_UF2.TXT'; if (Test-Path $info) { Write-Output \"## $($_.Root)\"; Get-Content $info -TotalCount 80; Write-Output '' } }",
        ),
    ]
}
