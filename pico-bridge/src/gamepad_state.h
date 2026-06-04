#pragma once

#include <stdint.h>

// The shared gamepad state struct. Single-writer / single-reader:
// `net_udp` writes it from the lwIP receive callback (CYW43 thread-safe
// background mode means callbacks run from the main loop, not an
// interrupt, so a memory fence at write time is enough). `xinput` reads
// it when building each USB IN report.
//
// Field layout deliberately mirrors XINPUT_GAMEPAD so the bridge can
// copy state bytes through with no remapping.
typedef struct {
    uint16_t buttons;
    uint8_t left_trigger;
    uint8_t right_trigger;
    int16_t left_x;
    int16_t left_y;
    int16_t right_x;
    int16_t right_y;
} gamepad_state_t;

// Watchdog: zeroed by `watchdog` if `last_packet_ms` is more than 100 ms
// stale. Updated by `net_udp` on every valid packet. Stored as uint32_t
// (milliseconds since boot) so 32-bit loads on Cortex-M are atomic; 49-day
// wrap is handled by unsigned-subtraction arithmetic.
extern volatile gamepad_state_t g_gamepad_state;
extern volatile uint32_t g_last_packet_ms;
extern volatile uint8_t g_parsec_connected; // mirrors the FLAG_PARSEC_CONNECTED bit
