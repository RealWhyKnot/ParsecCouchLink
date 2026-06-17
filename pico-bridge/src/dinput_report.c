#include "dinput_report.h"

#include <stdbool.h>
#include <string.h>

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

static uint8_t centered_axis_x_to_hid(int16_t value) {
    uint8_t scaled = dinput_axis_x_to_hid(value);
    return value == 0 ? 0x80 : scaled;
}

static uint8_t centered_axis_y_to_hid(int16_t value) {
    uint8_t scaled = dinput_axis_y_to_hid(value);
    return value == 0 ? 0x80 : scaled;
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

void dinput_build_ps3_report(const gamepad_state_t *state, dinput_report_t *out) {
    memset(out, 0, sizeof(*out));
    out->len = DINPUT_PS3_WIRE_REPORT_LEN;
    out->report_id = DINPUT_PS3_REPORT_ID;
    out->bytes[0] = DINPUT_PS3_REPORT_ID;

    uint16_t buttons = state->buttons;
    if (buttons & DINPUT_XINPUT_BACK)
        out->bytes[2] |= 1u << 0; // Select
    if (buttons & DINPUT_XINPUT_LEFT_THUMB)
        out->bytes[2] |= 1u << 1;
    if (buttons & DINPUT_XINPUT_RIGHT_THUMB)
        out->bytes[2] |= 1u << 2;
    if (buttons & DINPUT_XINPUT_START)
        out->bytes[2] |= 1u << 3;
    if (buttons & DINPUT_XINPUT_DPAD_UP)
        out->bytes[2] |= 1u << 4;
    if (buttons & DINPUT_XINPUT_DPAD_RIGHT)
        out->bytes[2] |= 1u << 5;
    if (buttons & DINPUT_XINPUT_DPAD_DOWN)
        out->bytes[2] |= 1u << 6;
    if (buttons & DINPUT_XINPUT_DPAD_LEFT)
        out->bytes[2] |= 1u << 7;

    if (state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out->bytes[3] |= 1u << 0; // L2
    if (state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out->bytes[3] |= 1u << 1; // R2
    if (buttons & DINPUT_XINPUT_LEFT_SHOULDER)
        out->bytes[3] |= 1u << 2; // L1
    if (buttons & DINPUT_XINPUT_RIGHT_SHOULDER)
        out->bytes[3] |= 1u << 3; // R1
    if (buttons & DINPUT_XINPUT_Y)
        out->bytes[3] |= 1u << 4; // Triangle
    if (buttons & DINPUT_XINPUT_B)
        out->bytes[3] |= 1u << 5; // Circle
    if (buttons & DINPUT_XINPUT_A)
        out->bytes[3] |= 1u << 6; // Cross
    if (buttons & DINPUT_XINPUT_X)
        out->bytes[3] |= 1u << 7; // Square
    if (buttons & DINPUT_XINPUT_GUIDE)
        out->bytes[4] |= 1u << 0; // PS

    out->bytes[6] = centered_axis_x_to_hid(state->left_x);
    out->bytes[7] = centered_axis_y_to_hid(state->left_y);
    out->bytes[8] = centered_axis_x_to_hid(state->right_x);
    out->bytes[9] = centered_axis_y_to_hid(state->right_y);
    out->bytes[14] = (buttons & DINPUT_XINPUT_DPAD_UP) ? 0xFF : 0x00;
    out->bytes[15] = (buttons & DINPUT_XINPUT_DPAD_RIGHT) ? 0xFF : 0x00;
    out->bytes[16] = (buttons & DINPUT_XINPUT_DPAD_DOWN) ? 0xFF : 0x00;
    out->bytes[17] = (buttons & DINPUT_XINPUT_DPAD_LEFT) ? 0xFF : 0x00;
    out->bytes[18] = state->left_trigger;
    out->bytes[19] = state->right_trigger;
    out->bytes[20] = (buttons & DINPUT_XINPUT_LEFT_SHOULDER) ? 0xFF : 0x00;
    out->bytes[21] = (buttons & DINPUT_XINPUT_RIGHT_SHOULDER) ? 0xFF : 0x00;
    out->bytes[22] = (buttons & DINPUT_XINPUT_Y) ? 0xFF : 0x00;
    out->bytes[23] = (buttons & DINPUT_XINPUT_B) ? 0xFF : 0x00;
    out->bytes[24] = (buttons & DINPUT_XINPUT_A) ? 0xFF : 0x00;
    out->bytes[25] = (buttons & DINPUT_XINPUT_X) ? 0xFF : 0x00;
    out->bytes[29] = 0x02; // plugged in over USB
    out->bytes[30] = 0x05; // battery full
    out->bytes[31] = 0x10; // wired rumble-capable state
    for (int i = 41; i <= 47; i += 2) {
        out->bytes[i] = 0x01;
        out->bytes[i + 1] = 0xFF;
    }
}

void dinput_build_ps4_report(const gamepad_state_t *state, uint8_t report_counter,
                             dinput_report_t *out) {
    memset(out, 0, sizeof(*out));
    out->len = DINPUT_PS4_WIRE_REPORT_LEN;
    out->report_id = DINPUT_PS4_REPORT_ID;
    out->bytes[0] = DINPUT_PS4_REPORT_ID;
    out->bytes[1] = centered_axis_x_to_hid(state->left_x);
    out->bytes[2] = centered_axis_y_to_hid(state->left_y);
    out->bytes[3] = centered_axis_x_to_hid(state->right_x);
    out->bytes[4] = centered_axis_y_to_hid(state->right_y);

    uint16_t buttons = state->buttons;
    out->bytes[5] = dinput_hat_from_buttons(buttons);
    if (out->bytes[5] == DINPUT_HAT_NEUTRAL)
        out->bytes[5] = 0x08;
    if (buttons & DINPUT_XINPUT_X)
        out->bytes[5] |= 1u << 4; // Square
    if (buttons & DINPUT_XINPUT_A)
        out->bytes[5] |= 1u << 5; // Cross
    if (buttons & DINPUT_XINPUT_B)
        out->bytes[5] |= 1u << 6; // Circle
    if (buttons & DINPUT_XINPUT_Y)
        out->bytes[5] |= 1u << 7; // Triangle

    if (buttons & DINPUT_XINPUT_LEFT_SHOULDER)
        out->bytes[6] |= 1u << 0;
    if (buttons & DINPUT_XINPUT_RIGHT_SHOULDER)
        out->bytes[6] |= 1u << 1;
    if (state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out->bytes[6] |= 1u << 2;
    if (state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        out->bytes[6] |= 1u << 3;
    if (buttons & DINPUT_XINPUT_BACK)
        out->bytes[6] |= 1u << 4; // Share
    if (buttons & DINPUT_XINPUT_START)
        out->bytes[6] |= 1u << 5; // Options
    if (buttons & DINPUT_XINPUT_LEFT_THUMB)
        out->bytes[6] |= 1u << 6;
    if (buttons & DINPUT_XINPUT_RIGHT_THUMB)
        out->bytes[6] |= 1u << 7;

    if (buttons & DINPUT_XINPUT_GUIDE)
        out->bytes[7] |= 1u << 0; // PS button
    out->bytes[7] |= (uint8_t)((report_counter & 0x3Fu) << 2);
    out->bytes[8] = state->left_trigger;
    out->bytes[9] = state->right_trigger;

    uint16_t axis_timing = ((uint16_t)report_counter) * 4u;
    put_le16(&out->bytes[10], axis_timing);
    put_le16(&out->bytes[12], 0x0000); // battery / timing baseline
    out->bytes[30] = 0x1B;             // full battery, charging over USB
    out->bytes[33] = 0x01;             // no extension data
}

void dinput_build_generic_hid_report(const gamepad_state_t *state, dinput_report_t *out) {
    memset(out, 0, sizeof(*out));
    out->len = DINPUT_GENERIC_HID_WIRE_REPORT_LEN;
    out->report_id = DINPUT_GENERIC_HID_REPORT_ID;

    uint16_t buttons = state->buttons;
    uint16_t generic = 0;
    if (buttons & DINPUT_XINPUT_X)
        generic |= 1u << 0;
    if (buttons & DINPUT_XINPUT_A)
        generic |= 1u << 1;
    if (buttons & DINPUT_XINPUT_B)
        generic |= 1u << 2;
    if (buttons & DINPUT_XINPUT_Y)
        generic |= 1u << 3;
    if (buttons & DINPUT_XINPUT_LEFT_SHOULDER)
        generic |= 1u << 4;
    if (buttons & DINPUT_XINPUT_RIGHT_SHOULDER)
        generic |= 1u << 5;
    if (state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        generic |= 1u << 6;
    if (state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD)
        generic |= 1u << 7;
    if (buttons & DINPUT_XINPUT_BACK)
        generic |= 1u << 8;
    if (buttons & DINPUT_XINPUT_START)
        generic |= 1u << 9;
    if (buttons & DINPUT_XINPUT_LEFT_THUMB)
        generic |= 1u << 10;
    if (buttons & DINPUT_XINPUT_RIGHT_THUMB)
        generic |= 1u << 11;

    put_le16(&out->bytes[0], generic);
    out->bytes[2] = centered_axis_x_to_hid(state->left_x);
    out->bytes[3] = centered_axis_y_to_hid(state->left_y);
    out->bytes[4] = centered_axis_x_to_hid(state->right_x);
    out->bytes[5] = centered_axis_y_to_hid(state->right_y);
    out->bytes[6] = state->left_trigger;
    out->bytes[7] = state->right_trigger;
}
