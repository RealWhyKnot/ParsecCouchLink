#include "n64_report.h"

#include "dinput_report.h"

typedef char
    n64_state_must_match_wire_size[(sizeof(struct joybus_n64_controller_state) == 4) ? 1 : -1];

static int8_t scale_axis_to_n64(int16_t value) {
    if (value == INT16_MIN)
        return N64_STICK_MIN;

    int32_t scaled = ((int32_t)value * N64_STICK_MAX) / INT16_MAX;
    if (scaled > N64_STICK_MAX)
        scaled = N64_STICK_MAX;
    if (scaled < N64_STICK_MIN)
        scaled = N64_STICK_MIN;
    return (int8_t)scaled;
}

struct joybus_n64_controller_state n64_report_from_gamepad(const gamepad_state_t *state) {
    struct joybus_n64_controller_state out = {0};
    uint16_t buttons = state ? state->buttons : 0;

    if (buttons & DINPUT_XINPUT_A)
        out.buttons |= JOYBUS_N64_BUTTON_A;
    if (buttons & DINPUT_XINPUT_B)
        out.buttons |= JOYBUS_N64_BUTTON_B;
    if (buttons & DINPUT_XINPUT_START)
        out.buttons |= JOYBUS_N64_BUTTON_START;

    if (buttons & DINPUT_XINPUT_DPAD_UP)
        out.buttons |= JOYBUS_N64_BUTTON_UP;
    if (buttons & DINPUT_XINPUT_DPAD_DOWN)
        out.buttons |= JOYBUS_N64_BUTTON_DOWN;
    if (buttons & DINPUT_XINPUT_DPAD_LEFT)
        out.buttons |= JOYBUS_N64_BUTTON_LEFT;
    if (buttons & DINPUT_XINPUT_DPAD_RIGHT)
        out.buttons |= JOYBUS_N64_BUTTON_RIGHT;

    if (buttons & DINPUT_XINPUT_LEFT_SHOULDER)
        out.buttons |= JOYBUS_N64_BUTTON_L;
    if ((buttons & DINPUT_XINPUT_RIGHT_SHOULDER) ||
        (state && state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD))
        out.buttons |= JOYBUS_N64_BUTTON_R;
    if (state && state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out.buttons |= JOYBUS_N64_BUTTON_Z;

    if (state) {
        if (state->right_x <= -N64_C_BUTTON_THRESHOLD)
            out.buttons |= JOYBUS_N64_BUTTON_C_LEFT;
        if (state->right_x >= N64_C_BUTTON_THRESHOLD)
            out.buttons |= JOYBUS_N64_BUTTON_C_RIGHT;
        if (state->right_y <= -N64_C_BUTTON_THRESHOLD)
            out.buttons |= JOYBUS_N64_BUTTON_C_DOWN;
        if (state->right_y >= N64_C_BUTTON_THRESHOLD)
            out.buttons |= JOYBUS_N64_BUTTON_C_UP;

        out.stick_x = scale_axis_to_n64(state->left_x);
        out.stick_y = scale_axis_to_n64(state->left_y);
    }

    return out;
}

uint32_t n64_report_pack(const struct joybus_n64_controller_state *state) {
    if (!state)
        return 0;
    return ((uint32_t)(state->buttons & 0xFFu)) | ((uint32_t)(state->buttons >> 8) << 8) |
           ((uint32_t)(uint8_t)state->stick_x << 16) | ((uint32_t)(uint8_t)state->stick_y << 24);
}

struct joybus_n64_controller_state n64_report_unpack(uint32_t packed) {
    struct joybus_n64_controller_state state = {
        .buttons = (uint16_t)(packed & 0xFFFFu),
        .stick_x = (int8_t)((packed >> 16) & 0xFFu),
        .stick_y = (int8_t)((packed >> 24) & 0xFFu),
    };
    return state;
}
