# ParsecToDreamcast

ParsecToDreamcast lets a remote Parsec player use a real retro console as player 2.

The Windows host reads the Parsec virtual Xbox controller, sends the button state over Wi-Fi, and a Raspberry Pi Pico 2 W presents that input as a wired Xbox 360 controller to a USB-to-console adapter such as USB4MAPLE.

**[Wiki](https://github.com/RealWhyKnot/ParsecToDreamcast/wiki)** | **[Quick Start](https://github.com/RealWhyKnot/ParsecToDreamcast/wiki/Quick-Start)** | **[Troubleshooting](https://github.com/RealWhyKnot/ParsecToDreamcast/wiki/Troubleshooting)**

## What You Need

- Windows 10/11 PC running Parsec
- Raspberry Pi Pico 2 W
- Micro-USB data cable
- 2.4 GHz Wi-Fi name and password
- USB4MAPLE or another USB-to-console adapter that accepts a wired Xbox 360 controller
- The console and controller adapter you want to use

## Quick Start

1. Download the latest `ParsecToDreamcast-v*.zip` from Releases.
2. Extract the whole zip to a normal folder, such as `Downloads` or `C:\Tools\ParsecToDreamcast`.
3. Open PowerShell in the extracted folder.
4. Run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\setup.ps1
   ```

5. Follow the prompts. The script flashes the Pico, provisions Wi-Fi, checks that the PC can find it, and can add `ptd-bridge.exe` to Windows startup.

After setup, have the remote player join through Parsec. The bridge sends their gamepad to the Pico, and the console sees the Pico as a wired controller.

## Release Contents

| File | Purpose |
|---|---|
| `setup.ps1` | First-run setup script. Start here. |
| `ptd-bridge.exe` | Windows bridge. Runs at logon or manually. |
| `pico-bridge.uf2` | Firmware copied to the Pico during setup. |
| `README.txt` | Short release-folder instructions. |
| `LICENSE` / `NOTICE` | License text and release archive notes. |

## Daily Use

If you accepted the startup shortcut during setup, sign into Windows and leave the bridge running. If not, run `ptd-bridge.exe` before the Parsec session starts.

Useful commands:

```powershell
.\ptd-bridge.exe doctor
.\ptd-bridge.exe logs --tail
.\ptd-bridge.exe configure-wifi
.\ptd-bridge.exe bundle
```

## Source Layout

- `bridge/` - Rust Windows bridge and setup wizard.
- `pico-bridge/` - Pico firmware.
- `setup.ps1` - release entrypoint for first-run setup.
- `build.ps1` - local build and release zip staging.
- `wiki/` - source-controlled GitHub Wiki pages.

The project is pre-release. Runtime protocol v1 and setup protocol v1 are documented in the [Protocol](wiki/Protocol.md) wiki page.

## License

Licensed under the GNU General Public License v3.0 or later. See [LICENSE](LICENSE) for the full text and [NOTICE](NOTICE) for release archive notes.
