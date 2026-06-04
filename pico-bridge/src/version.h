#pragma once

#include <stdint.h>

#include "pico_bridge_build_version.h"

#if PICO_BRIDGE_FW_YEAR < 2020 || PICO_BRIDGE_FW_YEAR > 2099
#error "PICO_BRIDGE_FW_YEAR must fit the compact wire encoding"
#endif
#if PICO_BRIDGE_FW_MONTH < 1 || PICO_BRIDGE_FW_MONTH > 12
#error "PICO_BRIDGE_FW_MONTH must be 1..12"
#endif
#if PICO_BRIDGE_FW_DAY < 1 || PICO_BRIDGE_FW_DAY > 31
#error "PICO_BRIDGE_FW_DAY must be 1..31"
#endif
#if PICO_BRIDGE_FW_REVISION < 0 || PICO_BRIDGE_FW_REVISION > 255
#error "PICO_BRIDGE_FW_REVISION must fit one byte"
#endif
#if PICO_BRIDGE_FW_SUFFIX_LEN != 0 && PICO_BRIDGE_FW_SUFFIX_LEN != 4
#error "PICO_BRIDGE_FW_SUFFIX_LEN must be 0 or 4"
#endif

// Compact legacy fields kept for the fixed-size UDP ACK and older hosts.
// Host-side code recognizes 20..99 as a 20xx release year offset and
// displays YYYY.M.D.0 instead of a legacy triplet.
#define PICO_BRIDGE_FW_WIRE_MAJOR ((uint8_t)(PICO_BRIDGE_FW_YEAR - 2000))
#define PICO_BRIDGE_FW_WIRE_MINOR ((uint8_t)PICO_BRIDGE_FW_MONTH)
#define PICO_BRIDGE_FW_WIRE_PATCH ((uint8_t)PICO_BRIDGE_FW_DAY)

// Windows keys USB driver binding caches on bcdDevice. The generated
// value follows the firmware build date as BCD MM.DD so a new daily build
// invalidates stale setup-mode USB bindings without tying the descriptor
// cache directly to the product release suffix.

// On-wire protocol versions, must match wiki/Protocol.md.
#define PICO_BRIDGE_UDP_PROTO_VERSION 1
#define PICO_BRIDGE_CDC_PROTO_VERSION 1

// Board type byte reported in the UDP ack body and CDC HELLO_ACK.
#define PICO_BRIDGE_BOARD_PICO_2_W 0x01
#define PICO_BRIDGE_BOARD_PICO_W_RP2040 0x02

#if defined(PICO_BOARD_IS_PICO_2_W) || defined(RASPBERRYPI_PICO_2_W) || defined(PICO_RP2350)
#define PICO_BRIDGE_BOARD_TYPE PICO_BRIDGE_BOARD_PICO_2_W
#else
#define PICO_BRIDGE_BOARD_TYPE PICO_BRIDGE_BOARD_PICO_W_RP2040
#endif
