# Protocol

This page is for maintainers. Normal users should start with [[Quick Start]].

Parsec CouchLink has two small protocols:

- Runtime UDP over Wi-Fi while the bridge is streaming controller state.
- USB-CDC setup mode while Wi-Fi credentials are being provisioned.

## Runtime UDP

- Port: UDP 4242.
- Packet size: controller, heartbeat, discovery, and diagnostic request packets are 17 bytes. Diagnostic replies can be larger.
- Addressing: the bridge broadcasts discovery, then sends unicast to the Pico.
- Watchdog: if the Pico has not received a valid packet for 100 ms, it outputs a neutral controller state.

Packet types:

| Type | Meaning |
|---|---|
| `0x01` | Controller state. |
| `0x02` | Heartbeat. |
| `0x03` | Discovery broadcast from the bridge. |
| `0x04` | Pico ack with firmware and board identity. |
| `0x05` | `GET_LOG` -- bridge requests the firmware diagnostic ring. Same 17-byte shape as the others; body is reserved. |
| `0x06` | `GET_USB_DIAG` -- bridge requests current run-mode USB/XInput status. Same 17-byte request shape as the others; body is reserved. |
| `0x07` | `REBOOT_TO_SETUP` -- bridge asks run-mode firmware to reboot into setup-mode USB-CDC so Wi-Fi can be changed. Same 17-byte request shape as the others; body is reserved. |
| `0x85` | `LOG_CHUNK` -- one variable-length reply chunk to `GET_LOG`. 12-byte header (chunk index, flags, total chunks, payload length, lost-bytes counter) + up to 256 bytes of log payload + CRC-16. The final chunk sets the `LAST_CHUNK` flag bit. |
| `0x86` | `USB_DIAG` -- fixed 78-byte reply to `GET_USB_DIAG` with USB mount/suspend state, descriptor counters, XInput IN/OUT counters, recent timestamps, and CRC-16. |

The controller fields match the standard XInput button, trigger, and stick layout so the bridge can copy the Windows XInput state directly into the packet body.

Compatibility is gated by protocol version. The bridge refuses to stream to a Pico that reports a different runtime protocol version. Capability bits in the ACK packet's `flags` byte advertise optional features without forcing a version bump: bit 0 (`LOG_CHUNK_SUPPORTED`) means the firmware will reply to `GET_LOG`; bit 1 (`USB_DIAG_SUPPORTED`) means it will reply to `GET_USB_DIAG`; bit 2 (`REBOOT_TO_SETUP_SUPPORTED`) means it accepts `REBOOT_TO_SETUP`. Older firmware leaves these flags clear, and the bridge gates diagnostic pulls accordingly.

## USB-CDC Setup

Setup mode is used before the Pico has working Wi-Fi credentials, or when credentials are cleared.

- USB VID/PID: `0x2E8A:0xCAF0`.
- Transport: CDC ACM virtual COM port.
- Framing: magic, protocol version, command, payload length, sequence, payload, CRC-16.
- Password handling: the bridge clears the password buffer after sending. The Pico clears its receive buffer after writing flash.

Commands:

| Command | Purpose |
|---|---|
| `HELLO` | Read firmware, board, and credential status. |
| `GET_STATUS` | Read Wi-Fi state and last setup error. |
| `SET_WIFI` | Store SSID and password in Pico flash. |
| `REBOOT_TO_RUN` | Reboot into runtime mode. |
| `SELF_TEST` | Firmware-side setup checks. |
| `GET_LOG_BUFFER` | Read the Pico diagnostic ring buffer. |
| `REBOOT_TO_BOOTSEL` | Reboot into the ROM BOOTSEL USB bootloader for reflashing. |

`HELLO_ACK` keeps the first six legacy identity bytes for older hosts:
protocol version, compact firmware date fields, board type, and flags. Newer
firmware appends `year`, `month`, `day`, `revision`, and an optional
four-character development suffix. Development builds report versions such as
`2026.5.29.7-D69A`; release builds report `2026.5.29.0`.

## USB Vendor Diag (setup mode)

Setup mode's composite also exposes a vendor-class interface (interface 2, class `0xFF`) that Windows binds to WinUSB. MS OS 2.0 descriptors advertise the binding, so no INF file is needed on Windows 8.1+. The host reads the firmware diagnostic ring buffer via a vendor IN control transfer on EP0, which works regardless of CDC bulk endpoint state -- diag retrieval no longer relies on the CDC FIFO being drained.

Control transfer:

| Field | Value |
|---|---|
| `bmRequestType` | `0xC1` (vendor IN, interface) |
| `bRequest` | `0x01` (`GET_DIAG_LOG`) |
| `wIndex` low byte | `2` (interface number) |
| `wLength` | up to `4100` (4-byte header + 4 KiB ring) |

Response payload matches the CDC `GET_LOG_BUFFER` body: a 4-byte little-endian lost-bytes counter, followed by the most-recent ring contents.

Run mode does not expose this interface -- the XInput persona is deliberately minimal to keep `xusb22.sys` binding stable. In run mode, diag retrieval uses UDP `GET_LOG` instead.

The bridge's `couchlink bundle` tries CDC, vendor control, and UDP in order; the first to succeed wins, and `manifest.json`'s `pico_diag_source` records which path produced the captured log (`setup-cdc`, `vendor-control`, or `run-udp`).

## USB Runtime Persona

In run mode, the Pico presents itself as a wired Xbox 360 controller. Setup mode and run mode use different USB IDs so Windows does not reuse the wrong driver binding across modes.

Run-mode firmware also tracks what the USB host did after the Pico joined Wi-Fi. `couchlink test usb --all` asks the Pico over UDP whether the USB host completed configuration, whether the XInput IN endpoint has accepted reports, and whether any host OUT traffic such as rumble or LED commands has arrived. The Pico cannot read the adapter's UI or driver name, but these counters distinguish the useful cases: no USB traffic, enumeration started but not configured, configured but not polling, polling XInput, and polling plus OUT traffic.
