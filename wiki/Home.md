# Parsec CouchLink

Parsec CouchLink turns a remote Parsec gamepad into a real controller input for a retro console.

```
Remote player
   |
   | Parsec virtual Xbox controller(s)
   v
Windows host running couchlink.exe
   |
   | Wi-Fi
   v
Raspberry Pi Pico W or Pico 2 W running CouchLink firmware
   |
   | USB as selected input persona
   v
USB4MAPLE or another USB-to-console adapter
   |
   v
Console player 2
```

## Start here

- [Quick Start](Quick-Start.md) - install from a release zip and run the setup script.
- [Controller Routing](Controller-Routing.md) - choose which controller goes to which Pico.
- [Setup and Flashing](Setup-and-Flashing.md) - what the script does and how to recover a Pico.
- [Troubleshooting](Troubleshooting.md) - what to run when setup or discovery fails.
- [Hardware Lab](Hardware-Lab.md) - unattended bench checks for firmware, reconnects, and controller output.
- [Reporting bugs](Reporting-Bugs.md) - how to make a bundle and what to include in an issue.
- [Build](Build.md) - build the release zip from source.
- [Protocol](Protocol.md) - short runtime and setup protocol reference.
- [Changelog](Changelog.md) - release notes.

## What ships in a release

| File | Purpose |
|---|---|
| `setup.ps1` | The first-run setup script. |
| `couchlink.exe` | Windows app that guides setup, routing, flashing, diagnostics, and streaming. |
| `couchlink-pico2w.uf2` | Firmware for Pico 2 W. |
| `couchlink-picow.uf2` | Firmware for Pico W / Pico WH. |
| `README.txt` | Short copy of the release-folder instructions. |
| `CHANGELOG.md` | Release history. |
| `LICENSE` / `NOTICE` | License text and release archive notes. |

## Normal flow

1. Download the release zip.
2. Extract it.
3. Run `setup.ps1` from PowerShell.
4. When prompted, hold BOOTSEL, plug in the Pico, then release BOOTSEL as soon as Windows shows the `RPI-RP2` or `RP2350` drive. Do not press BOOTSEL again during the reboot that follows.
5. Enter the 2.4 GHz Wi-Fi credentials.
6. Let setup add the Startup shortcut.
7. Run `couchlink.exe`, choose the Pico on the **Basic** tab, start streaming, then start a Parsec session and play.

The Wi-Fi password is sent to the Pico over USB setup mode. It is not saved on the PC.

For development benches with the Pico plugged into the Windows host, `couchlink lab`
can cycle setup mode, BOOTSEL flashing, run-mode controller enumeration, and
signal checks. See [Hardware Lab](Hardware-Lab.md).
