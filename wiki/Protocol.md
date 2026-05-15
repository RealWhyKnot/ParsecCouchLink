# Protocol

This page is for maintainers. Normal users should start with [[Quick Start]].

ParsecToDreamcast has two small protocols:

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

The controller fields match the standard XInput button, trigger, and stick layout so the bridge can copy the Windows XInput state directly into the packet body.

Compatibility is gated by protocol version. The bridge refuses to stream to a Pico that reports a different runtime protocol version.

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

## USB Runtime Persona

In run mode, the Pico presents itself as a wired Xbox 360 controller. Setup mode and run mode use different USB IDs so Windows does not reuse the wrong driver binding across modes.
