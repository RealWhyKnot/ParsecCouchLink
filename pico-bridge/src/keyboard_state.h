#pragma once

#include <stdint.h>

typedef struct {
    uint8_t modifiers;
    uint8_t keys[6];
} keyboard_state_t;

extern volatile keyboard_state_t g_keyboard_state;
