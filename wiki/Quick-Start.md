# Quick Start

This is the path for a new Pico user starting from a release zip.

## Requirements

- Windows 10/11 PC.
- Parsec installed and working.
- One of these Pico boards:
  - Raspberry Pi Pico 2 W (RP2350 + Wi-Fi) -- the default target.
  - Raspberry Pi Pico W or Pico WH (RP2040 + Wi-Fi) -- equivalent and fully supported.
- Micro-USB data cable. Charge-only cables will fail.
- 2.4 GHz Wi-Fi name and password. Both Pico variants use the CYW43439 radio, which is 2.4 GHz only -- 5 GHz-only networks won't work.
- USB-output modes: USB4MAPLE or another USB-to-console adapter that accepts Xbox 360, Xbox One, PS3, PS4, or keyboard USB devices.
- Bluetooth mode: a Bluetooth receiver or adapter that accepts a Classic Bluetooth HID gamepad, with the Pico kept plugged into the bridge PC over USB.

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
5. The Pico reboots into the CouchLink firmware. After a firmware update it enters USB setup/debug mode and keeps any saved Wi-Fi credentials. On later normal replug, saved credentials make it boot into the saved input mode, with XInput as the default.
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

For USB-output modes, plug the Pico into the USB-to-console adapter. For Bluetooth mode, leave the Pico plugged into the bridge PC over USB so CouchLink can feed it controller state with low latency. Put the receiver or console adapter into Bluetooth pairing/search mode and pair it with the CouchLink Bluetooth gamepad; use PIN `0000` if asked. Start the console, have the remote player join through Parsec, run `couchlink.exe`, and choose the Pico's streaming command on the **Basic** tab.

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
- Choose Auto, Xbox, DInput / PlayStation, Bluetooth, Maple, or Keyboard input mode before guided streaming.
- Show each Pico state: Wi-Fi ready, USB debug, BOOTSEL, or not seen.
- Save a detected Pico or remove a missing saved Pico.
- Recover one Pico left in USB debug mode when it already has saved Wi-Fi.
- Set up Wi-Fi, update firmware, read USB logs, or check the console USB adapter for one selected Pico.
- Open **Advanced** for one-off tools, status, Wi-Fi finder, controller check, Pico debug/recovery, USB adapter checks, logs, support bundles, and command reference.

When streaming starts, the terminal prints live counters. USB-output modes show PC-to-Pico Wi-Fi packets and Pico replies; use **Check console USB adapter** or `couchlink.exe test usb` if the console still sees no input. Bluetooth mode shows the PC USB input link plus the Pico's Bluetooth receiver state, including whether it is still discoverable, reached pairing/security without opening Classic HID, or connected and sending reports. For BlueRetro, try generic Bluetooth before the Xbox-named Classic HID mimic.

If the Startup shortcut was added, Windows starts the direct streaming command at logon:

```powershell
.\couchlink.exe run
```

The direct command uses the saved routing layout from the guided menu. If no layout is saved yet, it uses the first Pico it finds.

Useful direct commands:

```powershell
.\couchlink.exe doctor  # run every diagnostic check
.\couchlink.exe logs --tail  # follow the active log file
.\couchlink.exe bundle  # produce a support-bundle ZIP for bug reports
.\couchlink.exe flash   # re-flash without re-running setup
.\couchlink.exe debug   # open Pico debug and recovery
.\couchlink.exe configure-wifi  # re-send Wi-Fi credentials
.\couchlink.exe recover # auto-check Wi-Fi, setup USB, and BOOTSEL before streaming
.\couchlink.exe run --all  # route Controller 1, 2, ... to every detected Pico
.\couchlink.exe run --route 1=07D37EB6  # route Controller 1 to a specific Pico UID
.\couchlink.exe bluetooth  # switch one Pico to Bluetooth mode and stream over PC USB
.\couchlink.exe test usb --all  # check whether the USB adapter is polling the Pico
.\couchlink.exe debug --status  # show whether the Pico is on Wi-Fi, USB debug, or BOOTSEL
.\couchlink.exe debug --to-wifi --port COM3  # switch one USB debug Pico back to Wi-Fi
.\couchlink.exe bootsel --port COM3  # switch one USB debug Pico to BOOTSEL
.\couchlink.exe lab --scenario status  # development bench status snapshot
.\couchlink.exe test <name>  # run one diagnostic check by name
```

`setup.ps1` records a setup transcript under `%LOCALAPPDATA%\ParsecCouchLink\data\logs\` alongside the bridge's own logs. Other support and recovery tasks run through `couchlink.exe`.

## Reconfigure Wi-Fi

If the router or Wi-Fi password changes:

```powershell
.\couchlink.exe configure-wifi
```

After the Pico rejoins Wi-Fi, the command prints the confirmed Pico IP.

If the Pico is already running on Wi-Fi, the command can ask it to reboot into
setup-mode USB and then continue. If the Pico already has the correct Wi-Fi,
choose **Use current Wi-Fi and stop**.

## Pico Debug And Recovery

If you are not sure which mode the Pico is in, run:

```powershell
.\couchlink.exe debug
```

The debug menu shows whether the Pico is in Wi-Fi/input mode, USB debug
mode, or BOOTSEL firmware mode. It can switch a Wi-Fi Pico into USB debug mode,
switch USB debug mode back to Wi-Fi/input mode, read the firmware log, and
send a USB debug Pico into BOOTSEL for firmware update.
