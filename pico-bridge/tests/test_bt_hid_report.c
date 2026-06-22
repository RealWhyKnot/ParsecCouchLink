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

static uint16_t read_le16(const uint8_t *src) {
    return (uint16_t)src[0] | ((uint16_t)src[1] << 8);
}

static uint32_t read_le32(const uint8_t *src) {
    return (uint32_t)src[0] | ((uint32_t)src[1] << 8) | ((uint32_t)src[2] << 16) |
           ((uint32_t)src[3] << 24);
}

static uint32_t test_crc32_le_update(uint32_t crc, const uint8_t *data, uint8_t n) {
    for (uint8_t i = 0; i < n; i++) {
        crc ^= data[i];
        for (uint8_t bit = 0; bit < 8; bit++) {
            uint32_t mask = 0u - (crc & 1u);
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return crc;
}

static uint32_t expected_ds4_input_crc(const uint8_t *report, uint8_t report_len_without_crc) {
    uint8_t seed = 0xA1u;
    uint32_t crc = test_crc32_le_update(0xFFFFFFFFu, &seed, 1u);
    return ~test_crc32_le_update(crc, report, report_len_without_crc);
}

static bool descriptor_contains_report_id(const uint8_t *descriptor, uint16_t len,
                                          uint8_t report_id) {
    for (uint16_t i = 0; i + 1 < len; i++) {
        if (descriptor[i] == 0x85 && descriptor[i + 1] == report_id)
            return true;
    }
    return false;
}

static void generic_descriptor_has_expected_shape(void) {
    uint16_t len = 0;
    const uint8_t *descriptor = bt_hid_descriptor(BT_HID_TARGET_GENERIC, &len);

    CHECK(descriptor != NULL);
    CHECK(len > 40);
    CHECK(descriptor[0] == 0x05);
    CHECK(descriptor[1] == 0x01);
    CHECK(descriptor[6] == 0x85);
    CHECK(descriptor[7] == BT_HID_GENERIC_REPORT_ID);

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

static void generic_neutral_report_is_centered_and_released(void) {
    gamepad_state_t state = {0};
    bt_hid_report_t report;
    bt_hid_build_report(BT_HID_TARGET_GENERIC, &state, &report);

    CHECK(report.len == BT_HID_GENERIC_WIRE_REPORT_LEN);
    CHECK(report.report_id == BT_HID_GENERIC_REPORT_ID);
    CHECK(report.bytes[0] == BT_HID_GENERIC_REPORT_ID);
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
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_BACK | DINPUT_XINPUT_START |
                    DINPUT_XINPUT_DPAD_UP | DINPUT_XINPUT_DPAD_RIGHT;
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

static void xbox_wireless_profile_matches_expected_identity_and_shape(void) {
    uint16_t len = 0;
    const uint8_t *descriptor = bt_hid_descriptor(BT_HID_TARGET_XBOX, &len);

    CHECK(descriptor != NULL);
    CHECK(len == 306u);
    CHECK(descriptor_contains_report_id(descriptor, len, 0x01));
    CHECK(descriptor_contains_report_id(descriptor, len, 0x02));
    CHECK(descriptor_contains_report_id(descriptor, len, 0x03));
    CHECK(descriptor_contains_report_id(descriptor, len, 0x04));
    CHECK(strcmp(bt_hid_local_name(BT_HID_TARGET_XBOX), "Xbox Wireless Controller") == 0);
    CHECK(strcmp(bt_hid_service_name(BT_HID_TARGET_XBOX), "Xbox Wireless Controller") == 0);
    CHECK(bt_hid_vendor_id(BT_HID_TARGET_XBOX) == 0x045Eu);
    CHECK(bt_hid_product_id(BT_HID_TARGET_XBOX) == 0x02FDu);
    CHECK(bt_hid_bcd_version(BT_HID_TARGET_XBOX) == 0x0903u);

    gamepad_state_t neutral = {0};
    bt_hid_report_t report;
    bt_hid_build_report(BT_HID_TARGET_XBOX, &neutral, &report);
    CHECK(report.len == BT_HID_XBOX_WIRE_REPORT_LEN);
    CHECK(report.report_id == BT_HID_XBOX_REPORT_ID);
    CHECK(read_le16(&report.bytes[1]) == 0x8000u);
    CHECK(read_le16(&report.bytes[3]) == 0x8000u);
    CHECK(read_le16(&report.bytes[5]) == 0x8000u);
    CHECK(read_le16(&report.bytes[7]) == 0x8000u);
    CHECK((read_le16(&report.bytes[9]) & 0x03FFu) == 0);
    CHECK((read_le16(&report.bytes[11]) & 0x03FFu) == 0);
    CHECK((report.bytes[13] & 0x0Fu) == 0);
    CHECK((read_le16(&report.bytes[14]) & 0x03FFu) == 0);

    gamepad_state_t state = {0};
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_X | DINPUT_XINPUT_DPAD_UP |
                    DINPUT_XINPUT_DPAD_RIGHT | DINPUT_XINPUT_START;
    state.left_trigger = 255;
    state.left_x = -32768;
    state.left_y = 32767;
    state.right_x = 32767;
    state.right_y = -32768;
    bt_hid_build_report(BT_HID_TARGET_XBOX, &state, &report);
    CHECK(read_le16(&report.bytes[1]) == 0x0000u);
    CHECK(read_le16(&report.bytes[3]) == 0x0000u);
    CHECK(read_le16(&report.bytes[5]) == 0xFFFFu);
    CHECK(read_le16(&report.bytes[7]) == 0xFFFFu);
    CHECK((read_le16(&report.bytes[9]) & 0x03FFu) == 0x03FFu);
    CHECK((report.bytes[13] & 0x0Fu) == 0x02u);
    CHECK((read_le16(&report.bytes[14]) & (1u << 0)) != 0); // A
    CHECK((read_le16(&report.bytes[14]) & (1u << 2)) != 0); // X
    CHECK((read_le16(&report.bytes[14]) & (1u << 7)) != 0); // Start
}

static void ds4_profile_matches_expected_identity_and_shape(void) {
    uint16_t len = 0;
    const uint8_t *descriptor = bt_hid_descriptor(BT_HID_TARGET_PLAYSTATION, &len);

    CHECK(descriptor != NULL);
    CHECK(len > 300u);
    CHECK(descriptor_contains_report_id(descriptor, len, 0x01));
    CHECK(descriptor_contains_report_id(descriptor, len, 0x02));
    CHECK(descriptor_contains_report_id(descriptor, len, 0x11));
    CHECK(strcmp(bt_hid_local_name(BT_HID_TARGET_PLAYSTATION), "Wireless Controller") == 0);
    CHECK(strcmp(bt_hid_service_name(BT_HID_TARGET_PLAYSTATION), "Wireless Controller") == 0);
    CHECK(bt_hid_vendor_id(BT_HID_TARGET_PLAYSTATION) == 0x054Cu);
    CHECK(bt_hid_product_id(BT_HID_TARGET_PLAYSTATION) == 0x05C4u);
    CHECK(bt_hid_bcd_version(BT_HID_TARGET_PLAYSTATION) == 0x0100u);

    gamepad_state_t neutral = {0};
    bt_hid_report_t report;
    bt_hid_build_report(BT_HID_TARGET_PLAYSTATION, &neutral, &report);
    CHECK(report.len == BT_HID_PS4_WIRE_REPORT_LEN);
    CHECK(report.report_id == BT_HID_PS4_REPORT_ID);
    CHECK(report.bytes[0] == BT_HID_PS4_REPORT_ID);
    CHECK(report.bytes[1] == 0xC0u);
    CHECK(report.bytes[3] == 0x80u);
    CHECK(report.bytes[4] == 0x80u);
    CHECK(report.bytes[5] == 0x80u);
    CHECK(report.bytes[6] == 0x80u);
    CHECK((report.bytes[7] & 0x0Fu) == 0x08u);
    CHECK(report.bytes[10] == 0);
    CHECK(report.bytes[11] == 0);
    CHECK(report.bytes[74] != 0 || report.bytes[75] != 0 || report.bytes[76] != 0 ||
          report.bytes[77] != 0);
    CHECK(read_le32(&report.bytes[74]) ==
          expected_ds4_input_crc(report.bytes, BT_HID_PS4_WIRE_REPORT_LEN - 4u));

    gamepad_state_t state = {0};
    state.buttons = DINPUT_XINPUT_X | DINPUT_XINPUT_A | DINPUT_XINPUT_LEFT_SHOULDER |
                    DINPUT_XINPUT_DPAD_DOWN | DINPUT_XINPUT_DPAD_LEFT;
    state.right_trigger = 255;
    bt_hid_build_report(BT_HID_TARGET_PLAYSTATION, &state, &report);
    CHECK((report.bytes[7] & 0x0Fu) == DINPUT_HAT_DOWN_LEFT);
    CHECK((report.bytes[7] & (1u << 4)) != 0); // Square
    CHECK((report.bytes[7] & (1u << 5)) != 0); // Cross
    CHECK((report.bytes[8] & (1u << 0)) != 0); // L1
    CHECK((report.bytes[8] & (1u << 3)) != 0); // R2 digital
    CHECK(report.bytes[11] == 255);
    CHECK(read_le32(&report.bytes[74]) ==
          expected_ds4_input_crc(report.bytes, BT_HID_PS4_WIRE_REPORT_LEN - 4u));
}

static void target_metadata_is_stable(void) {
    CHECK(strcmp(bt_hid_target_label(BT_HID_TARGET_GENERIC), "bluetooth") == 0);
    CHECK(strcmp(bt_hid_target_label(BT_HID_TARGET_XBOX), "bluetooth-xbox") == 0);
    CHECK(strcmp(bt_hid_target_label(BT_HID_TARGET_PLAYSTATION), "bluetooth-playstation") == 0);
    CHECK(strcmp(bt_hid_local_name(BT_HID_TARGET_GENERIC), "CouchLink BT HID 00:00:00:00:00:00") ==
          0);
    CHECK(bt_hid_vendor_id(BT_HID_TARGET_GENERIC) == 0x2E8Au);
    CHECK(bt_hid_product_id(BT_HID_TARGET_GENERIC) == 0xCB10u);
    CHECK(bt_hid_bcd_version(BT_HID_TARGET_GENERIC) == 0x0100u);
}

int main(void) {
    generic_descriptor_has_expected_shape();
    generic_neutral_report_is_centered_and_released();
    generic_report_maps_core_controls();
    xbox_wireless_profile_matches_expected_identity_and_shape();
    ds4_profile_matches_expected_identity_and_shape();
    target_metadata_is_stable();
    puts("bt_hid_report tests passed");
    return 0;
}
