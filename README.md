# Parsec CouchLink

Parsec CouchLink lets a remote Parsec player use a real retro console as player 2.

The Windows host reads the Parsec virtual Xbox controller, sends the button state over Wi-Fi, and a Raspberry Pi Pico 2 W or Pico W presents that input as a wired Xbox 360 controller to a USB-to-console adapter such as USB4MAPLE.

For games that need a keyboard instead -- Typing of the Dead on the Dreamcast, for one -- the same Pico can switch to a USB keyboard with `couchlink.exe keyboard` and forward the player's typing. See [Controller Routing](https://github.com/RealWhyKnot/ParsecCouchLink/wiki/Controller-Routing) for keyboard mode.

**[Releases](https://github.com/RealWhyKnot/ParsecCouchLink/releases)** | **[Wiki](https://github.com/RealWhyKnot/ParsecCouchLink/wiki)** | **[Quick Start](https://github.com/RealWhyKnot/ParsecCouchLink/wiki/Quick-Start)** | **[Troubleshooting](https://github.com/RealWhyKnot/ParsecCouchLink/wiki/Troubleshooting)**

## What You Need

- Windows 10/11 PC running Parsec
- One of these Pico boards:
  - Raspberry Pi Pico 2 W (RP2350 + Wi-Fi) -- the default target
  - Raspberry Pi Pico W or Pico WH (RP2040 + Wi-Fi) -- also fully supported
- Micro-USB data cable
- 2.4 GHz Wi-Fi name and password (both boards use a 2.4 GHz-only radio)
- USB4MAPLE or another USB-to-console adapter that accepts a wired Xbox 360 controller
- The console and controller adapter you want to use

## Quick Start

1. Download the latest `ParsecCouchLink-v*.zip` from Releases.
2. Extract the whole zip to a normal folder, such as `Downloads` or `C:\Tools\ParsecCouchLink`.
3. Open PowerShell in the extracted folder.
4. Run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\setup.ps1
   ```

5. Follow the prompts. The script flashes the Pico, provisions Wi-Fi, checks that the PC can find it, and can add `couchlink.exe` to Windows startup. No controller is needed for setup.

After setup, have the remote player join through Parsec and run `couchlink.exe`. The app opens on the **Basic** tab, scans Wi-Fi, setup USB, and BOOTSEL, then shows each Pico with commands under that Pico only. Use the Pico's **Start streaming with Controller 1** or **Choose controller and stream** command for normal play. One-off diagnostics and fixes are under the **Advanced** tab.

## Release Contents

| File | Purpose |
|---|---|
| `setup.ps1` | First-run setup script. Start here. |
| `couchlink.exe` | Windows bridge. Runs at logon or manually. |
| `couchlink-pico2w.uf2` | Firmware for the Pico 2 W (RP2350). |
| `couchlink-picow.uf2` | Firmware for the Pico W / Pico WH (RP2040). |
| `README.txt` | Short release-folder instructions. |
| `CHANGELOG.md` | Release history. |
| `LICENSE` / `NOTICE` | License text and release archive notes. |

Setup detects which Pico you have at BOOTSEL time and uses the matching UF2 automatically. Only one of the two firmware files is written to your Pico.

## Daily Use

If you accepted the startup shortcut during setup, sign into Windows and leave the bridge running. If not, run `couchlink.exe` before the Parsec session starts.

Useful commands:

```powershell
.\couchlink.exe doctor
.\couchlink.exe keyboard
.\couchlink.exe controller
.\couchlink.exe logs --tail
.\couchlink.exe configure-wifi
.\couchlink.exe recover
.\couchlink.exe debug --status
.\couchlink.exe debug --to-wifi --port COM3
.\couchlink.exe bootsel --port COM3
.\couchlink.exe lab --scenario status
.\couchlink.exe lab --scenario full --power pnp-remove --no-flash --json .\lab-report.json
.\couchlink.exe test discover --ip 192.168.50.4
.\couchlink.exe test usb --all
.\couchlink.exe bundle
```

If `configure-wifi` finds a Pico already running on Wi-Fi, it can reboot that
Pico into setup-mode USB before asking for new credentials. If the existing
Wi-Fi is still correct after a firmware update, keep the current Wi-Fi and
start streaming.

`couchlink lab` is for development benches with the Pico plugged into the
Windows host. `--power pnp-remove` asks Windows to remove and rescan the
CouchLink Pico devices, including the run-mode XInput controller, which is
useful for checking reconnect handling. It is not a physical cable pull or a
USB power cut; use an external power backend for that.

## Reporting bugs

If something went wrong, the fastest path is:

1. `.\couchlink.exe bundle`
2. Open an issue at <https://github.com/RealWhyKnot/ParsecCouchLink/issues>
3. Fill in the form and drag the generated ZIP into the comment box.

The bundle contains logs, doctor output, and firmware diagnostics when
the Pico is reachable. It does NOT contain your Wi-Fi password.

If the bridge won't run at all, the setup transcript at
`%LOCALAPPDATA%\ParsecCouchLink\data\logs\setup-*.log` is the next-best
thing -- attach that instead.

See the wiki's [Reporting-Bugs](https://github.com/RealWhyKnot/ParsecCouchLink/wiki/Reporting-Bugs)
page for the full version.

## Source Layout

- `bridge/` - Rust Windows bridge and setup wizard.
- `pico-bridge/` - Pico firmware.
- `setup.ps1` - release entrypoint for first-run setup.
- `build.ps1` - local build and release zip staging.
- `wiki/` - source-controlled GitHub Wiki pages.

Runtime protocol v1 and setup protocol v1 are documented in the
[Protocol](wiki/Protocol.md) wiki page. Hardware bench coverage is documented
in [Hardware Lab](wiki/Hardware-Lab.md).

## License

Licensed under the GNU General Public License v3.0 or later. See [LICENSE](LICENSE) for the full text and [NOTICE](NOTICE) for release archive notes.
