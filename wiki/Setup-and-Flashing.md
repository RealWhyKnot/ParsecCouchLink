# Setup and Flashing

`setup.ps1` is the release entrypoint. It calls `couchlink.exe setup`, which detects which Pico you have in BOOTSEL and flashes the matching firmware. The user does not have to know whether they have a Pico 2 W (RP2350) or a Pico W / Pico WH (RP2040).

## What Setup Does

1. Checks that the release folder has `couchlink.exe` and at least one firmware file (`couchlink-pico2w.uf2` or `couchlink-picow.uf2`).
2. Tells the user what hardware is needed.
3. Starts the bridge setup wizard.
4. Detects the Pico in BOOTSEL mode and copies the matching UF2 onto it.
5. Sends Wi-Fi credentials over USB setup mode.
6. Waits for the Pico on the LAN.
7. Offers to create a Windows Startup shortcut.

The Wi-Fi password is held only long enough to send it to the Pico. It is not written to disk on the PC.

Setup and firmware flashing do not require a controller. Controller checks belong to the routing/streaming path after the Pico is online.

## Supported Boards

Either of these works -- the wizard auto-picks the right firmware:

| Board | Chip | BOOTSEL volume label | UF2 file |
|---|---|---|---|
| Pico 2 W | RP2350 + Wi-Fi | `RP2350` | `couchlink-pico2w.uf2` |
| Pico W / Pico WH | RP2040 + Wi-Fi | `RPI-RP2` | `couchlink-picow.uf2` |

Wi-Fi is 2.4 GHz only on both -- the underlying CYW43439 radio doesn't do 5 GHz.

## BOOTSEL Flashing

Use this when setup asks for the Pico in BOOTSEL mode:

1. Unplug the Pico if it is currently connected.
2. Press and **hold** the BOOTSEL button on the Pico.
3. With BOOTSEL still held, plug the Pico into the PC using a micro-USB **data** cable.
4. **Release BOOTSEL** as soon as Windows shows the removable drive (`RPI-RP2` for Pico W or Pico WH, `RP2350` for Pico 2 W). The Pico stays in flash mode after release -- you do not need to keep the button held while the firmware copies.

The bridge looks for either drive label, picks the matching UF2 from the release folder, and copies it. The Pico usually reboots as soon as the copy completes.

**Do not press BOOTSEL again while the Pico is rebooting into the new firmware.** The firmware reads BOOTSEL during its first three seconds of run time as a "wipe saved Wi-Fi credentials" signal. A stray press during reboot will erase any credentials that are saved and put you back at the provisioning prompt.

## Setup Mode

After flashing, the Pico should reboot as a USB serial setup device. The setup wizard sends:

- Wi-Fi SSID.
- Wi-Fi password.
- Reboot-to-run command.

Then the Pico joins Wi-Fi and starts listening for the bridge on UDP port 4242.
Setup prints the confirmed Pico IP after it receives a reply.

## No-Button Reflash

If the Pico is already running CouchLink setup mode, the app can ask it to reboot into BOOTSEL. You do not need to press the BOOTSEL button for that path.

From the guided menu:

1. Run `.\couchlink.exe`.
2. Choose **Update Pico firmware**.
3. Choose **Update firmware (recommended)**.

The guided path tries setup-mode USB first. If that is not available, it switches to BOOTSEL flashing instructions.

Advanced flash choices are still available under **Advanced flash options**, but most users should not need them.

Direct command:

```powershell
.\couchlink.exe flash --from-usb --all
```

This only works when the Pico is visible as a setup-mode USB serial device. If the Pico is already in run mode, or if Windows cannot see setup mode, use BOOTSEL flashing instead.

## Wi-Fi Change Recovery

If the Pico already has saved Wi-Fi, a firmware update may reboot straight
into run mode as an Xbox controller instead of setup-mode USB. That is OK when
the Wi-Fi is still correct.

To change Wi-Fi:

```powershell
.\couchlink.exe configure-wifi
```

If CouchLink finds the Pico running on Wi-Fi, it can ask the Pico to reboot
into setup-mode USB and then continue. If Wi-Fi is wrong and the Pico cannot
join the router, leave it powered for about 30 seconds; firmware with saved but
failing credentials bounces back to setup mode so `configure-wifi` can reach it.

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
