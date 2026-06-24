# Controller Routing

Run:

```powershell
.\couchlink.exe
```

On the **Basic** tab, choose the Pico you want to use. Pick **Start streaming with Controller 1** for the normal one-controller path, or **Choose controller and stream** to select Controller 1, 2, 3, or 4 for that Pico. Before streaming starts, choose the Pico input mode: Auto, Xbox, DInput / PlayStation, Bluetooth, Maple, or Keyboard. Xbox then lets you choose Auto Xbox, Xbox 360, or Xbox One; DInput / PlayStation lets you choose Auto DInput, PS3, or PS4.

## Common Layouts

For one Pico, use **Start streaming with Controller 1**.

For two or more Picos, Basic keeps actions under each Pico so you do not run a broad multi-Pico action by accident. Start the Pico you want, or use direct commands for scripted multi-Pico layouts:

```powershell
.\couchlink.exe run --all
.\couchlink.exe run --route 1=07D37EB6 --route 2=523861E6
```

Basic streaming actions save the selected layout. After that, `couchlink.exe run` uses the saved layout without asking again, which is what the Startup shortcut uses. Direct `run --all` and `run --route ...` commands use the layout on the command line.

## Auto Mode

Auto mode is for adapters where you do not know which gamepad USB shape works. It tries the Pico's gamepad personas, watches the Pico's USB diagnostics, and keeps the first mode where the adapter configures the USB device and accepts input reports.

Run:

```powershell
.\couchlink.exe auto
```

With several Picos, pick one or scan all:

```powershell
.\couchlink.exe auto --pico 07D37EB6
.\couchlink.exe auto --all
```

Auto tries the current gamepad mode first, then Xbox 360, Xbox One, PS3, PS4, and Maple without trying Keyboard. Keyboard is separate because it streams keystrokes instead of the Parsec/XInput gamepad state. Add `--no-stream` to select a mode without starting a stream.

Auto can prove that the adapter configured and polled the Pico's USB device. It cannot prove that a Dreamcast game accepted the translated Maple-side input; use the game's controller test or gameplay for that final check.

## XInput Mode

XInput mode is the default gamepad persona. The Pico presents a wired Xbox 360-style USB controller and consumes the normal Parsec/XInput gamepad state from the PC. `xbox360` is a direct alias for the same persona.

Switch a Pico to XInput mode and start streaming:

```powershell
.\couchlink.exe xinput
```

or:

```powershell
.\couchlink.exe xbox360
```

With several Picos, pick one:

```powershell
.\couchlink.exe xinput --pico 07D37EB6
```

Add `--no-stream` to change the persona without starting a stream.

## Xbox One Mode

Xbox One mode presents an Xbox One-compatible USB gamepad persona and consumes the same Parsec/XInput controller source as Xbox 360 mode. It is useful for adapters that support Xbox One-style devices but do not poll the wired Xbox 360 shape.

Switch a Pico to Xbox One mode and start streaming:

```powershell
.\couchlink.exe xboxone
```

To try only the Xbox family and keep whichever one the adapter polls:

```powershell
.\couchlink.exe xbox --no-stream
```

Use the guided menu's **Xbox** choice for the family prompt. Add `--no-stream` to change the persona without starting a stream.

## Keyboard Mode

Some console games need a keyboard instead of a controller -- Typing of the Dead on the Dreamcast is the usual reason. A Pico can present itself as a USB keyboard instead of an XInput gamepad, and the bridge forwards the remote player's typing to it.

Only the remote Parsec player's keystrokes are forwarded -- the bridge captures Parsec-injected input, not anything typed locally at the host PC. This matches the controller path, which reads Parsec's virtual gamepad rather than a local one.

Switch a Pico to keyboard mode and start streaming:

```powershell
.\couchlink.exe keyboard
```

With one Pico, that is all you need. With several, pick one:

```powershell
.\couchlink.exe keyboard --pico 07D37EB6
```

The Pico stores the choice and reboots into it, so after a one-time switch a plain `couchlink.exe run` keeps streaming the keyboard to that board. Switch back to XInput the same way:

```powershell
.\couchlink.exe xinput
```

Add `--no-stream` to either command to change the persona without starting a stream. The switch happens over Wi-Fi, so the Pico can stay plugged into the console the whole time.

A keyboard adapter is required on the console side: the Pico presents a standard USB HID boot keyboard, and the console adapter (for example a USB4MAPLE/Pico2Maple feeding a Dreamcast) maps it to the console's native keyboard. While streaming, the status line shows the keyboard instead of a controller:

```text
Keyboard -> 07D37EB6 | source live | out +60 total 900 | in 30 (reply 0.2s ago) | keys mods=0x02 keys=[0x04]
```

## Maple Mode

Maple mode is for Dreamcast adapters that already accept wired Xbox 360 controllers and translate them to Dreamcast Maple. It uses the same Parsec/XInput controller source and the same Xbox 360-compatible USB reports as XInput mode, but keeps a separate persisted mode and status label for Dreamcast setups.

Switch a Pico to Maple mode and start streaming:

```powershell
.\couchlink.exe maple
```

With several Picos, pick one:

```powershell
.\couchlink.exe maple --pico 07D37EB6
```

Switch back to XInput mode with:

```powershell
.\couchlink.exe xinput
```

Add `--no-stream` to change the persona without starting a stream. While streaming, Maple mode status still shows a controller source and the live XInput button/axis values because those are the values sent to the Maple-capable adapter.

## DInput Mode

DInput mode is a PlayStation-family compatibility selector for USB4MAPLE-style adapters that accept PS3 or PS4 HID controllers but do not accept the Pico's Xbox 360-compatible USB shape. It uses the same Parsec/XInput controller source as XInput and Maple mode, then cycles PS3 and PS4 until the adapter polls one.

Switch a Pico to DInput mode and start streaming:

```powershell
.\couchlink.exe dinput
```

To pin a specific PlayStation persona:

```powershell
.\couchlink.exe ps3
.\couchlink.exe ps4
```

With several Picos, pick one:

```powershell
.\couchlink.exe dinput --pico 07D37EB6
```

Switch back to XInput mode with:

```powershell
.\couchlink.exe xinput
```

Add `--no-stream` to change the persona without starting a stream. PS3 is tried before PS4 because USB4MAPLE field reports more consistently recommend PS3 mode for generic DInput-style compatibility. Rumble is not implemented for these personas.

## Bluetooth Mode

Bluetooth mode is for a wireless receiver or adapter that accepts a Classic Bluetooth HID gamepad. Unlike the USB-output modes, the Pico stays plugged into the bridge PC. CouchLink sends each controller frame to the Pico over USB CDC, and the Pico sends Bluetooth HID reports to the paired receiver. The live Bluetooth command is a streaming loop and keeps printing status until you stop it.

Pairing is receiver-side: run a Bluetooth command, leave the Pico plugged into the bridge PC over USB, then put the receiver or console adapter into Bluetooth pairing/search mode. Pair with the advertised controller name for the selected mode: `CouchLink BT HID` for generic Bluetooth, `Xbox Wireless Controller` for Xbox, or `Wireless Controller` for DualShock 4 / PlayStation. Use PIN `0000` if the receiver asks for one. Do not pair the Pico to Windows for console play; Windows seeing the Pico as pairable only proves the Pico is advertising.

Switch a Pico to generic Bluetooth mode and start streaming:

```powershell
.\couchlink.exe bluetooth
```

Controller-specific Bluetooth mimics are also available:

```powershell
.\couchlink.exe bluetooth-xbox
.\couchlink.exe bluetooth-playstation
```

For BITFUNX/BlueRetro N64 adapters, leave the Pico on the bridge PC over USB and plug the adapter into the N64 controller port. CouchLink does not use the adapter Mini USB/config port as the controller input path. Start with DualShock 4 mode (`.\couchlink.exe blueretro-playstation`), then try generic HID (`.\couchlink.exe blueretro`) if HID does not open. Use `bluetooth-xbox` / `blueretro-xbox` only as a diagnostic; the Xbox-named mode is a Classic Bluetooth HID mimic, not Xbox BLE/HOGP mode. Do not flash official upstream firmware onto BITFUNX units unless the vendor says that firmware matches the unit.

The stream will not start unless the selected Bluetooth-mode Pico is also visible as the CouchLink USB diagnostic device on the PC. This avoids the extra Wi-Fi hop for live controller input. In Bluetooth mode, the plugged-in Pico is the bridge input/diagnostic path, not the console-side controller output; pick the Parsec virtual controller or local test controller as the source slot, and use the Bluetooth receiver/adapter as the output.

During streaming, the status lines separate the local USB input link from the Bluetooth receiver link. `PC USB input` counters show controller frames reaching the Pico. `discoverable` means the Pico is waiting for the receiver. `pairing/security seen` means the receiver reached Bluetooth security but did not open a Classic HID channel. `HID reconnect scheduled` or `HID reconnect attempts` means the Pico is actively trying to open HID to the paired receiver after pairing. `receiver connected` plus increasing report counts means the Pico is sending Bluetooth HID reports to the paired receiver.

### If the player's typing isn't reaching the game

A few Parsec-side settings gate guest keyboard input:

- **Keyboard permission.** Guests default to controller-only. The host must grant the guest keyboard and mouse permission (click the guest's profile picture during the session, or set default permissions in Parsec).
- **Approved Apps.** If the host has Approved Apps enabled, guest input is blocked unless the focused window is on the whitelist. It is off by default.
- **Same session.** Run the bridge in the same signed-in Windows session Parsec is injecting into. It will not see input across the lock screen, a UAC/secure desktop prompt, or a different user session.
- **Layout and lock keys.** The character a key produces follows the host keyboard layout, so a client/host layout mismatch (for example AZERTY vs QWERTY) can type the wrong characters, and Caps/Num Lock state can drift between the two ends.

To confirm the bridge is capturing the player's keystrokes, run with verbose logging while the guest types:

```powershell
.\couchlink.exe keyboard -vv
```

A single `keyboard: capturing Parsec-injected input` line confirms the hook is seeing the guest's input; `-vv` then logs each captured key. If that line never appears while the guest is typing, the input isn't being injected the way the bridge expects -- check the Parsec settings above.

## What Parsec Provides

Parsec passes guest gamepads to the host through its virtual USB gamepad driver. On Windows, those gamepads normally appear as Xbox 360 controllers, which CouchLink reads as XInput slots. Parsec's [gamepad setup guide](https://support.parsec.app/hc/en-us/articles/32381705301908-Setup-Gamepad) also covers virtual gamepad setup and controller order management.

If multiple guests join with controllers, Windows can expose multiple XInput slots. CouchLink can route up to four slots:

| CouchLink source | Windows XInput slot |
|---|---|
| Controller 1 | Slot 0 |
| Controller 2 | Slot 1 |
| Controller 3 | Slot 2 |
| Controller 4 | Slot 3 |

## Live Status

While streaming, the terminal prints status lines like:

```text
Controller 1 -> 07D37EB6 | source live | out +125 total 4300 | in 70 (reply 0.2s ago) | state buttons=0x1000 ...
```

Use this to confirm:

- The source controller is live.
- Packets are leaving the PC.
- The Pico is replying.
- Button and stick values change when the player presses input.

Bluetooth mode shows USB output counters instead of Pico UDP replies because live input is carried over the plugged-in USB cable.

## Direct Commands

The menu is the normal path. These commands are for scripts and launchers:

```powershell
.\couchlink.exe run --all
.\couchlink.exe run --route 1=07D37EB6 --route 2=523861E6
.\couchlink.exe run --route 1=192.168.50.4
.\couchlink.exe run --pico 07D37EB6
.\couchlink.exe run --pico 192.168.50.4
```

Use `.\couchlink.exe test discover --all` to list Pico UIDs before writing a route command.
If broadcast discovery fails but the router shows the Pico's IP, use
`.\couchlink.exe test discover --ip 192.168.50.4` or choose **Enter Pico
IP manually** in the guided menu.

## Bench Testing Note

In USB-output modes, the Pico USB side plugs into the console adapter. If you plug that output into the same Windows PC for testing, Windows may see the Pico itself as an Xbox controller. Do not pick that Pico output as the source controller; pick the Parsec virtual controller or a local test controller instead. Bluetooth mode is the exception: the Pico intentionally stays plugged into the bridge PC for USB CDC input.
