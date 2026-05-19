# Changelog

## Unreleased

- Bridge log files are no longer empty by default. The tracing filter directive matched a crate-name prefix that the binary's actual crate identifier did not satisfy, so every `info!` / `warn!` / `error!` call dropped silently on both stderr and the rotating file. `couchlink run` is no longer a silent console window; bundles now carry readable bridge logs.
- `couchlink bundle` can now pull the firmware's diag-log ring over UDP from a running Pico. The bundle tries setup-mode USB-CDC first and falls back to a unicast UDP probe against the last-known address. `pico-diag.txt` and the manifest now name which source produced the log (`setup-cdc` or `run-udp`).
- When CDC capture fails, `pico-diag.txt` now names the specific step that broke -- port enumeration, port open, HELLO write, HELLO read, or frame decode -- with bytes-received count and a hex dump of any pre-magic bytes seen on the wire. The previous stub said "couldn't capture, see the bridge log" without distinguishing which path failed.
- `couchlink flash` refuses to write a UF2 whose family ID does not match the detected BOOTSEL drive. The previous behavior was a warning that continued the copy, which produced a Pico that silently never re-enumerated when an RP2040 image was dropped onto an RP2350 (or vice versa).
- `couchlink setup` stage 4 waits up to 120 s (was 60 s) for the setup-mode CDC port to appear and prints a progress line every 10 s while it waits. The previous 60 s budget was tight when the host went through USB passthrough (VM, WSL2) or a first-time driver bind.
- `couchlink setup` stage 4 treats a port-disappeared event right after `REBOOT_TO_RUN` as success rather than a hard error. The firmware always reboots after handling that command, so a missing reply is the expected outcome of a fast reboot, not a failure.
- Firmware CDC `bcdDevice` is derived from the firmware semver macros (`PICO_BRIDGE_FW_MAJOR/MINOR`) so Windows re-binds `usbser.sys` after a CDC-protocol break instead of reusing a cached binding from an older interface layout.
- Release zip now stages `setup.ps1`, `couchlink.exe`, and `couchlink-pico.uf2` together.
- Wiki pages now hold setup, flashing, troubleshooting, build, and protocol notes.
