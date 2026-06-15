# Hardware Lab

`couchlink lab` is an unattended bench harness for development and release
checks. It is meant for plugged-in Picos on a Windows host, not for normal
console play.

Run the current status snapshot first:

```powershell
.\couchlink.exe lab --scenario status
```

The status snapshot records visible setup-mode USB Picos, Wi-Fi/controller-mode
Picos, BOOTSEL drives, Windows XInput slots, and problem USB devices.

## Scenarios

| Scenario | What it checks |
|---|---|
| `status` | Snapshot only; no mode changes. |
| `mode-cycle` | Setup USB -> Wi-Fi/controller mode -> setup USB. |
| `flash-cycle` | Setup USB -> BOOTSEL -> flash matching UF2 -> setup USB or Wi-Fi recovery. |
| `power-cycle` | Selected reconnect backend only. |
| `full` | Status, mode cycle, reconnect cycle, flash cycle, final Wi-Fi/controller check, and signal checks. |

Run the full scenario once:

```powershell
.\couchlink.exe lab --scenario full --cycles 1
```

By default, `--power auto` uses an external power backend only when one is
configured and its probe command passes. Otherwise it falls back to firmware
reset re-enumeration.

## Reconnect Backends

| Power value | Behavior |
|---|---|
| `auto` | Use configured external power only when its probe passes; otherwise use `reset`. |
| `reset` | Reboot firmware between setup and run modes. USB power is not cut. |
| `external` | Run configured off/on commands and wait for selected boards to disappear and return. |
| `pnp-restart` | Use `pnputil /disable-device` and `/enable-device` on CouchLink setup USB instances. |
| `pnp-remove` | Use `pnputil /remove-device <id> /subtree`, then `pnputil /scan-devices`, on CouchLink PnP instances. |

`pnp-remove` is the strongest built-in Windows reconnect simulation. During the
full scenario it removes/rescans setup-mode USB instances, returns the Pico to
run mode, verifies the XInput controller persona, removes/rescans that XInput
PnP instance, then verifies Wi-Fi/controller detection and signal delivery
again.

`pnp-remove` still does not physically cut USB power. Use an external power
backend when the test must prove a real power loss or cable pull.

Example full reconnect run without reflashing:

```powershell
.\couchlink.exe lab --scenario full --cycles 1 --power pnp-remove --no-flash --json .\lab-report.json
```

## Select Boards

With no selector, the lab probes every visible Pico:

```powershell
.\couchlink.exe lab --all
```

Select one or more boards by UID, IP, or board name:

```powershell
.\couchlink.exe lab --pico 07D37EB6 --pico 192.168.50.4
```

## Firmware Inputs

The flash cycle resolves the matching UF2 for the detected board. Point `--uf2`
at either a specific UF2 or a directory containing board-specific release files:

```powershell
.\couchlink.exe lab --scenario flash-cycle --uf2 .\dist\ParsecCouchLink
```

Use `--no-flash` when the bench should exercise mode and reconnect paths
without writing firmware.

## External Power Backend

External power commands live in the per-user config file:

```text
%APPDATA%\ParsecCouchLink\config\config.toml
```

The command arrays use the first item as the executable and the remaining items
as arguments:

```toml
[lab_power]
off = ["powershell", "-NoProfile", "-File", "D:\\tools\\pico-power.ps1", "off"]
on = ["powershell", "-NoProfile", "-File", "D:\\tools\\pico-power.ps1", "on"]
probe = ["powershell", "-NoProfile", "-File", "D:\\tools\\pico-power.ps1", "probe"]
```

When `--power auto` is used, the probe must pass before the external backend is
selected.

## Reports

Add `--json` to save a machine-readable report:

```powershell
.\couchlink.exe lab --scenario full --power pnp-remove --json .\lab-report.json
```

The JSON report records selected scenario, requested and selected power backend,
visible devices, and each pass/fail/skip step with timing. A nonzero failure
count makes the command exit with an error.
