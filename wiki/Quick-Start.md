# Quick Start

This is the path for a new Pico user starting from a release zip.

## Requirements

- Windows 10/11 PC.
- Parsec installed and working.
- Raspberry Pi Pico 2 W.
- Micro-USB data cable. Charge-only cables will fail.
- 2.4 GHz Wi-Fi name and password. Pico 2 W cannot join 5 GHz-only networks.
- USB4MAPLE or another USB-to-console adapter that accepts a wired Xbox 360 controller.

## Install

1. Download the latest `ParsecToDreamcast-v*.zip` from Releases.
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
3. Setup copies `pico-bridge.uf2` to the Pico.
4. The Pico reboots as a USB serial setup device.
5. Enter the 2.4 GHz Wi-Fi credentials when prompted.
6. Setup waits for the Pico to join Wi-Fi and answer discovery.
7. Press a button on the Parsec gamepad or a local Xbox controller for the smoke test.
8. Accept the Startup shortcut if you want the bridge to run at every logon.

## What Success Looks Like

Setup ends with:

```text
Setup is complete. From now on, ptd-bridge runs at logon.
```

Then plug the Pico into the USB-to-console adapter. Start the console, have the remote player join through Parsec, and leave `ptd-bridge.exe` running on the Windows host.

## Daily Use

If the Startup shortcut was added, Windows starts the bridge at logon. If not, run:

```powershell
.\ptd-bridge.exe
```

Useful checks:

```powershell
.\ptd-bridge.exe doctor
.\ptd-bridge.exe logs --tail
.\ptd-bridge.exe bundle
```

## Reconfigure Wi-Fi

If the router or Wi-Fi password changes:

```powershell
.\ptd-bridge.exe configure-wifi
```

If the Pico cannot enter setup mode, follow [[Setup and Flashing]].
