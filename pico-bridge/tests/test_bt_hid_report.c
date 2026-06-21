#include <assert.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#include "bt_hid_report.h"
#include "dinput_report.h"

#define CHECK(expr) assert(expr)

static uint32_t report_buttons(const bt_hid_report_t *report) {
    return (uint32_t)report->bytes[1] | ((uint32_t)report->bytes[2] << 8) |
           (((uint32_t)report->bytes[3] & 0x0Fu) << 16);
}

static uint8_t report_hat(const bt_hid_report_t *report) {
    return (uint8_t)(report->bytes[3] >> 4);
}

static void descriptor_has_expected_shape(void) {
    uint16_t len = 0;
    const uint8_t *descriptor = bt_hid_descriptor(BT_HID_TARGET_GENERIC, &len);

    CHECK(descriptor != NULL);
    CHECK(len > 40);
    CHECK(descriptor[0] == 0x05);
    CHECK(descriptor[1] == 0x01);
    CHECK(descriptor[6] == 0x85);
    CHECK(descriptor[7] == BT_HID_REPORT_ID);

    bool saw_buttons = false;
    bool saw_hat = false;
    bool saw_axes = false;
    for (uint16_t i = 0; i + 1 < len; i++) {
        if (descriptor[i] == 0x29 && descriptor[i + 1] == BT_HID_BUTTON_COUNT)
            saw_buttons = true;
        if (descriptor[i] == 0x09 && descriptor[i + 1] == 0x39)
            saw_hat = true;
        if (descriptor[i] == 0x95 && descriptor[i + 1] == 0x06)
            saw_axes = true;
    }
    CHECK(saw_buttons);
    CHECK(saw_hat);
    CHECK(saw_axes);
}

static void neutral_report_is_centered_and_released(void) {
    gamepad_state_t state = {0};
    bt_hid_report_t report;
    bt_hid_build_report(BT_HID_TARGET_GENERIC, &state, &report);

    CHECK(report.len == BT_HID_WIRE_REPORT_LEN);
    CHECK(report.report_id == BT_HID_REPORT_ID);
    CHECK(report.bytes[0] == BT_HID_REPORT_ID);
    CHECK(report_buttons(&report) == 0);
    CHECK(report_hat(&report) == DINPUT_HAT_NEUTRAL);
    CHECK(report.bytes[4] == 0x80);
    CHECK(report.bytes[5] == 0x80);
    CHECK(report.bytes[6] == 0x80);
    CHECK(report.bytes[7] == 0x80);
    CHECK(report.bytes[8] == 0);
    CHECK(report.bytes[9] == 0);
}

static void generic_report_maps_core_controls(void) {
    gamepad_state_t state = {0};
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_BACK |
                    DINPUT_XINPUT_START | DINPUT_XINPUT_DPAD_UP |
                    DINPUT_XINPUT_DPAD_RIGHT;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;
    state.right_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD - 1;
    state.left_x = -32768;
    state.left_y = 32767;
    state.right_x = 32767;
    state.right_y = -32768;

    bt_hid_report_t report;
    bt_hid_build_report(BT_HID_TARGET_GENERIC, &state, &report);

    uint32_t buttons = report_buttons(&report);
    CHECK((buttons & (1u << 0)) != 0);  // A
    CHECK((buttons & (1u << 1)) != 0);  // B
    CHECK((buttons & (1u << 6)) != 0);  // Back
    CHECK((buttons & (1u << 7)) != 0);  // Start
    CHECK((buttons & (1u << 10)) != 0); // LT digital
    CHECK((buttons & (1u << 11)) == 0); // RT below threshold
    CHECK((buttons & (1u << 13)) != 0); // D-pad up as button
    CHECK((buttons & (1u << 16)) != 0); // D-pad right as button
    CHECK(report_hat(&report) == DINPUT_HAT_UP_RIGHT);
    CHECK(report.bytes[4] == 0x00);
    CHECK(report.bytes[5] == 0x00);
    CHECK(report.bytes[6] == 0xFF);
    CHECK(report.bytes[7] == 0xFF);
    CHECK(report.bytes[8] == DINPUT_TRIGGER_BUTTON_THRESHOLD);
    CHECK(report.bytes[9] == DINPUT_TRIGGER_BUTTON_THRESHOLD - 1);
}

static void target_button_orders_are_distinct(void) {
    gamepad_state_t state = {0};
    state.buttons = DINPUT_XINPUT_X | DINPUT_XINPUT_A;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;

    bt_hid_report_t generic;
    bt_hid_report_t xbox;
    bt_hid_report_t playstation;
    bt_hid_build_report(BT_HID_TARGET_GENERIC, &state, &generic);
    bt_hid_build_report(BT_HID_TARGET_XBOX, &state, &xbox);
    bt_hid_build_report(BT_HID_TARGET_PLAYSTATION, &state, &playstation);

    CHECK(report_buttons(&generic) != report_buttons(&xbox));
    CHECK(report_buttons(&generic) != report_buttons(&playstation));
    CHECK(report_buttons(&xbox) != report_buttons(&playstation));

    CHECK((report_buttons(&generic) & (1u << 0)) != 0);     // A on button 1
    CHECK((report_buttons(&generic) & (1u << 2)) != 0);     // X on button 3
    CHECK((report_buttons(&xbox) & (1u << 0)) != 0);        // A on button 1
    CHECK((report_buttons(&xbox) & (1u << 2)) != 0);        // X on button 3
    CHECK((report_buttons(&xbox) & (1u << 6)) != 0);        // LT on button 7
    CHECK((report_buttons(&playstation) & (1u << 0)) != 0); // X on button 1
    CHECK((report_buttons(&playstation) & (1u << 1)) != 0); // A on button 2
}

static void target_metadata_is_stable(void) {
    CHECK(strcmp(bt_hid_target_label(BT_HID_TARGET_GENERIC), "bluetooth-hid") == 0);
    CHECK(strcmp(bt_hid_target_label(BT_HID_TARGET_XBOX), "bluetooth-xbox-hid") == 0);
    CHECK(strcmp(bt_hid_target_label(BT_HID_TARGET_PLAYSTATION),
                 "bluetooth-playstation-hid") == 0);
    CHECK(bt_hid_product_id(BT_HID_TARGET_GENERIC) == 0xCB10u);
    CHECK(bt_hid_product_id(BT_HID_TARGET_XBOX) == 0xCB11u);
    CHECK(bt_hid_product_id(BT_HID_TARGET_PLAYSTATION) == 0xCB12u);
}

int main(void) {
    descriptor_has_expected_shape();
    neutral_report_is_centered_and_released();
    generic_report_maps_core_controls();
    target_button_orders_are_distinct();
    target_metadata_is_stable();
    puts("bt_hid_report tests passed");
    return 0;
}
