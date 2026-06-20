#pragma once

#include <stdint.h>

#include "gamepad_state.h"
#include "joybus/common/n64_controller.h"

#define N64_STICK_MIN (-80)
#define N64_STICK_MAX 80
#define N64_C_BUTTON_THRESHOLD 16384

struct joybus_n64_controller_state n64_report_from_gamepad(const gamepad_state_t *state);
uint32_t n64_report_pack(const struct joybus_n64_controller_state *state);
struct joybus_n64_controller_state n64_report_unpack(uint32_t packed);
