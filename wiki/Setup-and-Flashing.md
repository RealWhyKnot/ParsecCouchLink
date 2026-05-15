# Setup and Flashing

`setup.ps1` is the release entrypoint. It wraps `couchlink.exe setup --uf2 .\couchlink-pico.uf2` so the user does not have to find the firmware file by hand.

## What Setup Does

1. Checks that the release folder has `couchlink.exe` and `couchlink-pico.uf2`.
2. Tells the user what hardware is needed.
3. Starts the bridge setup wizard.
4. Flashes the Pico in BOOTSEL mode.
5. Sends Wi-Fi credentials over USB setup mode.
6. Waits for the Pico on the LAN.
7. Runs a simple XInput smoke test.
8. Offers to create a Windows Startup shortcut.

The Wi-Fi password is held only long enough to send it to the Pico. It is not written to disk on the PC.

## BOOTSEL Flashing

Use this when setup asks for the Pico in BOOTSEL mode:

1. Unplug the Pico.
2. Hold the BOOTSEL button.
3. Plug the Pico into the PC while holding BOOTSEL.
4. Release BOOTSEL after Windows shows the removable drive.

The bridge looks for an `RPI-RP2` or `RP2350` drive and copies the UF2 onto it. The Pico usually reboots as soon as the copy completes.

## Setup Mode

After flashing, the Pico should reboot as a USB serial setup device. The setup wizard sends:

- Wi-Fi SSID.
- Wi-Fi password.
- Reboot-to-run command.

Then the Pico joins Wi-Fi and starts listening for the bridge on UDP port 4242.

## Recovery

If the Pico has bad Wi-Fi credentials:

1. Unplug the Pico.
2. Hold BOOTSEL.
3. Plug it back in.
4. Keep holding BOOTSEL for at least 3 seconds after power-on.

The firmware clears saved Wi-Fi and reboots into setup mode. Then run:

```powershell
.\couchlink.exe configure-wifi
```

If the Pico appears as a removable drive instead, it is in the hardware bootloader rather than firmware setup mode. Flash `couchlink-pico.uf2` again and then retry the recovery steps.

## Manual Flash

Manual flash is still available:

```powershell
.\couchlink.exe flash --uf2 .\couchlink-pico.uf2
```

Or drag `couchlink-pico.uf2` onto the `RPI-RP2` or `RP2350` drive.
