# Protocol

This page is for maintainers. Normal users should start with [[Quick Start]].

Parsec CouchLink has two small protocols:

- Runtime UDP over Wi-Fi while the bridge is streaming controller state.
- USB-CDC setup mode while Wi-Fi credentials are being provisioned.

## Runtime UDP

- Port: UDP 4242.
- Packet size: 17 bytes.
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
| `0x06` | `REBOOT_TO_BOOTSEL` -- bridge requests the Pico jump to the bootrom's USB bootloader (RPI-RP2 / RP2350 mass-storage drive) for unattended re-flashing. Same 17-byte shape; body is reserved. Pico replies with a normal `0x04` ACK before the jump so the bridge can confirm receipt. |
| `0x85` | `LOG_CHUNK` -- one variable-length reply chunk to `GET_LOG`. 12-byte header (chunk index, flags, total chunks, payload length, lost-bytes counter) + up to 256 bytes of log payload + CRC-16. The final chunk sets the `LAST_CHUNK` flag bit. |

The controller fields match the standard XInput button, trigger, and stick layout so the bridge can copy the Windows XInput state directly into the packet body.

Compatibility is gated by protocol version. The bridge refuses to stream to a Pico that reports a different runtime protocol version. Capability bits in the ACK packet's `flags` byte advertise optional features without forcing a version bump -- bit 0 (`LOG_CHUNK_SUPPORTED`) means the firmware will reply to a `GET_LOG` request. Older firmware leaves the flag clear, and the bridge gates its diag pull accordingly.

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
| `CLEAR_WIFI` | Erase saved Wi-Fi credentials. |
| `REBOOT_TO_RUN` | Reboot into runtime mode. |
| `SELF_TEST` | Firmware-side setup checks. |
| `GET_LOG_BUFFER` | Read the Pico diagnostic ring buffer. |
| `REBOOT_TO_BOOTSEL` | Jump to the bootrom USB bootloader so a new UF2 can be dropped onto the mass-storage drive without pressing the physical BOOTSEL button. Pico ACKs first and lets the TX FIFO drain before handing the bus to the bootrom. |

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
