# ParsecToDreamcast

ParsecToDreamcast turns a remote Parsec gamepad into a real controller input for a retro console.

```
Remote player
   |
   | Parsec virtual Xbox controller
   v
Windows host running ptd-bridge.exe
   |
   | Wi-Fi
   v
Raspberry Pi Pico 2 W running pico-bridge.uf2
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
- [[Build]] - build the release zip from source.
- [[Protocol]] - short runtime and setup protocol reference.
- [[Changelog]] - release notes.

## What Ships In A Release

| File | Purpose |
|---|---|
| `setup.ps1` | The first-run setup script. |
| `ptd-bridge.exe` | Windows app that reads the Parsec gamepad and sends state to the Pico. |
| `pico-bridge.uf2` | Pico firmware flashed by setup. |
| `README.txt` | Short copy of the release-folder instructions. |

## Normal Flow

1. Download the release zip.
2. Extract it.
3. Run `setup.ps1` from PowerShell.
4. Hold BOOTSEL while plugging in the Pico when prompted.
5. Enter the 2.4 GHz Wi-Fi credentials.
6. Let setup add the Startup shortcut.
7. Start a Parsec session and play.

The Wi-Fi password is sent to the Pico over USB setup mode. It is not saved on the PC.
