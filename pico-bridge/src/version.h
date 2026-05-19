#pragma once

// Firmware semver. Bump fw_major when the wire or CDC protocol breaks
// compatibility. The bridge refuses to talk to a Pico whose proto_version
// differs. fw_minor bumps land in bcdDevice via usb_descriptors.c, so
// Windows re-binds usbser.sys after a re-flash and does not reuse cached
// descriptors from a different interface layout -- bump it on USB
// composite-layout changes too.
#define PICO_BRIDGE_FW_MAJOR 0
#define PICO_BRIDGE_FW_MINOR 2
#define PICO_BRIDGE_FW_PATCH 0

// On-wire protocol versions, must match wiki/Protocol.md.
#define PICO_BRIDGE_UDP_PROTO_VERSION 1
#define PICO_BRIDGE_CDC_PROTO_VERSION 1

// Board type byte reported in the UDP ack body and CDC HELLO_ACK.
#define PICO_BRIDGE_BOARD_PICO_2_W       0x01
#define PICO_BRIDGE_BOARD_PICO_W_RP2040  0x02

#if defined(PICO_BOARD_IS_PICO_2_W) || defined(RASPBERRYPI_PICO_2_W) || defined(PICO_RP2350)
#define PICO_BRIDGE_BOARD_TYPE PICO_BRIDGE_BOARD_PICO_2_W
#else
#define PICO_BRIDGE_BOARD_TYPE PICO_BRIDGE_BOARD_PICO_W_RP2040
#endif
