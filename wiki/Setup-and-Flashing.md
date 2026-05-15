# Setup and Flashing

`setup.ps1` is the release entrypoint. It calls `couchlink.exe setup`, which detects which Pico you have in BOOTSEL and flashes the matching firmware. The user does not have to know whether they have a Pico 2 W (RP2350) or a Pico W / Pico WH (RP2040).

## What Setup Does

1. Checks that the release folder has `couchlink.exe` and at least one firmware file (`couchlink-pico2w.uf2` or `couchlink-picow.uf2`).
2. Tells the user what hardware is needed.
3. Starts the bridge setup wizard.
4. Detects the Pico in BOOTSEL mode and copies the matching UF2 onto it.
5. Sends Wi-Fi credentials over USB setup mode.
6. Waits for the Pico on the LAN.
7. Runs a simple XInput smoke test.
8. Offers to create a Windows Startup shortcut.

The Wi-Fi password is held only long enough to send it to the Pico. It is not written to disk on the PC.

## Supported Boards

Either of these works -- the wizard auto-picks the right firmware:

| Board | Chip | BOOTSEL volume label | UF2 file |
|---|---|---|---|
| Pico 2 W | RP2350 + Wi-Fi | `RP2350` | `couchlink-pico2w.uf2` |
| Pico W / Pico WH | RP2040 + Wi-Fi | `RPI-RP2` | `couchlink-picow.uf2` |

Wi-Fi is 2.4 GHz only on both -- the underlying CYW43439 radio doesn't do 5 GHz.

## BOOTSEL Flashing

Use this when setup asks for the Pico in BOOTSEL mode:

1. Unplug the Pico.
2. Hold the BOOTSEL button.
3. Plug the Pico into the PC while holding BOOTSEL.
4. Release BOOTSEL after Windows shows the removable drive (`RPI-RP2` or `RP2350`).

The bridge looks for either drive label, picks the matching UF2 from the release folder, and copies it. The Pico usually reboots as soon as the copy completes.

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

If the Pico appears as a removable drive instead, it is in the hardware bootloader rather than firmware setup mode. Re-run setup (which will re-flash) and then retry the recovery steps.

## Manual Flash

Manual flash is still available. With no `--uf2`, the bridge picks the matching file from next to `couchlink.exe`:

```powershell
.\couchlink.exe flash
```

To force a specific file:

```powershell
.\couchlink.exe flash --uf2 .\couchlink-pico2w.uf2
.\couchlink.exe flash --uf2 .\couchlink-picow.uf2
```

You can also drag the appropriate UF2 onto the `RPI-RP2` or `RP2350` drive in Explorer. Make sure you use `couchlink-picow.uf2` for the `RPI-RP2` drive and `couchlink-pico2w.uf2` for the `RP2350` drive -- the chip families aren't cross-compatible and the bootloader rejects the wrong one.
