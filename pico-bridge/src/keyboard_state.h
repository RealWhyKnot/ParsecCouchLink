#pragma once

#include <stdint.h>

// Shared keyboard state for the HID boot-keyboard persona. Mirrors the
// 8-byte USB HID boot report minus the constant reserved byte: one
// modifier bitmap (left/right Ctrl, Shift, Alt, GUI) plus up to six
// concurrently-held key usage codes (HID Keyboard/Keypad page 0x07).
//
// Single-writer / single-reader, same discipline as g_gamepad_state:
// `net_udp` writes it from the lwIP receive callback (which runs from the
// main loop under threadsafe-background cyw43), `hid_kbd` reads it when
// building each USB IN report. A neutral (all-keys-up) state is all
// zeros, which is also what the watchdog writes when the bridge goes
// quiet.
typedef struct {
    uint8_t modifiers;
    uint8_t keys[6];
} keyboard_state_t;

extern volatile keyboard_state_t g_keyboard_state;
