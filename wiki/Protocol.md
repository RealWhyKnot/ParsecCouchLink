# Protocol

This page is for maintainers. Normal users should start with [[Quick Start]].

Parsec CouchLink has two small protocols:

- Runtime UDP over Wi-Fi while the bridge is streaming controller or keyboard state.
- USB-CDC setup mode while Wi-Fi credentials are being provisioned.

## Runtime UDP

- Port: UDP 4242.
- Packet size: controller, keyboard, heartbeat, discovery, and diagnostic request packets are 17 bytes. Diagnostic replies can be larger.
- Addressing: the bridge broadcasts discovery, then sends unicast to the Pico.
- Watchdog: if the Pico has not received a valid packet for 100 ms, it outputs a neutral state (controller centred / all keys released).

Packet types:

| Type | Meaning |
|---|---|
| `0x01` | Controller state. |
| `0x02` | Heartbeat. |
| `0x03` | Discovery broadcast from the bridge. |
| `0x04` | Pico ack with firmware and board identity. |
| `0x05` | `GET_LOG` -- bridge requests the firmware diagnostic ring. Same 17-byte shape as the others; body is reserved. |
| `0x06` | `GET_USB_DIAG` -- bridge requests current run-mode USB status. Same 17-byte request shape as the others; body is reserved. |
| `0x07` | `REBOOT_TO_SETUP` -- bridge asks run-mode firmware to reboot into setup-mode USB-CDC so Wi-Fi can be changed. Same 17-byte request shape as the others; body is reserved. |
| `0x08` | `KEY_STATE` -- keyboard report for the keyboard persona. The 8-byte USB HID boot report sits in the first 8 body bytes: modifier bitmap, reserved, then six key usage codes. |
| `0x09` | `KEY_HEARTBEAT` -- keyboard heartbeat, sent when the report is unchanged so the firmware watchdog stays fed. |
| `0x0A` | `SET_PERSONA` -- bridge asks run-mode firmware to persist an output persona and reboot into it. `body[0]` is the persona (`0` = controller, `1` = keyboard; other values are treated as controller). Ignored if the Pico is already in that persona. |
| `0x0B` | `GET_VERSION` -- bridge requests the exact runtime firmware build. Same 17-byte request shape as the others; body is reserved. |
| `0x85` | `LOG_CHUNK` -- one variable-length reply chunk to `GET_LOG`. 12-byte header (chunk index, flags, total chunks, payload length, lost-bytes counter) + up to 256 bytes of log payload + CRC-16. The final chunk sets the `LAST_CHUNK` flag bit. |
| `0x86` | `USB_DIAG` -- fixed 78-byte reply to `GET_USB_DIAG` with USB mount/suspend state, descriptor counters, IN/OUT report counters, recent timestamps, and CRC-16. |
| `0x87` | `VERSION` -- fixed 17-byte reply to `GET_VERSION` with `year`, `month`, `day`, `revision`, and optional four-character development suffix. |

The controller fields match the standard XInput button, trigger, and stick layout so the bridge can copy the Windows XInput state directly into the packet body. The controller persona consumes `STATE`/`HEARTBEAT` and emits wired Xbox 360-compatible USB reports. Keyboard packets carry a standard USB HID boot-keyboard report, so a Pico in the keyboard persona processes `KEY_STATE`/`KEY_HEARTBEAT`. The bridge sends whichever packet type matches the persona the Pico advertised in its ack.

Compatibility is gated by protocol version. The bridge refuses to stream to a Pico that reports a different runtime protocol version. Capability bits in the ACK packet's `flags` byte advertise optional features without forcing a version bump: bit 0 (`LOG_CHUNK_SUPPORTED`) means the firmware will reply to `GET_LOG`; bit 1 (`USB_DIAG_SUPPORTED`) means it will reply to `GET_USB_DIAG`; bit 2 (`REBOOT_TO_SETUP_SUPPORTED`) means it accepts `REBOOT_TO_SETUP`; bit 3 (`KEYBOARD_PERSONA`) means the Pico is currently presenting the USB keyboard and accepts `SET_PERSONA` plus the keyboard packet types; bit 4 (`FULL_VERSION_SUPPORTED`) means it will reply to `GET_VERSION`. Older firmware leaves these flags clear, and the bridge gates behaviour accordingly.

The ACK keeps its compact legacy body for compatibility: protocol version, date triplet, board type, uptime, and short UID. When `FULL_VERSION_SUPPORTED` is set, the bridge immediately follows discovery with `GET_VERSION` so user-facing Wi-Fi status can show the exact firmware string, including suffixes such as `2026.6.15.0-0030`.

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

## Runtime Persona

In run mode, the Pico presents one of two output personas, chosen by a persona byte stored alongside the Wi-Fi credentials in flash:

- **Controller** (default): a wired Xbox 360 controller (`0x045E:0x028E`, vendor class for `xusb22.sys`).
- **Keyboard**: a standard USB HID boot keyboard (`0x2E8A:0xCAF1`), for console games that need a keyboard such as Typing of the Dead on the Dreamcast.

Only one persona is selected at a time. Because the runtime output is fixed at boot, switching persona persists the new value and reboots the board. The Pico lives plugged into the console rather than the host, so the switch happens over Wi-Fi: `couchlink keyboard` / `couchlink controller` send a `SET_PERSONA` request, then wait for the board to rejoin Wi-Fi advertising the new persona. A record written before personas existed reads back as the controller default, and reserved persona values also fall back to controller, so existing boards keep working without re-provisioning. Setup mode, keyboard mode, and controller mode use USB identities chosen for their target host binding.

Run-mode firmware also tracks what the USB host did after the Pico joined Wi-Fi. `couchlink test usb --all` asks the Pico over UDP whether the USB host completed configuration, whether the IN endpoint has accepted reports, and whether any host OUT traffic (controller rumble/LED, or keyboard LED-lock reports) has arrived. The Pico cannot read the adapter's UI or driver name, but these counters distinguish the useful cases: no USB traffic, enumeration started but not configured, configured but not polling, polling, and polling plus OUT traffic. The report counters and verdicts are labelled for the active persona; controller expects Xbox 360-compatible polling, while keyboard expects HID keyboard polling.
