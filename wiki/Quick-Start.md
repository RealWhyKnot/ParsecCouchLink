# Quick Start

This is the path for a new Pico user starting from a release zip.

## Requirements

- Windows 10/11 PC.
- Parsec installed and working.
- One of these Pico boards:
  - Raspberry Pi Pico 2 W (RP2350 + Wi-Fi) -- the default target.
  - Raspberry Pi Pico W or Pico WH (RP2040 + Wi-Fi) -- equivalent, often easier to find in retail (e.g. Micro Center stocks the Pico WH at ~$6).
- Micro-USB data cable. Charge-only cables will fail.
- 2.4 GHz Wi-Fi name and password. Both Pico variants use the CYW43439 radio, which is 2.4 GHz only -- 5 GHz-only networks won't work.
- USB4MAPLE or another USB-to-console adapter that accepts a wired Xbox 360 controller.

## Install

1. Download the latest `ParsecCouchLink-v*.zip` from Releases.
2. Extract the full zip to a normal folder. Avoid `Program Files`.
3. Open PowerShell in that folder.
4. Run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\setup.ps1
   ```

## First Run

The script explains each step before it starts. In short:

1. Press and **hold** the BOOTSEL button on the Pico.
2. With BOOTSEL still held, plug the Pico into the PC using a micro-USB **data** cable (charge-only cables will not work).
3. **Release BOOTSEL** as soon as Windows shows a removable drive named `RPI-RP2` (Pico W or Pico WH) or `RP2350` (Pico 2 W). The Pico stays in flash mode after you let go -- you do not need to keep the button held while the firmware copies.
4. Setup detects which Pico is in BOOTSEL and copies the matching firmware (`couchlink-pico2w.uf2` for the RP2350 board, `couchlink-picow.uf2` for the RP2040 board).
5. The Pico reboots into the CouchLink firmware. After a firmware update it enters USB setup/debug mode and keeps any saved Wi-Fi credentials. On later normal replug, saved credentials make it boot as a controller.
6. The Pico comes back as a USB serial setup device.
7. Enter the 2.4 GHz Wi-Fi credentials when prompted.
8. Setup waits for the Pico to join Wi-Fi and answer discovery.
9. Accept the Startup shortcut if you want the bridge to run at every logon.

No controller is needed during setup. Controller detection and routing happen later, when you choose a Pico's streaming command on the **Basic** tab or run `couchlink.exe run`.

## What Success Looks Like

Setup ends with:

```text
Setup is complete. From now on, couchlink runs at logon.
Confirmed Pico IP: 192.168.50.4
```

Then plug the Pico into the USB-to-console adapter. Start the console, have the remote player join through Parsec, run `couchlink.exe`, and choose the Pico's streaming command on the **Basic** tab.

If automatic Wi-Fi discovery fails later, choose **Enter Pico IP manually** in the guided menu and enter the confirmed IP from setup.

## Daily Use

Run the app from the release folder:

```powershell
.\couchlink.exe
```

The app opens on the **Basic** tab. It scans Wi-Fi, setup USB, and BOOTSEL,
then shows each saved or detected Pico as its own device entry. Commands are
listed under the Pico they affect, so Basic never runs a multi-Pico fix or
flash by accident.

The guided menu can:

- Start streaming for one selected Pico.
- Choose which Windows controller feeds one selected Pico.
- Show each Pico state: Wi-Fi ready, USB debug, BOOTSEL, or not seen.
- Save a detected Pico or remove a missing saved Pico.
- Recover one Pico left in USB debug mode when it already has saved Wi-Fi.
- Set up Wi-Fi, update firmware, read USB logs, or check the console USB adapter for one selected Pico.
- Open **Advanced** for one-off tools, status, Wi-Fi finder, controller check, Pico debug/recovery, USB adapter checks, logs, support bundles, and command reference.

When streaming starts, the terminal prints live counters so you can see packets going out to the Pico and replies coming back.

If the Startup shortcut was added, Windows starts the direct streaming command at logon:

```powershell
.\couchlink.exe run
```

The direct command uses the saved routing layout from the guided menu. If no layout is saved yet, it uses the first Pico it finds.

Each subcommand also has a one-shot PowerShell wrapper in the release folder. Right-click and "Run with PowerShell", or call from an existing PowerShell prompt:

```powershell
.\doctor.ps1            # run every diagnostic check
.\logs.ps1 --tail       # follow the active log file
.\bundle.ps1            # produce a support-bundle ZIP for bug reports
.\flash.ps1             # re-flash without re-running setup
.\bootsel.ps1           # switch setup-mode USB Pico to BOOTSEL
.\debug.ps1             # open Pico debug and recovery
.\configure-wifi.ps1    # re-send Wi-Fi credentials
.\couchlink.exe recover # auto-check Wi-Fi, setup USB, and BOOTSEL before streaming
.\couchlink.exe run --all  # route Controller 1, 2, ... to every detected Pico
.\couchlink.exe run --route 1=07D37EB6  # route Controller 1 to a specific Pico UID
.\couchlink.exe test usb --all  # check whether the USB adapter is polling the Pico
.\couchlink.exe debug --status  # show whether the Pico is on Wi-Fi, USB debug, or BOOTSEL
.\couchlink.exe debug --to-wifi --port COM3  # switch one USB debug Pico back to Wi-Fi
.\couchlink.exe bootsel --port COM3  # switch one USB debug Pico to BOOTSEL
.\couchlink.exe lab --scenario status  # development bench status snapshot
.\couchlink.exe test <name>  # run one diagnostic check by name
```

The wrappers record a transcript under `%LOCALAPPDATA%\ParsecCouchLink\data\logs\` alongside the bridge's own logs, so one folder has everything a bug report needs. The bare `couchlink.exe` form works too -- the wrappers are just shortcuts that pre-name the subcommand and capture a transcript.

## Reconfigure Wi-Fi

If the router or Wi-Fi password changes:

```powershell
.\configure-wifi.ps1
```

(equivalent to `.\couchlink.exe configure-wifi`.)

After the Pico rejoins Wi-Fi, the command prints the confirmed Pico IP.

If the Pico is already running on Wi-Fi, the command can ask it to reboot into
setup-mode USB and then continue. If the Pico already has the correct Wi-Fi,
choose **Use current Wi-Fi and stop**.

## Pico Debug And Recovery

If you are not sure which mode the Pico is in, run:

```powershell
.\couchlink.exe debug
```

The debug menu shows whether the Pico is in Wi-Fi/controller mode, USB debug
mode, or BOOTSEL firmware mode. It can switch a Wi-Fi Pico into USB debug mode,
switch USB debug mode back to Wi-Fi/controller mode, read the firmware log, and
send a USB debug Pico into BOOTSEL for firmware update.
