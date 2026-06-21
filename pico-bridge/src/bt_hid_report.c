#include "bt_hid_report.h"

#include <stdbool.h>
#include <string.h>

#include "dinput_report.h"

static const uint8_t bt_hid_gamepad_descriptor[] = {
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x05,       // Usage (Game Pad)
    0xA1, 0x01,       // Collection (Application)
    0x85, BT_HID_REPORT_ID,
    0x05, 0x09,       // Usage Page (Button)
    0x19, 0x01,       // Usage Minimum (Button 1)
    0x29, BT_HID_BUTTON_COUNT,
    0x15, 0x00,       // Logical Minimum (0)
    0x25, 0x01,       // Logical Maximum (1)
    0x75, 0x01,       // Report Size (1)
    0x95, BT_HID_BUTTON_COUNT,
    0x81, 0x02,       // Input (Data, Variable, Absolute)
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x39,       // Usage (Hat switch)
    0x15, 0x00,       // Logical Minimum (0)
    0x25, 0x07,       // Logical Maximum (7)
    0x35, 0x00,       // Physical Minimum (0)
    0x46, 0x3B, 0x01, // Physical Maximum (315)
    0x65, 0x14,       // Unit (English Rotation, degrees)
    0x75, 0x04,       // Report Size (4)
    0x95, 0x01,       // Report Count (1)
    0x81, 0x42,       // Input (Data, Variable, Absolute, Null State)
    0x65, 0x00,       // Unit (None)
    0x09, 0x30,       // Usage (X)
    0x09, 0x31,       // Usage (Y)
    0x09, 0x33,       // Usage (Rx)
    0x09, 0x34,       // Usage (Ry)
    0x09, 0x32,       // Usage (Z)
    0x09, 0x35,       // Usage (Rz)
    0x15, 0x00,       // Logical Minimum (0)
    0x26, 0xFF, 0x00, // Logical Maximum (255)
    0x75, 0x08,       // Report Size (8)
    0x95, 0x06,       // Report Count (6)
    0x81, 0x02,       // Input (Data, Variable, Absolute)
    0xC0,             // End Collection
};

static uint8_t centered_axis_x_to_hid(int16_t value) {
    uint8_t scaled = dinput_axis_x_to_hid(value);
    return value == 0 ? 0x80 : scaled;
}

static uint8_t centered_axis_y_to_hid(int16_t value) {
    uint8_t scaled = dinput_axis_y_to_hid(value);
    return value == 0 ? 0x80 : scaled;
}

static void set_button(uint32_t *buttons, uint8_t one_based_index, bool active) {
    if (active && one_based_index >= 1 && one_based_index <= BT_HID_BUTTON_COUNT)
        *buttons |= 1u << (one_based_index - 1u);
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

static uint32_t xbox_buttons(const gamepad_state_t *state) {
    uint32_t out = 0;
    set_button(&out, 1, xbutton(state, DINPUT_XINPUT_A));
    set_button(&out, 2, xbutton(state, DINPUT_XINPUT_B));
    set_button(&out, 3, xbutton(state, DINPUT_XINPUT_X));
    set_button(&out, 4, xbutton(state, DINPUT_XINPUT_Y));
    set_button(&out, 5, xbutton(state, DINPUT_XINPUT_LEFT_SHOULDER));
    set_button(&out, 6, xbutton(state, DINPUT_XINPUT_RIGHT_SHOULDER));
    set_button(&out, 7, state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD);
    set_button(&out, 8, state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD);
    set_button(&out, 9, xbutton(state, DINPUT_XINPUT_BACK));
    set_button(&out, 10, xbutton(state, DINPUT_XINPUT_START));
    set_button(&out, 11, xbutton(state, DINPUT_XINPUT_LEFT_THUMB));
    set_button(&out, 12, xbutton(state, DINPUT_XINPUT_RIGHT_THUMB));
    set_button(&out, 13, xbutton(state, DINPUT_XINPUT_GUIDE));
    set_button(&out, 14, xbutton(state, DINPUT_XINPUT_DPAD_UP));
    set_button(&out, 15, xbutton(state, DINPUT_XINPUT_DPAD_DOWN));
    set_button(&out, 16, xbutton(state, DINPUT_XINPUT_DPAD_LEFT));
    set_button(&out, 17, xbutton(state, DINPUT_XINPUT_DPAD_RIGHT));
    return out;
}

static uint32_t playstation_buttons(const gamepad_state_t *state) {
    uint32_t out = 0;
    set_button(&out, 1, xbutton(state, DINPUT_XINPUT_X));
    set_button(&out, 2, xbutton(state, DINPUT_XINPUT_A));
    set_button(&out, 3, xbutton(state, DINPUT_XINPUT_B));
    set_button(&out, 4, xbutton(state, DINPUT_XINPUT_Y));
    set_button(&out, 5, xbutton(state, DINPUT_XINPUT_LEFT_SHOULDER));
    set_button(&out, 6, xbutton(state, DINPUT_XINPUT_RIGHT_SHOULDER));
    set_button(&out, 7, state->left_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD);
    set_button(&out, 8, state->right_trigger >= DINPUT_TRIGGER_BUTTON_THRESHOLD);
    set_button(&out, 9, xbutton(state, DINPUT_XINPUT_BACK));
    set_button(&out, 10, xbutton(state, DINPUT_XINPUT_START));
    set_button(&out, 11, xbutton(state, DINPUT_XINPUT_LEFT_THUMB));
    set_button(&out, 12, xbutton(state, DINPUT_XINPUT_RIGHT_THUMB));
    set_button(&out, 13, xbutton(state, DINPUT_XINPUT_GUIDE));
    set_button(&out, 14, xbutton(state, DINPUT_XINPUT_DPAD_UP));
    set_button(&out, 15, xbutton(state, DINPUT_XINPUT_DPAD_DOWN));
    set_button(&out, 16, xbutton(state, DINPUT_XINPUT_DPAD_LEFT));
    set_button(&out, 17, xbutton(state, DINPUT_XINPUT_DPAD_RIGHT));
    return out;
}

static uint32_t buttons_for_target(bt_hid_target_t target, const gamepad_state_t *state) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return xbox_buttons(state);
    case BT_HID_TARGET_PLAYSTATION:
        return playstation_buttons(state);
    case BT_HID_TARGET_GENERIC:
    default:
        return generic_buttons(state);
    }
}

const uint8_t *bt_hid_descriptor(bt_hid_target_t target, uint16_t *len) {
    (void)target;
    if (len)
        *len = (uint16_t)sizeof(bt_hid_gamepad_descriptor);
    return bt_hid_gamepad_descriptor;
}

const char *bt_hid_target_label(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return "bluetooth-xbox-hid";
    case BT_HID_TARGET_PLAYSTATION:
        return "bluetooth-playstation-hid";
    case BT_HID_TARGET_GENERIC:
    default:
        return "bluetooth-hid";
    }
}

const char *bt_hid_local_name(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return "CouchLink BT Xbox 00:00:00:00:00:00";
    case BT_HID_TARGET_PLAYSTATION:
        return "CouchLink BT PS 00:00:00:00:00:00";
    case BT_HID_TARGET_GENERIC:
    default:
        return "CouchLink BT HID 00:00:00:00:00:00";
    }
}

const char *bt_hid_service_name(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return "CouchLink Bluetooth Xbox HID";
    case BT_HID_TARGET_PLAYSTATION:
        return "CouchLink Bluetooth PlayStation HID";
    case BT_HID_TARGET_GENERIC:
    default:
        return "CouchLink Bluetooth HID";
    }
}

uint16_t bt_hid_product_id(bt_hid_target_t target) {
    switch (target) {
    case BT_HID_TARGET_XBOX:
        return 0xCB11u;
    case BT_HID_TARGET_PLAYSTATION:
        return 0xCB12u;
    case BT_HID_TARGET_GENERIC:
    default:
        return 0xCB10u;
    }
}

void bt_hid_build_report(bt_hid_target_t target, const gamepad_state_t *state,
                         bt_hid_report_t *out) {
    memset(out, 0, sizeof(*out));
    out->len = BT_HID_WIRE_REPORT_LEN;
    out->report_id = BT_HID_REPORT_ID;
    out->bytes[0] = BT_HID_REPORT_ID;

    uint32_t buttons = buttons_for_target(target, state);
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
