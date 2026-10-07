# Parsec CouchLink

Parsec CouchLink lets a remote Parsec player use a real retro console as player 2.

The Windows host reads the Parsec virtual Xbox controller and sends the button state over Wi-Fi to a Raspberry Pi Pico 2 W or Pico W. The Pico shows up as a USB controller to a console adapter like USB4MAPLE.

Some games want a keyboard, like Typing of the Dead on the Dreamcast. `couchlink.exe keyboard` switches the same Pico to a USB keyboard and forwards the player's typing. If you aren't sure which gamepad modes your adapter takes, `couchlink.exe auto` tries each supported USB mode and keeps the first one the adapter polls.

[Releases](https://github.com/RealWhyKnot/ParsecCouchLink/releases)

## What you need

- A Windows 10 or 11 PC running Parsec
- A Raspberry Pi Pico 2 W (RP2350, the default target), or a Pico W or Pico WH (RP2040)
- A micro-USB data cable
- The name and password of a 2.4 GHz Wi-Fi network, since both boards only have 2.4 GHz radios
- USB4MAPLE or another USB-to-console adapter that accepts Xbox 360, Xbox One, PS3, PS4 or keyboard USB devices
- The console and controller adapter you want to play on

## Quick start

1. Download the latest `ParsecCouchLink-v*.zip` from Releases.
2. Extract the whole zip to a normal folder, like `Downloads` or `C:\Tools\ParsecCouchLink`.
3. Open PowerShell in the extracted folder.
4. Run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\setup.ps1
   ```

5. Follow the prompts. The script flashes the Pico, sets up its Wi-Fi and checks that the PC can find it. It can also add `couchlink.exe` to Windows startup. You don't need a controller for setup.

Once setup's done, have the remote player join through Parsec and run `couchlink.exe`. It opens on the Basic tab and looks for Picos over Wi-Fi, setup USB and BOOTSEL, then lists each one with its own commands. For normal play, pick that Pico's "Start streaming with Controller 1" or "Choose controller and stream". One-off diagnostics and fixes are on the Advanced tab.

## Release contents

| File | Purpose |
|---|---|
| `setup.ps1` | First-run setup script. Start here. |
| `couchlink.exe` | Windows bridge. Runs at logon or by hand. |
| `couchlink-pico2w.uf2` | Firmware for the Pico 2 W (RP2350). |
| `couchlink-picow.uf2` | Firmware for the Pico W and Pico WH (RP2040). |
| `README.txt` | Short instructions for the release folder. |
| `CHANGELOG.md` | Release history. |
| `LICENSE` / `NOTICE` | License text and release archive notes. |

Setup works out which Pico you have while it's in BOOTSEL and writes the matching UF2.

## Daily use

If you took the startup shortcut during setup, the bridge starts when you sign in to Windows. Otherwise, run `couchlink.exe` before the Parsec session starts.

Other commands:

```powershell
.\couchlink.exe doctor
.\couchlink.exe auto
.\couchlink.exe xbox
.\couchlink.exe xinput
.\couchlink.exe xbox360
.\couchlink.exe xboxone
.\couchlink.exe keyboard
.\couchlink.exe maple
.\couchlink.exe dinput
.\couchlink.exe ps3
.\couchlink.exe ps4
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

If `configure-wifi` finds a Pico that's already on Wi-Fi, it can reboot it into setup-mode USB before asking for new credentials. After a firmware update, keep the current Wi-Fi if it hasn't changed and start streaming.

`couchlink lab` is for development benches with the Pico plugged into the Windows host. `--power pnp-remove` has Windows remove and rescan the CouchLink Pico devices, the run-mode XInput device included, which tests reconnect handling. It doesn't pull the cable or cut USB power. For that you need an external power backend.

## Reporting bugs

1. Run `.\couchlink.exe bundle`.
2. Open an issue at <https://github.com/RealWhyKnot/ParsecCouchLink/issues>.
3. Fill in the form and drag the generated ZIP into the comment box.

The bundle has logs, doctor output, firmware diagnostics, USB adapter counters, and recent Windows USB events when the Pico is reachable. Your Wi-Fi password isn't in it.

If the bridge won't run at all, attach the setup transcript from `%LOCALAPPDATA%\ParsecCouchLink\data\logs\setup-*.log` instead.

## Source layout

`bridge/` is the Rust Windows bridge and setup wizard, and `pico-bridge/` is the Pico firmware. `setup.ps1` is the release entry point for first-run setup. `build.ps1` builds locally and stages the release zip.

## License

GNU General Public License v3.0 or later. See [LICENSE](LICENSE) for the text and [NOTICE](NOTICE) for release archive notes.
