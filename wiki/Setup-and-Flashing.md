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

## Recovery

If the Pico has bad Wi-Fi credentials saved, it reboots into run mode (an XInput controller, not a serial device) on every plug-in, so `configure-wifi` cannot reach it. The firmware provides a credential-wipe trigger on the BOOTSEL button. The **timing is different from BOOTSEL flashing** -- this is the most common source of confusion.

For BOOTSEL flashing you press the button **before** plugging in. For a credential wipe you press it **after** plugging in:

1. Unplug the Pico.
2. Plug the Pico back in **without** pressing BOOTSEL. Wait roughly one second.
3. Within the first three seconds after plug-in, press and **hold** the BOOTSEL button.
4. Keep holding for at least three full seconds.
5. Release BOOTSEL.

The firmware reads the BOOTSEL pin three seconds into its boot. If the button is held at that moment, it clears saved Wi-Fi and reboots into setup mode. Then run:

```powershell
.\couchlink.exe configure-wifi
```

If the Pico instead appears as a removable drive named `RPI-RP2` or `RP2350`, you held BOOTSEL during plug-in by mistake -- that puts the Pico into the boot ROM's flashing mode, not the firmware's credential-wipe path. Unplug, wait a second, and try the recovery steps again without holding BOOTSEL while you plug in.

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
