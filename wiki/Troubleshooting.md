# Troubleshooting

If you've already tried the doctor and still can't get unstuck, jump
straight to [Reporting bugs](Reporting-Bugs.md) -- it covers what to
attach to an issue.

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

- Windows Firewall blocks UDP broadcast.
- The Pico joined a different Wi-Fi network.
- Router guest isolation blocks device-to-device traffic.
- The Pico is powered from the console side but too far from Wi-Fi.

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
