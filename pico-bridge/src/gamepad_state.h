#pragma once

#include <stdint.h>

typedef struct {
    uint16_t buttons;
    uint8_t left_trigger;
    uint8_t right_trigger;
    int16_t left_x;
    int16_t left_y;
    int16_t right_x;
    int16_t right_y;
} gamepad_state_t;

extern volatile gamepad_state_t g_gamepad_state;
extern volatile uint32_t g_last_packet_ms;
extern volatile uint8_t g_parsec_connected;
