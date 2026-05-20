# Parsec CouchLink

Parsec CouchLink turns a remote Parsec gamepad into a real controller input for a retro console.

```
Remote player
   |
   | Parsec virtual Xbox controller
   v
Windows host running couchlink.exe
   |
   | Wi-Fi
   v
Raspberry Pi Pico 2 W running couchlink-pico.uf2
   |
   | USB as wired Xbox 360 controller
   v
USB4MAPLE or another USB-to-console adapter
   |
   v
Console player 2
```

## Start Here

- [[Quick Start]] - install from a release zip and run the setup script.
- [[Setup and Flashing]] - what the script does and how to recover a Pico.
- [[Troubleshooting]] - what to run when setup or discovery fails.
- [[Remote Help|Remote-Help]] - open a debug tunnel so someone you trust can look at your bridge over the network.
- [[Reporting-Bugs]] - how to make a bundle and what to include in an issue.
- [[Build]] - build the release zip from source.
- [[Protocol]] - short runtime and setup protocol reference.
- [[Changelog]] - release notes.

## What Ships In A Release

| File | Purpose |
|---|---|
| `setup.ps1` | The first-run setup script. |
| `couchlink.exe` | Windows app that reads the Parsec gamepad and sends state to the Pico. |
| `couchlink-pico.uf2` | Pico firmware flashed by setup. |
| `README.txt` | Short copy of the release-folder instructions. |

## Normal Flow

1. Download the release zip.
2. Extract it.
3. Run `setup.ps1` from PowerShell.
4. When prompted, hold BOOTSEL, plug in the Pico, then release BOOTSEL as soon as Windows shows the `RPI-RP2` or `RP2350` drive. Do not press BOOTSEL again during the reboot that follows.
5. Enter the 2.4 GHz Wi-Fi credentials.
6. Let setup add the Startup shortcut.
7. Start a Parsec session and play.

The Wi-Fi password is sent to the Pico over USB setup mode. It is not saved on the PC.
