#include "dinput_report.h"

#include <stdbool.h>

static void put_le16(uint8_t *dst, uint16_t value) {
    dst[0] = (uint8_t)(value & 0xFFu);
    dst[1] = (uint8_t)((value >> 8) & 0xFFu);
}

uint8_t dinput_axis_x_to_hid(int16_t value) {
    int32_t scaled = ((int32_t)value + 32768) * 255 / 65535;
    if (scaled < 0)
        return 0;
    if (scaled > 255)
        return 255;
    return (uint8_t)scaled;
}

uint8_t dinput_axis_y_to_hid(int16_t value) {
    int32_t scaled = ((int32_t)32767 - value) * 255 / 65535;
    if (scaled < 0)
        return 0;
    if (scaled > 255)
        return 255;
    return (uint8_t)scaled;
}

uint8_t dinput_hat_from_buttons(uint16_t buttons) {
    bool up = (buttons & DINPUT_XINPUT_DPAD_UP) != 0;
    bool down = (buttons & DINPUT_XINPUT_DPAD_DOWN) != 0;
    bool left = (buttons & DINPUT_XINPUT_DPAD_LEFT) != 0;
    bool right = (buttons & DINPUT_XINPUT_DPAD_RIGHT) != 0;

    if (up && !down) {
        if (right && !left)
            return DINPUT_HAT_UP_RIGHT;
        if (left && !right)
            return DINPUT_HAT_UP_LEFT;
        return DINPUT_HAT_UP;
    }
    if (down && !up) {
        if (right && !left)
            return DINPUT_HAT_DOWN_RIGHT;
        if (left && !right)
            return DINPUT_HAT_DOWN_LEFT;
        return DINPUT_HAT_DOWN;
    }
    if (right && !left)
        return DINPUT_HAT_RIGHT;
    if (left && !right)
        return DINPUT_HAT_LEFT;
    return DINPUT_HAT_NEUTRAL;
}

uint16_t dinput_buttons_from_gamepad(const gamepad_state_t *state) {
    uint16_t out = 0;
    uint16_t buttons = state->buttons;

    if (buttons & DINPUT_XINPUT_A)
        out |= DINPUT_BUTTON_A;
    if (buttons & DINPUT_XINPUT_B)
        out |= DINPUT_BUTTON_B;
    if (buttons & DINPUT_XINPUT_X)
        out |= DINPUT_BUTTON_X;
    if (buttons & DINPUT_XINPUT_Y)
        out |= DINPUT_BUTTON_Y;
    if (buttons & DINPUT_XINPUT_LEFT_SHOULDER)
        out |= DINPUT_BUTTON_LB;
    if (buttons & DINPUT_XINPUT_RIGHT_SHOULDER)
        out |= DINPUT_BUTTON_RB;
    if (buttons & DINPUT_XINPUT_BACK)
        out |= DINPUT_BUTTON_BACK;
    if (buttons & DINPUT_XINPUT_START)
        out |= DINPUT_BUTTON_START;
    if (buttons & DINPUT_XINPUT_LEFT_THUMB)
        out |= DINPUT_BUTTON_LS;
    if (buttons & DINPUT_XINPUT_RIGHT_THUMB)
        out |= DINPUT_BUTTON_RS;
    if (state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out |= DINPUT_BUTTON_LT_DIGITAL;
    if (state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out |= DINPUT_BUTTON_RT_DIGITAL;
    if (buttons & DINPUT_XINPUT_GUIDE)
        out |= DINPUT_BUTTON_HOME;

    return out;
}

void dinput_build_report(const gamepad_state_t *state, dinput_report_t *out) {
    out->bytes[0] = DINPUT_REPORT_ID;
    out->bytes[1] = dinput_hat_from_buttons(state->buttons);
    out->bytes[2] = dinput_axis_x_to_hid(state->left_x);
    out->bytes[3] = dinput_axis_y_to_hid(state->left_y);
    out->bytes[4] = dinput_axis_x_to_hid(state->right_x);
    out->bytes[5] = dinput_axis_y_to_hid(state->right_y);
    out->bytes[6] = state->right_trigger;
    out->bytes[7] = state->left_trigger;
    put_le16(&out->bytes[8], dinput_buttons_from_gamepad(state));
    out->bytes[10] = 0x64;
}
