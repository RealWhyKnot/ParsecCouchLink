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

1. Hold BOOTSEL on the Pico.
2. Plug the Pico into the PC while still holding BOOTSEL.
3. Setup detects which Pico is in BOOTSEL and copies the matching firmware (`couchlink-pico2w.uf2` for the RP2350 board, `couchlink-picow.uf2` for the RP2040 board).
4. The Pico reboots as a USB serial setup device.
5. Enter the 2.4 GHz Wi-Fi credentials when prompted.
6. Setup waits for the Pico to join Wi-Fi and answer discovery.
7. Press a button on the Parsec gamepad or a local Xbox controller for the smoke test.
8. Accept the Startup shortcut if you want the bridge to run at every logon.

## What Success Looks Like

Setup ends with:

```text
Setup is complete. From now on, couchlink runs at logon.
```

Then plug the Pico into the USB-to-console adapter. Start the console, have the remote player join through Parsec, and leave `couchlink.exe` running on the Windows host.

## Daily Use

If the Startup shortcut was added, Windows starts the bridge at logon. If not, run:

```powershell
.\couchlink.exe
```

Useful checks:

```powershell
.\couchlink.exe doctor
.\couchlink.exe logs --tail
.\couchlink.exe bundle
```

## Reconfigure Wi-Fi

If the router or Wi-Fi password changes:

```powershell
.\couchlink.exe configure-wifi
```

If the Pico cannot enter setup mode, follow [[Setup and Flashing]].
