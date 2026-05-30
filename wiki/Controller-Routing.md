# Controller Routing

Run:

```powershell
.\couchlink.exe
```

Choose **Start or route controllers**. The app searches for every Pico that is running on Wi-Fi, shows the controllers Windows can currently see, then asks how to route them.

## Common Layouts

For one Pico:

1. Choose **Route one controller to one Pico**.
2. Pick the Pico.
3. Pick Controller 1, 2, 3, or 4.
4. Start streaming.

For two or more Picos:

1. Choose **Auto-route controllers to every detected Pico**.
2. Controller 1 goes to the first Pico, Controller 2 to the second Pico, and so on.
3. Start streaming.

For a custom layout:

1. Choose **Custom routing**.
2. Pick the Picos that should receive input.
3. Pick the source controller for each Pico.

The app saves the layout. After that, `couchlink.exe run` uses the saved layout without asking again, which is what the Startup shortcut uses.

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
.\couchlink.exe run --pico 07D37EB6
```

Use `.\couchlink.exe test discover --all` to list Pico UIDs before writing a route command.

## Bench Testing Note

In normal use, the Pico USB side plugs into the console adapter. If you plug the Pico into the same Windows PC for testing, Windows may see the Pico itself as an Xbox controller. Do not pick that Pico output as the source controller; pick the Parsec virtual controller or a local test controller instead.
