# Troubleshooting

If you've already tried the doctor and still can't get unstuck, see
[Reporting bugs](Reporting-Bugs.md) for what to attach to an issue if
you want a maintainer to take a look.

Start with:

```powershell
.\couchlink.exe doctor
```

For the normal guided view, run:

```powershell
.\couchlink.exe
```

The **Basic** tab shows each saved or detected Pico separately. Each Pico has
its own commands for streaming, Wi-Fi setup, recovery, firmware flashing, USB
checks, saving, or removal depending on its current state.

For a live log view:

```powershell
.\couchlink.exe logs --tail
```

For a shareable support bundle:

```powershell
.\couchlink.exe bundle
```

The bundle includes recent logs and diagnostic output. It does not include Wi-Fi credentials or SSID.

## Pico Debug And Recovery

If the Pico feels "lost", use the debug menu first:

```powershell
.\couchlink.exe debug
```

The menu checks all three recoverable states:

- **Wi-Fi/controller mode**: the normal running mode. The bridge can ask this Pico to reboot into USB debug mode.
- **USB debug mode**: the setup USB serial mode. From here you can read logs, change Wi-Fi, switch back to Wi-Fi/controller mode, or enter BOOTSEL.
- **BOOTSEL firmware mode**: the hardware fallback used by firmware update.

Useful direct commands:

```powershell
.\couchlink.exe debug --status
.\couchlink.exe debug --to-usb-debug
.\couchlink.exe debug --to-wifi --port COM3
.\couchlink.exe debug --to-bootsel
.\couchlink.exe bootsel --port COM3
.\couchlink.exe debug --logs
```

If none of those modes show a Pico, use BOOTSEL flashing. Hold BOOTSEL while
plugging the Pico into this PC, then run the guided firmware update.

## Setup Cannot Find The Pico In BOOTSEL

Check:

- The Pico was plugged in while BOOTSEL was held.
- Windows shows a removable drive named `RPI-RP2` or `RP2350`.
- The USB cable supports data.
- No other file copy is already in progress.

Then rerun:

```powershell
powershell -ExecutionPolicy Bypass -File .\setup.ps1
```

## Setup Cannot Find USB Serial Mode

This happens after flashing if the Pico did not boot the firmware setup mode.

If the Pico already had saved Wi-Fi, it may have booted straight into run mode
as an Xbox controller. In the guided menu, choose **Use current Wi-Fi and stop**
if you only updated firmware. If you need to change Wi-Fi, choose the option to
reboot the running Pico into setup mode.

If neither setup-mode USB nor Wi-Fi discovery appears, try:

1. Unplug the Pico.
2. Plug it back in normally, without holding BOOTSEL.
3. Wait 5 seconds.
4. Run `.\couchlink.exe debug --status`.
5. Rerun setup.

If it still fails, flash again.

## Wi-Fi Provisioning Fails

Check:

- The SSID is 2.4 GHz.
- The password is correct.
- The PC and Pico are on the same LAN.
- Guest network isolation is off.

Run:

```powershell
.\couchlink.exe configure-wifi
```

## Discovery Fails

Run:

```powershell
.\couchlink.exe test discover
```

If the guided menu says `No Pico replied on Wi-Fi`, that is not a
controller problem. It means the PC did not receive a UDP discovery reply
from a running Pico.

The **Basic** tab shows setup USB Picos separately. If a Pico has saved Wi-Fi,
use that Pico's **Recover to Wi-Fi/controller mode** command before streaming.

Try this in order:

1. Confirm the Pico is powered and not sitting in BOOTSEL. If it appears
   as setup-mode USB, use `configure-wifi` first.
2. Run `.\couchlink.exe recover`, then choose that Pico's streaming command on the **Basic** tab.
3. Run `.\couchlink.exe test discover --all`.
4. If the router admin page shows the Pico's IP, probe it directly:

   ```powershell
   .\couchlink.exe test discover --ip 192.168.50.4
   .\couchlink.exe run --pico 192.168.50.4
   ```

   The guided menu also shows saved Picos with a last-IP probe when automatic
   Wi-Fi discovery finds nothing.
5. If the router name or password changed, run `.\couchlink.exe
   configure-wifi`.
6. If `configure-wifi` cannot find the Pico, use BOOTSEL flashing in
   [[Setup and Flashing]], then run `configure-wifi` again.
7. Run `.\couchlink.exe doctor`.
8. Run `.\couchlink.exe bundle` and attach the ZIP to a bug report.

Common causes:

- Windows Firewall blocks UDP broadcast. The bridge will print a
  `New-NetFirewallRule` command the first time it sees a bind / self-ping
  failure -- copy that into an elevated PowerShell prompt to allow inbound
  UDP for the bridge.
- The Pico joined a different Wi-Fi network. Verify with
  `.\couchlink.exe doctor`.
- Router AP isolation blocks device-to-device traffic. Many consumer
  routers and APs ship with a feature called "AP isolation" or "Client
  isolation" (the exact name varies -- UniFi, Eero, and a number of ISP
  gateways have it). When enabled, two clients on the same Wi-Fi cannot
  see each other, so UDP discovery never reaches the Pico. Disable it in
  your router's admin UI, or move the Pico and PC onto a network without
  it.
- Multi-homed Windows PC -- if your PC has both Ethernet and Wi-Fi
  connected at the same time, broadcast traffic can go out the wrong
  adapter and never reach the Pico on the Wi-Fi side. The bridge tries
  to broadcast on every active interface; if it still misses, temporarily
  disable the adapter that does not lead to the Pico.
- The Pico is powered from the console side but too far from Wi-Fi.

## Wi-Fi Country Code (EU / UK channels 12 and 13)

The shipped firmware uses the `CYW43_COUNTRY_WORLDWIDE` country code by
default. That is safe to ship anywhere but excludes channels 12 and 13
on 2.4 GHz, which some EU / UK routers use. If your AP is on channel
12 or 13 and the Pico can't see it, rebuild the firmware with a country
code matching your locale:

```powershell
cmake -S pico-bridge -B build -DPICO_BRIDGE_WIFI_COUNTRY=CYW43_COUNTRY_UK
cmake --build build
```

Use one of the `CYW43_COUNTRY_*` macros documented in
`pico-sdk/src/rp2_common/pico_cyw43_arch/include/pico/cyw43_arch.h`.

## Controller Input Is Missing

Run:

```powershell
.\couchlink.exe test xinput
```

Parsec must expose the remote controller as an XInput gamepad on the host. A local wired Xbox controller is also enough for bench testing.

If streaming starts but the console does not react, run the guided menu again:

```powershell
.\couchlink.exe
```

On the **Basic** tab, choose the Pico, then use **Start streaming with Controller 1** or **Choose controller and stream**. Verify that the right Controller 1-4 source is mapped to the right Pico. While streaming, the terminal should show outbound packet counts increasing and recent Pico replies. If the source says `waiting for source`, Windows does not currently see that XInput slot.

## USB Adapter Does Not See The Pico

If the Pico is on Wi-Fi but the console adapter or a test PC does not see it as a controller, run:

```powershell
.\couchlink.exe test usb --all
```

Or, if discovery misses the Pico but you know its IP:

```powershell
.\couchlink.exe test usb --ip 192.168.50.4
```

The result tells you where USB stopped:

- `Pico sees no USB host enumeration traffic`: check cable, adapter power, and port.
- `started enumeration but did not configure`: the host read descriptors but did not accept the device.
- `configured, but the host has not accepted a <persona> report`: unplug and replug the Pico, then update firmware if it repeats.
- `host is polling the <persona> endpoint`: the adapter accepted the Pico as a controller. Some adapters do not send rumble, LED, or output traffic until a game starts.

On a development bench where the Pico is plugged into the Windows host, use
[[Hardware Lab]] for repeatable reconnect checks. `couchlink lab --scenario full
--power pnp-remove --no-flash` removes and rescans the CouchLink setup and
XInput PnP instances, then verifies Wi-Fi/controller detection and signal
delivery after reconnect. It is still a Windows PnP simulation, not a physical
power cut.

## Logs

Log location is printed by:

```powershell
.\couchlink.exe logs
```

Attach a support bundle instead of hand-copying logs when possible:

```powershell
.\couchlink.exe bundle
```
