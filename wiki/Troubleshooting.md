# Troubleshooting

If you've already tried the doctor and still can't get unstuck, see
[Reporting bugs](Reporting-Bugs.md) for what to attach to an issue if
you want a maintainer to take a look.

Start with:

```powershell
.\couchlink.exe doctor
```

For a live log view:

```powershell
.\couchlink.exe logs --tail
```

For a shareable support bundle:

```powershell
.\couchlink.exe bundle
```

The bundle includes recent logs and diagnostic output. It does not include Wi-Fi credentials or SSID.

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

Try:

1. Unplug the Pico.
2. Plug it back in normally, without holding BOOTSEL.
3. Wait 5 seconds.
4. Rerun setup.

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

## Logs

Log location is printed by:

```powershell
.\couchlink.exe logs
```

Attach a support bundle instead of hand-copying logs when possible:

```powershell
.\couchlink.exe bundle
```
