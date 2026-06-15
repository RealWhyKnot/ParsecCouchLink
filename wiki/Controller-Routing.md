# Controller Routing

Run:

```powershell
.\couchlink.exe
```

On the **Basic** tab, choose the Pico you want to use. Pick **Start streaming with Controller 1** for the normal one-controller path, or **Choose controller and stream** to select Controller 1, 2, 3, or 4 for that Pico.

## Common Layouts

For one Pico, use **Start streaming with Controller 1**.

For two or more Picos, Basic keeps actions under each Pico so you do not run a broad multi-Pico action by accident. Start the Pico you want, or use direct commands for scripted multi-Pico layouts:

```powershell
.\couchlink.exe run --all
.\couchlink.exe run --route 1=07D37EB6 --route 2=523861E6
```

Basic streaming actions save the selected layout. After that, `couchlink.exe run` uses the saved layout without asking again, which is what the Startup shortcut uses. Direct `run --all` and `run --route ...` commands use the layout on the command line.

## Keyboard Mode

Some console games need a keyboard instead of a controller -- Typing of the Dead on the Dreamcast is the usual reason. A Pico can present itself as a USB keyboard instead of an Xbox controller, and the bridge forwards the remote player's typing to it.

Only the remote Parsec player's keystrokes are forwarded -- the bridge captures Parsec-injected input, not anything typed locally at the host PC. This matches the controller path, which reads Parsec's virtual gamepad rather than a local one.

Switch a Pico to keyboard mode and start streaming:

```powershell
.\couchlink.exe keyboard
```

With one Pico, that is all you need. With several, pick one:

```powershell
.\couchlink.exe keyboard --pico 07D37EB6
```

The Pico stores the choice and reboots into it, so after a one-time switch a plain `couchlink.exe run` keeps streaming the keyboard to that board. Switch back to a controller the same way:

```powershell
.\couchlink.exe controller
```

Add `--no-stream` to either command to change the persona without starting a stream. The switch happens over Wi-Fi, so the Pico can stay plugged into the console the whole time.

A keyboard adapter is required on the console side: the Pico presents a standard USB HID boot keyboard, and the console adapter (for example a USB4MAPLE/Pico2Maple feeding a Dreamcast) maps it to the console's native keyboard. While streaming, the status line shows the keyboard instead of a controller:

```text
Keyboard -> 07D37EB6 | source live | out +60 total 900 | in 30 (reply 0.2s ago) | keys mods=0x02 keys=[0x04]
```

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

In normal use, the Pico USB side plugs into the console adapter. If you plug the Pico into the same Windows PC for testing, Windows may see the Pico itself as an Xbox controller. Do not pick that Pico output as the source controller; pick the Parsec virtual controller or a local test controller instead.
