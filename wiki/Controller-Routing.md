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
