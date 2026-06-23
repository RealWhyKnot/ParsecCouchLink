# Protocol

This page is for maintainers. Normal users should start with [[Quick Start]].

Parsec CouchLink has three small protocols:

- Runtime UDP over Wi-Fi while the bridge is streaming USB-output controller or keyboard state.
- USB-CDC setup mode while Wi-Fi credentials are being provisioned.
- USB-CDC Bluetooth input while a Bluetooth-mode Pico stays plugged into the bridge PC.

## Runtime UDP

- Port: UDP 4242.
- Packet size: controller, keyboard, heartbeat, discovery, and diagnostic request packets are 17 bytes. Diagnostic replies can be larger.
- Addressing: the bridge broadcasts discovery, then sends unicast to the Pico.
- Watchdog: if the Pico has not received a valid packet for 100 ms, it outputs a neutral state (controller centred / all keys released).

Bluetooth mode does not use runtime UDP for live controller packets. In Bluetooth mode the bridge writes the same controller state body over USB CDC to the Pico, and the Pico outputs Classic Bluetooth HID to the paired receiver.

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
| `0x0A` | `SET_PERSONA` -- bridge asks run-mode firmware to persist an output persona and reboot into it. `body[0]` is the persona (`0` = Xbox 360 / XInput, `1` = keyboard, `2` = Maple, `3` = PS3, `4` = PS4, `5` = Xbox One). Ignored if the Pico is already in that persona. |
| `0x0B` | `GET_VERSION` -- bridge requests the exact runtime firmware build. Same 17-byte request shape as the others; body is reserved. |
| `0x85` | `LOG_CHUNK` -- one variable-length reply chunk to `GET_LOG`. 12-byte header (chunk index, flags, total chunks, payload length, lost-bytes counter) + up to 256 bytes of log payload + CRC-16. The final chunk sets the `LAST_CHUNK` flag bit. |
| `0x86` | `USB_DIAG` -- fixed 78-byte reply to `GET_USB_DIAG` with USB mount/suspend state, descriptor counters, IN/OUT report counters, recent timestamps, and CRC-16. |
| `0x87` | `VERSION` -- fixed 17-byte reply to `GET_VERSION` with `year`, `month`, `day`, `revision`, and optional four-character development suffix. |

The controller fields match the standard XInput button, trigger, and stick layout so the bridge can copy the Windows XInput state directly into the packet body. The Xbox 360, Maple, PS3, PS4, Xbox One, generic HID, and Bluetooth personas consume the controller state body; the Pico maps the same state into the selected output report shape. Keyboard packets carry a standard USB HID boot-keyboard report, so a Pico in the keyboard persona processes `KEY_STATE`/`KEY_HEARTBEAT`. The bridge sends whichever packet type matches the persona the Pico advertised in its ack.

Compatibility is gated by protocol version. The bridge refuses to stream to a Pico that reports a different runtime protocol version. Capability bits in the ACK packet's `flags` byte advertise optional features without forcing a version bump: bit 0 (`LOG_CHUNK_SUPPORTED`) means the firmware will reply to `GET_LOG`; bit 1 (`USB_DIAG_SUPPORTED`) means it will reply to `GET_USB_DIAG`; bit 2 (`REBOOT_TO_SETUP_SUPPORTED`) means it accepts `REBOOT_TO_SETUP`; bit 3 (`KEYBOARD_PERSONA`) means the Pico is currently presenting the USB keyboard and accepts `SET_PERSONA` plus the keyboard packet types; bit 4 (`FULL_VERSION_SUPPORTED`) means it will reply to `GET_VERSION`; bit 5 (`MAPLE_PERSONA`) means Maple, or Xbox One when combined with bit 7; bit 6 (`DINPUT_PERSONA`) means PS3, or PS4 when combined with bit 7; bit 7 (`ALT_PERSONA`) extends those persona bits. Exact persona-bit combinations also identify Bluetooth targets: `ALT_PERSONA` alone is generic Bluetooth, `DINPUT_PERSONA|MAPLE_PERSONA|ALT_PERSONA` is Bluetooth Xbox Wireless Controller, and all four persona bits together select Bluetooth DualShock 4. Older firmware leaves these flags clear, and the bridge treats that as Xbox 360 / XInput.

The ACK keeps its compact legacy body for compatibility: protocol version, date triplet, board type, uptime, and short UID. When `FULL_VERSION_SUPPORTED` is set, the bridge immediately follows discovery with `GET_VERSION` so user-facing Wi-Fi status can show the exact firmware string, including suffixes such as `2026.6.15.0-0030`.

## USB-CDC Setup

Setup mode is used before the Pico has working Wi-Fi credentials, or when credentials are cleared. Bluetooth run mode intentionally keeps the same CDC identity so the bridge PC can send live controller input with one local USB hop.

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
| `BT_STATE` | Bluetooth run-mode input. Payload is one flags byte plus the 12-byte controller state body. No response is sent on success. |
| `BT_HEARTBEAT` | Bluetooth run-mode heartbeat with the same 13-byte payload shape. No response is sent on success. |
| `BT_GET_STATUS` | Bluetooth run-mode status query. Response includes the Bluetooth HID target, advertised name, connection flags, last status, channel id, and report counters. |

`HELLO_ACK` keeps the first six legacy identity bytes for older hosts:
protocol version, compact firmware date fields, board type, and flags. Newer
firmware appends `year`, `month`, `day`, `revision`, and an optional
four-character development suffix. Development builds report versions such as
`2026.5.29.7-D69A`; release builds report `2026.5.29.0`.

`HELLO_ACK` flags: bit 0 means saved Wi-Fi credentials are present, bit 1 means Wi-Fi is joined when reported, bit 2 means the firmware can reboot into run mode, and bit 3 means the CDC port is already active in run mode. The bridge uses bit 3 to avoid mistaking Bluetooth run-mode CDC for setup-mode recovery.

`BT_STATUS` payload version 4 starts with the version 1 fixed fields: `version`, `flags`, `target`, `last_status`, `report_len`, one reserved byte, `cid`, ten little-endian counters (`init`, `ready`, `open`, `close`, `can_send`, `report_build`, `report_send`, `send_request`, `last_event_ms`, `last_send_ms`). Flag bit 0 means the Bluetooth stack started, bit 1 means a receiver is connected, and bit 2 means a report send is queued. Version 2 added little-endian control-report counters: `get_report_count`, `get_report_success_count`, `get_report_unsupported_count`, `set_report_count`, `set_report_accepted_count`, `set_report_unsupported_count`, `out_report_count`, `out_report_accepted_count`, and `out_report_unsupported_count`; one-byte last report IDs/types for GET, SET, and interrupt OUT; two reserved bytes; and little-endian last payload lengths for GET, SET, and interrupt OUT. Version 3 appended pairing/security counters: PIN request/response, user-confirmation request/response, simple-pairing complete, authentication complete, link-key notification, encryption change, disconnection complete, HID open failure, `last_security_event_ms`, and the last security/open status bytes. Version 4 appends active HID reconnect counters (`reconnect_state`, cycle attempts, last status/reason, schedule/attempt/success/failure/blocked counts, and `last_reconnect_ms`), Classic ACL connection-complete status, and observed incoming HID L2CAP PSM/local CID counters. Byte 212 is the advertised-name length, followed by the advertised Classic Bluetooth HID name. Version 3 used byte 152 for the name length, version 2 used byte 98, and version 1 used byte 48. Current bridge builds decode versions 1, 2, and 3 with newer fields set to zero, and future higher versions decode the stable version 3 prefix while reporting the newer status version.

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

USB-output run modes do not expose this interface -- the XInput persona is deliberately minimal to keep `xusb22.sys` binding stable. In those run modes, diag retrieval uses UDP `GET_LOG` instead. Bluetooth run mode keeps the setup USB identity on the bridge PC so CDC can carry live controller input and diagnostics while Bluetooth carries the controller output.

The bridge's `couchlink bundle` tries CDC, vendor control, and UDP in order; the first to succeed wins, and `manifest.json`'s `pico_diag_source` records which path produced the captured log (`setup-cdc`, `vendor-control`, or `run-udp`).

## Runtime Persona

In run mode, the Pico presents one output persona, chosen by a persona byte stored alongside the Wi-Fi credentials in flash. Auto is a bridge-side selector that chooses one of the gamepad personas; it is not a firmware persona.

- **Xbox 360 / XInput** (default): a wired Xbox 360 controller (`0x045E:0x028E`, vendor class for `xusb22.sys`).
- **Keyboard**: a standard USB HID boot keyboard (`0x2E8A:0xCAF1`), for console games that need a keyboard such as Typing of the Dead on the Dreamcast.
- **Maple**: a Dreamcast Maple adapter mode fed by the same XInput state packets as the XInput persona. USB enumeration and reports intentionally match the wired Xbox 360 XInput persona (`0x045E:0x028E`) so USB-to-Maple adapters can translate it.
- **PS3**: a DualShock 3-style HID gamepad (`0x054C:0x0268`).
- **PS4**: a DualShock 4-style HID gamepad (`0x054C:0x09CC`).
- **Xbox One**: an Xbox One-compatible XGIP vendor-class gamepad.
- **Generic HID**: a USB HID gamepad for adapters that accept a simple HID descriptor.
- **Bluetooth**: a generic Classic Bluetooth HID gamepad target for wireless receivers. The Pico advertises as `CouchLink BT HID ...` and stays plugged into the bridge PC over USB CDC for live controller input.
- **Bluetooth Xbox**: a Classic Bluetooth HID target that advertises as `Xbox Wireless Controller` with Microsoft `0x045E:0x02FD` PnP identity and an Xbox Wireless Controller-style report descriptor. This is a Classic HID mimic, not Xbox BLE/HOGP mode.
- **Bluetooth PlayStation**: a DualShock 4 Classic Bluetooth HID target that advertises as `Wireless Controller` with Sony `0x054C:0x05C4` PnP identity and DS4 Bluetooth report `0x11`.

Only one persona is selected at a time. Because the runtime output is fixed at boot, switching persona persists the new value and reboots the board. USB-output personas usually have the Pico plugged into the console-side adapter, so the switch happens over Wi-Fi: `couchlink keyboard`, `couchlink xinput`, `couchlink xboxone`, `couchlink maple`, `couchlink ps3`, `couchlink ps4`, and `couchlink generic-hid` send a `SET_PERSONA` request, then wait for the board to rejoin Wi-Fi advertising the new persona. `couchlink bluetooth`, `couchlink bluetooth-xbox`, and `couchlink bluetooth-playstation` also switch over Wi-Fi, but streaming then requires the matching Pico to be plugged into the bridge PC over USB. `couchlink dinput` cycles PS3, generic HID, then PS4, and `couchlink auto` tries the USB gamepad personas and keeps the first one that the USB host polls. A record written before personas existed reads back as the XInput default, so existing boards keep working without re-provisioning. Setup mode and each USB runtime persona use USB identities chosen for their target host binding. Bluetooth personas keep CDC diagnostic USB on the bridge PC; `couchlink bundle` writes `bluetooth-report.txt` and skips the console USB adapter survey for those personas.

The PS3 persona uses report ID `0x01` with a 48-byte payload matching the native DualShock 3-style HID shape. The PS4 persona uses report ID `0x01` with a 63-byte payload matching the common DualShock 4-style controller input shape. Generic Bluetooth uses Classic HID report ID `0x01` with 20 buttons, a hat switch, and six 8-bit axes. Bluetooth Xbox uses report ID `0x01` with 16-bit sticks, 10-bit triggers, a hat, and 10 buttons. Bluetooth PlayStation uses DS4 Bluetooth report ID `0x11` with a 78-byte wire report. All gamepad personas map the same Parsec/XInput source buttons, triggers, and sticks. Rumble is not implemented for these personas.

Run-mode firmware also tracks what the USB host did after the Pico joined Wi-Fi. `couchlink test usb --all` asks the Pico over UDP whether the USB host completed configuration, whether the IN endpoint has accepted reports, and whether any host OUT traffic has arrived. `couchlink bundle` captures the same counters into `usb-diag.txt`, so a normal support ZIP contains both the firmware log ring and the current USB adapter verdict. The Pico cannot read the adapter's UI or driver name, but these counters distinguish the useful cases: no USB traffic, enumeration started but not configured, configured but not polling, polling, and polling plus OUT traffic. The report counters and verdicts are labelled for the active persona.
