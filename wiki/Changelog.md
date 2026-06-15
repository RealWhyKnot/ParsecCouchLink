# Changelog

## Current Release

See the repository changelog for the generated release list:

https://github.com/RealWhyKnot/ParsecCouchLink/blob/main/CHANGELOG.md

Latest published release:

https://github.com/RealWhyKnot/ParsecCouchLink/releases/latest

## Current Highlights

- `v2026.6.14.1` keeps provisioned Picos in run mode on normal replug so they enumerate as Xbox 360 controllers instead of falling back to setup-mode USB.
- The no-argument guided menu opens on a **Basic** tab with one entry per saved or detected Pico. Each entry exposes only commands for that Pico, such as streaming, Wi-Fi setup, recovery, firmware flashing, USB diagnostics, saving, or removal.
- Direct `couchlink run` can auto-recover setup-mode USB Picos that already have saved Wi-Fi by rebooting them back to Wi-Fi/controller mode and retrying discovery.
- `couchlink debug` provides guided Pico recovery plus direct mode-switch commands for Wi-Fi/controller mode, USB debug mode, and BOOTSEL firmware mode.
- `couchlink test usb` asks run-mode firmware over Wi-Fi for USB/XInput status, including mount/configuration state, descriptor counters, accepted IN reports, and host OUT traffic.
- `couchlink lab` runs unattended plugged-in Pico bench scenarios, including `--power pnp-remove` for Windows PnP remove/rescan reconnect coverage.
- `couchlink bundle` gathers logs, doctor output, Windows USB event-log snippets, bridge state journal entries, and firmware diagnostics when reachable by setup USB, vendor control, or run-mode UDP.
- Release zips include `setup.ps1`, `couchlink.exe`, both board-specific UF2 files, support wrappers, and a manifest.
