#include "bt_hid_report.h"

#include <stdbool.h>
#include <string.h>

#include "dinput_report.h"

static const uint8_t bt_hid_gamepad_descriptor[] = {
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x05, // Usage (Game Pad)
    0xA1, 0x01, // Collection (Application)
    0x85, BT_HID_GENERIC_REPORT_ID,
    0x05, 0x09, // Usage Page (Button)
    0x19, 0x01, // Usage Minimum (Button 1)
    0x29, BT_HID_BUTTON_COUNT,
    0x15, 0x00, // Logical Minimum (0)
    0x25, 0x01, // Logical Maximum (1)
    0x75, 0x01, // Report Size (1)
    0x95, BT_HID_BUTTON_COUNT,
    0x81, 0x02, // Input (Data, Variable, Absolute)
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x39, // Usage (Hat switch)
    0x15, 0x00, // Logical Minimum (0)
    0x25, 0x07, // Logical Maximum (7)
    0x35, 0x00, // Physical Minimum (0)
    0x46, 0x3B,
    0x01,       // Physical Maximum (315)
    0x65, 0x14, // Unit (English Rotation, degrees)
    0x75, 0x04, // Report Size (4)
    0x95, 0x01, // Report Count (1)
    0x81, 0x42, // Input (Data, Variable, Absolute, Null State)
    0x65, 0x00, // Unit (None)
    0x09, 0x30, // Usage (X)
    0x09, 0x31, // Usage (Y)
    0x09, 0x33, // Usage (Rx)
    0x09, 0x34, // Usage (Ry)
    0x09, 0x32, // Usage (Z)
    0x09, 0x35, // Usage (Rz)
    0x15, 0x00, // Logical Minimum (0)
    0x26, 0xFF,
    0x00,       // Logical Maximum (255)
    0x75, 0x08, // Report Size (8)
    0x95, 0x06, // Report Count (6)
    0x81, 0x02, // Input (Data, Variable, Absolute)
    0xC0,       // End Collection
};

static const uint8_t bt_hid_xbox_descriptor[] = {
    0x05, 0x01, 0x09, 0x05, 0xa1, 0x01, 0x85, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x09, 0x30, 0x09, 0x31,
    0x15, 0x00, 0x27, 0xff, 0xff, 0x00, 0x00, 0x95, 0x02, 0x75, 0x10, 0x81, 0x02, 0xc0, 0x09, 0x01,
    0xa1, 0x00, 0x09, 0x32, 0x09, 0x35, 0x15, 0x00, 0x27, 0xff, 0xff, 0x00, 0x00, 0x95, 0x02, 0x75,
    0x10, 0x81, 0x02, 0xc0, 0x05, 0x02, 0x09, 0xc5, 0x15, 0x00, 0x26, 0xff, 0x03, 0x95, 0x01, 0x75,
    0x0a, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x02, 0x09,
    0xc4, 0x15, 0x00, 0x26, 0xff, 0x03, 0x95, 0x01, 0x75, 0x0a, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x06, 0x95, 0x01, 0x81, 0x03, 0x05, 0x01, 0x09, 0x39, 0x15, 0x01, 0x25, 0x08, 0x35, 0x00,
    0x46, 0x3b, 0x01, 0x66, 0x14, 0x00, 0x75, 0x04, 0x95, 0x01, 0x81, 0x42, 0x75, 0x04, 0x95, 0x01,
    0x15, 0x00, 0x25, 0x00, 0x35, 0x00, 0x45, 0x00, 0x65, 0x00, 0x81, 0x03, 0x05, 0x09, 0x19, 0x01,
    0x29, 0x0f, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0f, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00,
    0x75, 0x01, 0x95, 0x01, 0x81, 0x03, 0x05, 0x0c, 0x0a, 0xb2, 0x00, 0x15, 0x00, 0x25, 0x01, 0x95,
    0x01, 0x75, 0x01, 0x81, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x07, 0x95, 0x01, 0x81, 0x03, 0x05,
    0x0f, 0x09, 0x21, 0x85, 0x03, 0xa1, 0x02, 0x09, 0x97, 0x15, 0x00, 0x25, 0x01, 0x75, 0x04, 0x95,
    0x01, 0x91, 0x02, 0x15, 0x00, 0x25, 0x00, 0x75, 0x04, 0x95, 0x01, 0x91, 0x03, 0x09, 0x70, 0x15,
    0x00, 0x25, 0x64, 0x75, 0x08, 0x95, 0x04, 0x91, 0x02, 0x09, 0x50, 0x66, 0x01, 0x10, 0x55, 0x0e,
    0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x09, 0xa7, 0x15, 0x00, 0x26,
    0xff, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x65, 0x00, 0x55, 0x00, 0x09, 0x7c, 0x15, 0x00,
    0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0xc0, 0xc0,
};

static const uint8_t bt_hid_ds4_descriptor[] = {
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, 0x75, 0x08, 0x95, 0x0A, 0x81, 0x02, 0x06, 0x04,
    0xFF, 0x85, 0x02, 0x09, 0x24, 0x95, 0x24, 0xB1, 0x02, 0x85, 0xA3, 0x09, 0x25, 0x95, 0x30, 0xB1,
    0x02, 0x85, 0x05, 0x09, 0x26, 0x95, 0x28, 0xB1, 0x02, 0x85, 0x06, 0x09, 0x27, 0x95, 0x34, 0xB1,
    0x02, 0x85, 0x07, 0x09, 0x28, 0x95, 0x30, 0xB1, 0x02, 0x85, 0x08, 0x09, 0x29, 0x95, 0x2F, 0xB1,
    0x02, 0x06, 0x03, 0xFF, 0x85, 0x03, 0x09, 0x21, 0x95, 0x26, 0xB1, 0x02, 0x85, 0x04, 0x09, 0x22,
    0x95, 0x2E, 0xB1, 0x02, 0x85, 0xF0, 0x09, 0x47, 0x95, 0x3F, 0xB1, 0x02, 0x85, 0xF1, 0x09, 0x48,
    0x95, 0x3F, 0xB1, 0x02, 0x85, 0xF2, 0x09, 0x49, 0x95, 0x0F, 0xB1, 0x02, 0x85, 0x11, 0x06, 0x00,
    0xFF, 0x09, 0x20, 0x95, 0x02, 0x81, 0x02, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09,
    0x35, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, 0x09, 0x39, 0x15, 0x00,
    0x25, 0x07, 0x75, 0x04, 0x95, 0x01, 0x81, 0x42, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0E, 0x15, 0x00,
    0x25, 0x01, 0x75, 0x01, 0x95, 0x0E, 0x81, 0x02, 0x75, 0x06, 0x95, 0x01, 0x81, 0x01, 0x05, 0x01,
    0x09, 0x33, 0x09, 0x34, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0x06,
    0x00, 0xFF, 0x09, 0x20, 0x95, 0x03, 0x81, 0x02, 0x05, 0x01, 0x19, 0x40, 0x29, 0x42, 0x16, 0x00,
    0x80, 0x26, 0x00, 0x7F, 0x75, 0x10, 0x95, 0x03, 0x81, 0x02, 0x19, 0x43, 0x29, 0x45, 0x16, 0x00,
    0xE0, 0x26, 0xFF, 0x1F, 0x95, 0x03, 0x81, 0x02, 0x06, 0x00, 0xFF, 0x09, 0x20, 0x15, 0x00, 0x26,
    0xFF, 0x00, 0x75, 0x08, 0x95, 0x31, 0x81, 0x02, 0x09, 0x21, 0x75, 0x08, 0x95, 0x4D, 0x91, 0x02,
    0x85, 0x12, 0x09, 0x22, 0x95, 0x8D, 0x81, 0x02, 0x09, 0x23, 0x91, 0x02, 0x85, 0x13, 0x09, 0x24,
    0x95, 0xCD, 0x81, 0x02, 0x09, 0x25, 0x91, 0x02, 0x85, 0x14, 0x09, 0x26, 0x96, 0x0D, 0x01, 0x81,
    0x02, 0x09, 0x27, 0x91, 0x02, 0x85, 0x15, 0x09, 0x28, 0x96, 0x4D, 0x01, 0x81, 0x02, 0x09, 0x29,
    0x91, 0x02, 0x85, 0x16, 0x09, 0x2A, 0x96, 0x8D, 0x01, 0x81, 0x02, 0x09, 0x2B, 0x91, 0x02, 0x85,
    0x17, 0x09, 0x2C, 0x96, 0xCD, 0x01, 0x81, 0x02, 0x09, 0x2D, 0x91, 0x02, 0x85, 0x18, 0x09, 0x2E,
    0x96, 0x0D, 0x02, 0x81, 0x02, 0x09, 0x2F, 0x91, 0x02, 0x85, 0x19, 0x09, 0x30, 0x96, 0x22, 0x02,
    0x81, 0x02, 0x09, 0x31, 0x91, 0x02, 0x06, 0x80, 0xFF, 0x85, 0x82, 0x09, 0x22, 0x95, 0x3F, 0xB1,
    0x02, 0x85, 0x83, 0x09, 0x23, 0xB1, 0x02, 0x85, 0x84, 0x09, 0x24, 0xB1, 0x02, 0x85, 0x90, 0x09,
    0x30, 0xB1, 0x02, 0x85, 0x91, 0x09, 0x31, 0xB1, 0x02, 0x85, 0x92, 0x09, 0x32, 0xB1, 0x02, 0x85,
    0x93, 0x09, 0x33, 0xB1, 0x02, 0x85, 0xA0, 0x09, 0x40, 0xB1, 0x02, 0x85, 0xA4, 0x09, 0x44, 0xB1,
    0x02, 0xC0,
};

static const uint8_t bt_hid_ds4_feature_02[] = {
    0xfe, 0xff, 0x0e, 0x00, 0x04, 0x00, 0xd4, 0x22, 0x2a, 0xdd, 0xbb, 0x22,
    0x5e, 0xdd, 0x81, 0x22, 0x84, 0xdd, 0x1c, 0x02, 0x1c, 0x02, 0x85, 0x1f,
    0xb0, 0xe0, 0xc6, 0x20, 0xb5, 0xe0, 0xb1, 0x20, 0x83, 0xdf, 0x0c, 0x00,
};

static const uint8_t bt_hid_ds4_feature_a3[] = {
    0x4a, 0x75, 0x6e, 0x20, 0x20, 0x39, 0x20, 0x32, 0x30, 0x31, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x31, 0x32, 0x3a, 0x33, 0x36, 0x3a, 0x34, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x08, 0xb4, 0x01, 0x00, 0x00, 0x00, 0x07, 0xa0, 0x10, 0x20, 0x00, 0xa0, 0x02, 0x00,
};

static const uint8_t bt_hid_ds4_feature_f2[] = {
    0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x55, 0xf8, 0x2a,
};

static uint16_t copy_exact(uint8_t *buffer, uint16_t reqlen, const uint8_t *src, uint16_t len) {
    if (!buffer || !src || reqlen < len)
        return 0;
    memcpy(buffer, src, len);
    return len;
}

static uint16_t copy_input_payload(bt_hid_target_t target, const gamepad_state_t *state,
                                   uint8_t report_id, uint8_t *buffer, uint16_t reqlen) {
    bt_hid_report_t report;
    gamepad_state_t neutral = {0};
    bt_hid_build_report(target, state ? state : &neutral, &report);
    if (report.report_id != report_id || report.len == 0)
        return 0;
    return copy_exact(buffer, reqlen, &report.bytes[1], (uint16_t)(report.len - 1u));
}

static uint16_t get_xbox_input_payload(uint8_t report_id, const gamepad_state_t *state,
                                       uint8_t *buffer, uint16_t reqlen) {
    if (report_id == BT_HID_XBOX_REPORT_ID)
        return copy_input_payload(BT_HID_TARGET_XBOX, state, report_id, buffer, reqlen);
    return 0;
}

static uint16_t get_ds4_feature_payload(uint8_t report_id, uint8_t *buffer, uint16_t reqlen) {
    switch (report_id) {
    case BT_HID_PS4_FEATURE_PAIRING_REPORT_ID:
        return copy_exact(buffer, reqlen, bt_hid_ds4_feature_02,
                          (uint16_t)sizeof(bt_hid_ds4_feature_02));
    case BT_HID_PS4_FEATURE_BUILD_DATE_REPORT_ID:
        return copy_exact(buffer, reqlen, bt_hid_ds4_feature_a3,
                          (uint16_t)sizeof(bt_hid_ds4_feature_a3));
    case BT_HID_PS4_FEATURE_AUTH_STATUS_REPORT_ID:
        return copy_exact(buffer, reqlen, bt_hid_ds4_feature_f2,
                          (uint16_t)sizeof(bt_hid_ds4_feature_f2));
    default:
        return 0;
    }
}

static void put_le16(uint8_t *dst, uint16_t value) {
    dst[0] = (uint8_t)(value & 0xFFu);
    dst[1] = (uint8_t)((value >> 8) & 0xFFu);
}

static void put_le24(uint8_t *dst, uint32_t value) {
    dst[0] = (uint8_t)(value & 0xFFu);
    dst[1] = (uint8_t)((value >> 8) & 0xFFu);
    dst[2] = (uint8_t)((value >> 16) & 0xFFu);
}

static void put_le32(uint8_t *dst, uint32_t value) {
    dst[0] = (uint8_t)(value & 0xFFu);
    dst[1] = (uint8_t)((value >> 8) & 0xFFu);
    dst[2] = (uint8_t)((value >> 16) & 0xFFu);
    dst[3] = (uint8_t)((value >> 24) & 0xFFu);
}

static uint32_t crc32_le_update(uint32_t crc, const uint8_t *data, uint8_t n) {
    for (uint8_t i = 0; i < n; i++) {
        crc ^= data[i];
        for (uint8_t bit = 0; bit < 8; bit++) {
            uint32_t mask = 0u - (crc & 1u);
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return crc;
}

static uint8_t centered_axis_x_to_hid(int16_t value) {
    uint8_t scaled = dinput_axis_x_to_hid(value);
    return value == 0 ? 0x80 : scaled;
}

static uint8_t centered_axis_y_to_hid(int16_t value) {
    uint8_t scaled = dinput_axis_y_to_hid(value);
    return value == 0 ? 0x80 : scaled;
}

static uint16_t centered_axis_x_to_hid16(int16_t value) {
    return (uint16_t)((int32_t)value + 32768);
}

static uint16_t centered_axis_y_to_hid16(int16_t value) {
    if (value == 0)
        return 0x8000u;
    return (uint16_t)((int32_t)32767 - value);
}

static uint16_t trigger_to_hid10(uint8_t value) {
    return (uint16_t)(((uint32_t)value * 1023u + 127u) / 255u);
}

static void set_button(uint32_t *buttons, uint8_t one_based_index, bool active) {
    if (active && one_based_index >= 1 && one_based_index <= BT_HID_BUTTON_COUNT)
        *buttons |= 1u << (one_based_index - 1u);
}

static void set_button_bit(uint32_t *buttons, uint8_t zero_based_index, bool active) {
    if (active && zero_based_index < 24)
        *buttons |= 1u << zero_based_index;
}

static bool xbutton(const gamepad_state_t *state, uint16_t mask) {
    return (state->buttons & mask) != 0;
}

static uint32_t generic_buttons(const gamepad_state_t *state) {
    uint32_t out = 0;
    set_button(&out, 1, xbutton(state, DINPUT_XINPUT_A));
    set_button(&out, 2, xbutton(state, DINPUT_XINPUT_B));
    set_button(&out, 3, xbutton(state, DINPUT_XINPUT_X));
    set_button(&out, 4, xbutton(state, DINPUT_XINPUT_Y));
    set_button(&out, 5, xbutton(state, DINPUT_XINPUT_LEFT_SHOULDER));
    set_button(&out, 6, xbutton(state, DINPUT_XINPUT_RIGHT_SHOULDER));
    set_button(&out, 7, xbutton(state, DINPUT_XINPUT_BACK));
    set_button(&out, 8, xbutton(state, DINPUT_XINPUT_START));
    set_button(&out, 9, xbutton(state, DINPUT_XINPUT_LEFT_THUMB));
    set_button(&out, 10, xbutton(state, DINPUT_XINPUT_RIGHT_THUMB));
    set_button(&out, 11, state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD);
    set_button(&out, 12, state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD);
    set_button(&out, 13, xbutton(state, DINPUT_XINPUT_GUIDE));
    set_button(&out, 14, xbutton(state, DINPUT_XINPUT_DPAD_UP));
    set_button(&out, 15, xbutton(state, DINPUT_XINPUT_DPAD_DOWN));
    set_button(&out, 16, xbutton(state, DINPUT_XINPUT_DPAD_LEFT));
    set_button(&out, 17, xbutton(state, DINPUT_XINPUT_DPAD_RIGHT));
    return out;
}

static uint8_t xbox_hat_from_dinput(uint8_t dinput_hat) {
    return dinput_hat == DINPUT_HAT_NEUTRAL ? 0u : (uint8_t)(dinput_hat + 1u);
}

static uint32_t xbox_buttons(const gamepad_state_t *state) {
    uint32_t out = 0;
    set_button_bit(&out, 0, xbutton(state, DINPUT_XINPUT_A));
    set_button_bit(&out, 1, xbutton(state, DINPUT_XINPUT_B));
    set_button_bit(&out, 3, xbutton(state, DINPUT_XINPUT_X));
    set_button_bit(&out, 4, xbutton(state, DINPUT_XINPUT_Y));
    set_button_bit(&out, 6, xbutton(state, DINPUT_XINPUT_LEFT_SHOULDER));
    set_button_bit(&out, 7, xbutton(state, DINPUT_XINPUT_RIGHT_SHOULDER));
    set_button_bit(&out, 10, xbutton(state, DINPUT_XINPUT_BACK));
    set_button_bit(&out, 11, xbutton(state, DINPUT_XINPUT_START));
    set_button_bit(&out, 12, xbutton(state, DINPUT_XINPUT_GUIDE));
    set_button_bit(&out, 13, xbutton(state, DINPUT_XINPUT_LEFT_THUMB));
    set_button_bit(&out, 14, xbutton(state, DINPUT_XINPUT_RIGHT_THUMB));
    return out;
}

static void build_generic_report(const gamepad_state_t *state, bt_hid_report_t *out) {
    out->len = BT_HID_GENERIC_WIRE_REPORT_LEN;
    out->report_id = BT_HID_GENERIC_REPORT_ID;
    out->bytes[0] = BT_HID_GENERIC_REPORT_ID;

    uint32_t buttons = generic_buttons(state);
    uint8_t hat = dinput_hat_from_buttons(state->buttons);

    out->bytes[1] = (uint8_t)(buttons & 0xFFu);
    out->bytes[2] = (uint8_t)((buttons >> 8) & 0xFFu);
    out->bytes[3] = (uint8_t)(((buttons >> 16) & 0x0Fu) | ((hat & 0x0Fu) << 4));
    out->bytes[4] = centered_axis_x_to_hid(state->left_x);
    out->bytes[5] = centered_axis_y_to_hid(state->left_y);
    out->bytes[6] = centered_axis_x_to_hid(state->right_x);
    out->bytes[7] = centered_axis_y_to_hid(state->right_y);
    out->bytes[8] = state->left_trigger;
    out->bytes[9] = state->right_trigger;
}

static void build_xbox_report(const gamepad_state_t *state, bt_hid_report_t *out) {
    out->len = BT_HID_XBOX_WIRE_REPORT_LEN;
    out->report_id = BT_HID_XBOX_REPORT_ID;
    out->bytes[0] = BT_HID_XBOX_REPORT_ID;

    put_le16(&out->bytes[1], centered_axis_x_to_hid16(state->left_x));
    put_le16(&out->bytes[3], centered_axis_y_to_hid16(state->left_y));
    put_le16(&out->bytes[5], centered_axis_x_to_hid16(state->right_x));
    put_le16(&out->bytes[7], centered_axis_y_to_hid16(state->right_y));
    put_le16(&out->bytes[9], trigger_to_hid10(state->left_trigger) & 0x03FFu);
    put_le16(&out->bytes[11], trigger_to_hid10(state->right_trigger) & 0x03FFu);
    out->bytes[13] = xbox_hat_from_dinput(dinput_hat_from_buttons(state->buttons)) & 0x0Fu;
    put_le24(&out->bytes[14], xbox_buttons(state));
}

static void finish_ds4_crc(bt_hid_report_t *out) {
    uint8_t seed = 0xA1u;
    uint32_t crc = crc32_le_update(0xFFFFFFFFu, &seed, 1u);
    crc = ~crc32_le_update(crc, out->bytes, BT_HID_PS4_WIRE_REPORT_LEN - 4u);
    put_le32(&out->bytes[BT_HID_PS4_WIRE_REPORT_LEN - 4u], crc);
}

static void build_ds4_report(const gamepad_state_t *state, bt_hid_report_t *out) {
    dinput_report_t usb_report;
    dinput_build_ps4_report(state, 0, &usb_report);

    out->len = BT_HID_PS4_WIRE_REPORT_LEN;
    out->report_id = BT_HID_PS4_REPORT_ID;
    out->bytes[0] = BT_HID_PS4_REPORT_ID;
    out->bytes[1] = 0xC0u;
    out->bytes[2] = 0x00u;
    out->bytes[3] = usb_report.bytes[1];
    out->bytes[4] = usb_report.bytes[2];
    out->bytes[5] = usb_report.bytes[3];
    out->bytes[6] = usb_report.bytes[4];
    out->bytes[7] = usb_report.bytes[5];
    out->bytes[8] = usb_report.bytes[6];
    out->bytes[9] = usb_report.bytes[7];
    out->bytes[10] = usb_report.bytes[8];
    out->bytes[11] = usb_report.bytes[9];
    out->bytes[12] = usb_report.bytes[10];
    out->bytes[13] = usb_report.bytes[11];
    out->bytes[32] = 0x09u;
    out->bytes[35] = 0x01u;
    finish_ds4_crc(out);
}

const uint8_t *bt_hid_descriptor(bt_hid_target_t target, uint16_t *len) {
    const uint8_t *descriptor = bt_hid_gamepad_descriptor;
    uint16_t descriptor_len = (uint16_t)sizeof(bt_hid_gamepad_descriptor);
    switch (target) {
    case BT_HID_TARGET_XBOX:
        descriptor = bt_hid_xbox_descriptor;
        descriptor_len = (uint16_t)sizeof(bt_hid_xbox_descriptor);
        break;
    case BT_HID_TARGET_PLAYSTATION:
        descriptor = bt_hid_ds4_descriptor;
        descriptor_len = (uint16_t)sizeof(bt_hid_ds4_descriptor);
        break;
    case BT_HID_TARGET_GENERIC:
    default:
        break;
    }
    if (len)
        *len = descriptor_len;
    return descriptor;
}

const char *bt_hid_target_label(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return "bluetooth-xbox";
    case BT_HID_TARGET_PLAYSTATION:
        return "bluetooth-playstation";
    case BT_HID_TARGET_GENERIC:
    default:
        return "bluetooth";
    }
}

const char *bt_hid_local_name(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return "Xbox Wireless Controller";
    case BT_HID_TARGET_PLAYSTATION:
        return "Wireless Controller";
    case BT_HID_TARGET_GENERIC:
    default:
        return "CouchLink BT HID 00:00:00:00:00:00";
    }
}

const char *bt_hid_service_name(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return "Xbox Wireless Controller";
    case BT_HID_TARGET_PLAYSTATION:
        return "Wireless Controller";
    case BT_HID_TARGET_GENERIC:
    default:
        return "CouchLink Bluetooth HID";
    }
}

uint16_t bt_hid_vendor_id(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return 0x045Eu;
    case BT_HID_TARGET_PLAYSTATION:
        return 0x054Cu;
    case BT_HID_TARGET_GENERIC:
    default:
        return 0x2E8Au;
    }
}

uint16_t bt_hid_product_id(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return 0x02FDu;
    case BT_HID_TARGET_PLAYSTATION:
        return 0x05C4u;
    case BT_HID_TARGET_GENERIC:
    default:
        return 0xCB10u;
    }
}

uint16_t bt_hid_bcd_version(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return 0x0903u;
    case BT_HID_TARGET_PLAYSTATION:
        return 0x0100u;
    case BT_HID_TARGET_GENERIC:
    default:
        return 0x0100u;
    }
}

void bt_hid_build_report(bt_hid_target_t target, const gamepad_state_t *state,
                         bt_hid_report_t *out) {
    memset(out, 0, sizeof(*out));
    switch (target) {
    case BT_HID_TARGET_XBOX:
        build_xbox_report(state, out);
        break;
    case BT_HID_TARGET_PLAYSTATION:
        build_ds4_report(state, out);
        break;
    case BT_HID_TARGET_GENERIC:
    default:
        build_generic_report(state, out);
        break;
    }
}

uint16_t bt_hid_get_report_payload(bt_hid_target_t target, uint8_t report_type, uint8_t report_id,
                                   const gamepad_state_t *state, uint8_t *buffer, uint16_t reqlen) {
    if (report_type == BT_HID_REPORT_TYPE_INPUT) {
        switch (target) {
        case BT_HID_TARGET_XBOX:
            return get_xbox_input_payload(report_id, state, buffer, reqlen);
        case BT_HID_TARGET_PLAYSTATION:
            return copy_input_payload(target, state, report_id, buffer, reqlen);
        case BT_HID_TARGET_GENERIC:
        default:
            return copy_input_payload(BT_HID_TARGET_GENERIC, state, report_id, buffer, reqlen);
        }
    }

    if (report_type != BT_HID_REPORT_TYPE_FEATURE)
        return 0;
    if (target != BT_HID_TARGET_PLAYSTATION)
        return 0;
    return get_ds4_feature_payload(report_id, buffer, reqlen);
}

bool bt_hid_accept_set_report(bt_hid_target_t target, uint8_t report_type, const uint8_t *report,
                              uint16_t report_size, uint8_t *report_id, uint16_t *payload_len) {
    if (!report || report_size == 0)
        return false;

    uint8_t id = report[0];
    uint16_t len = (uint16_t)(report_size - 1u);
    if (report_id)
        *report_id = id;
    if (payload_len)
        *payload_len = len;

    if (report_type != BT_HID_REPORT_TYPE_OUTPUT)
        return false;
    if (target == BT_HID_TARGET_XBOX)
        return id == BT_HID_XBOX_OUTPUT_REPORT_ID && len == BT_HID_XBOX_OUTPUT_PAYLOAD_REPORT_LEN;
    if (target == BT_HID_TARGET_PLAYSTATION)
        return id == BT_HID_PS4_REPORT_ID && len == BT_HID_PS4_PAYLOAD_REPORT_LEN;
    return false;
}

bool bt_hid_accept_output_payload(bt_hid_target_t target, uint8_t report_type, uint8_t report_id,
                                  const uint8_t *payload, uint16_t payload_len) {
    (void)payload;
    if (payload_len > 0 && !payload)
        return false;
    if (report_type != BT_HID_REPORT_TYPE_OUTPUT)
        return false;
    if (target == BT_HID_TARGET_XBOX)
        return report_id == BT_HID_XBOX_OUTPUT_REPORT_ID &&
               payload_len == BT_HID_XBOX_OUTPUT_PAYLOAD_REPORT_LEN;
    if (target == BT_HID_TARGET_PLAYSTATION)
        return report_id == BT_HID_PS4_REPORT_ID && payload_len == BT_HID_PS4_PAYLOAD_REPORT_LEN;
    return false;
}
