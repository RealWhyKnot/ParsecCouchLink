#include <stdio.h>
#include <string.h>

#include "dinput_report.h"

static int failures = 0;

#define CHECK(cond)                                                                                \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);                                 \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

static gamepad_state_t neutral_state(void) {
    gamepad_state_t state;
    memset(&state, 0, sizeof(state));
    return state;
}

static void test_axis_conversion(void) {
    CHECK(dinput_axis_x_to_hid(-32768) == 0x00);
    CHECK(dinput_axis_x_to_hid(0) == 0x7F);
    CHECK(dinput_axis_x_to_hid(32767) == 0xFF);

    CHECK(dinput_axis_y_to_hid(32767) == 0x00);
    CHECK(dinput_axis_y_to_hid(0) == 0x7F);
    CHECK(dinput_axis_y_to_hid(-32768) == 0xFF);
}

static void test_hat_values(void) {
    CHECK(dinput_hat_from_buttons(0) == DINPUT_HAT_NEUTRAL);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_UP) == DINPUT_HAT_UP);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_UP | DINPUT_XINPUT_DPAD_RIGHT) ==
          DINPUT_HAT_UP_RIGHT);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_RIGHT) == DINPUT_HAT_RIGHT);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_DOWN | DINPUT_XINPUT_DPAD_RIGHT) ==
          DINPUT_HAT_DOWN_RIGHT);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_DOWN) == DINPUT_HAT_DOWN);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_DOWN | DINPUT_XINPUT_DPAD_LEFT) ==
          DINPUT_HAT_DOWN_LEFT);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_LEFT) == DINPUT_HAT_LEFT);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_UP | DINPUT_XINPUT_DPAD_LEFT) ==
          DINPUT_HAT_UP_LEFT);
    CHECK(dinput_hat_from_buttons(DINPUT_XINPUT_DPAD_UP | DINPUT_XINPUT_DPAD_DOWN) ==
          DINPUT_HAT_NEUTRAL);
}

static void test_ps3_neutral_report(void) {
    gamepad_state_t state = neutral_state();
    dinput_report_t report;
    dinput_build_ps3_report(&state, &report);

    CHECK(report.len == DINPUT_PS3_WIRE_REPORT_LEN);
    CHECK(report.report_id == DINPUT_PS3_REPORT_ID);
    CHECK(report.bytes[0] == 0x01);
    CHECK(report.bytes[2] == 0x00);
    CHECK(report.bytes[3] == 0x00);
    CHECK(report.bytes[6] == 0x80);
    CHECK(report.bytes[7] == 0x80);
    CHECK(report.bytes[8] == 0x80);
    CHECK(report.bytes[9] == 0x80);
    CHECK(report.bytes[18] == 0x00);
    CHECK(report.bytes[19] == 0x00);
    CHECK(report.bytes[29] == 0x02);
    CHECK(report.bytes[30] == 0x05);
    CHECK(report.bytes[31] == 0x10);
    CHECK(report.bytes[41] == 0x01);
    CHECK(report.bytes[42] == 0xFF);
    CHECK(report.bytes[48] == 0xFF);
}

static void test_ps3_buttons_and_analogs(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_X | DINPUT_XINPUT_Y |
                    DINPUT_XINPUT_LEFT_SHOULDER | DINPUT_XINPUT_RIGHT_SHOULDER |
                    DINPUT_XINPUT_BACK | DINPUT_XINPUT_START | DINPUT_XINPUT_LEFT_THUMB |
                    DINPUT_XINPUT_RIGHT_THUMB | DINPUT_XINPUT_GUIDE | DINPUT_XINPUT_DPAD_UP |
                    DINPUT_XINPUT_DPAD_RIGHT;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD - 1;
    state.right_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;
    state.left_x = -32768;
    state.left_y = 32767;
    state.right_x = 32767;
    state.right_y = -32768;

    dinput_report_t report;
    dinput_build_ps3_report(&state, &report);

    CHECK(report.bytes[2] == 0x3F);
    CHECK(report.bytes[3] == 0xFE);
    CHECK(report.bytes[4] == 0x01);
    CHECK(report.bytes[6] == 0x00);
    CHECK(report.bytes[7] == 0x00);
    CHECK(report.bytes[8] == 0xFF);
    CHECK(report.bytes[9] == 0xFF);
    CHECK(report.bytes[14] == 0xFF);
    CHECK(report.bytes[15] == 0xFF);
    CHECK(report.bytes[18] == DINPUT_TRIGGER_BUTTON_THRESHOLD - 1);
    CHECK(report.bytes[19] == DINPUT_TRIGGER_BUTTON_THRESHOLD);
    CHECK(report.bytes[22] == 0xFF);
    CHECK(report.bytes[23] == 0xFF);
    CHECK(report.bytes[24] == 0xFF);
    CHECK(report.bytes[25] == 0xFF);
}

static void test_ps4_neutral_report(void) {
    gamepad_state_t state = neutral_state();
    dinput_report_t report;
    dinput_build_ps4_report(&state, 0, &report);

    CHECK(report.len == DINPUT_PS4_WIRE_REPORT_LEN);
    CHECK(report.report_id == DINPUT_PS4_REPORT_ID);
    CHECK(report.bytes[0] == 0x01);
    CHECK(report.bytes[1] == 0x80);
    CHECK(report.bytes[2] == 0x80);
    CHECK(report.bytes[3] == 0x80);
    CHECK(report.bytes[4] == 0x80);
    CHECK(report.bytes[5] == 0x08);
    CHECK(report.bytes[6] == 0x00);
    CHECK(report.bytes[7] == 0x00);
    CHECK(report.bytes[8] == 0x00);
    CHECK(report.bytes[9] == 0x00);
    CHECK(report.bytes[30] == 0x1B);
}

static void test_ps4_buttons_and_counter(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_X | DINPUT_XINPUT_Y |
                    DINPUT_XINPUT_LEFT_SHOULDER | DINPUT_XINPUT_RIGHT_SHOULDER |
                    DINPUT_XINPUT_BACK | DINPUT_XINPUT_START | DINPUT_XINPUT_LEFT_THUMB |
                    DINPUT_XINPUT_RIGHT_THUMB | DINPUT_XINPUT_GUIDE | DINPUT_XINPUT_DPAD_DOWN |
                    DINPUT_XINPUT_DPAD_LEFT;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;
    state.right_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD - 1;

    dinput_report_t report;
    dinput_build_ps4_report(&state, 9, &report);

    CHECK((report.bytes[5] & 0x0F) == DINPUT_HAT_DOWN_LEFT);
    CHECK((report.bytes[5] & 0xF0) == 0xF0);
    CHECK(report.bytes[6] == 0xF7);
    CHECK(report.bytes[7] == ((9u << 2) | 0x01u));
    CHECK(report.bytes[8] == DINPUT_TRIGGER_BUTTON_THRESHOLD);
    CHECK(report.bytes[9] == DINPUT_TRIGGER_BUTTON_THRESHOLD - 1);
}

static void test_generic_hid_neutral_report(void) {
    gamepad_state_t state = neutral_state();
    dinput_report_t report;
    dinput_build_generic_hid_report(&state, &report);

    CHECK(report.len == DINPUT_GENERIC_HID_WIRE_REPORT_LEN);
    CHECK(report.report_id == DINPUT_GENERIC_HID_REPORT_ID);
    CHECK(report.bytes[0] == 0x00);
    CHECK(report.bytes[1] == 0x00);
    CHECK(report.bytes[2] == 0x80);
    CHECK(report.bytes[3] == 0x80);
    CHECK(report.bytes[4] == 0x80);
    CHECK(report.bytes[5] == 0x80);
    CHECK(report.bytes[6] == 0x00);
    CHECK(report.bytes[7] == 0x00);
}

static void test_generic_hid_buttons_and_axes(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = DINPUT_XINPUT_X | DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_Y |
                    DINPUT_XINPUT_LEFT_SHOULDER | DINPUT_XINPUT_RIGHT_SHOULDER |
                    DINPUT_XINPUT_BACK | DINPUT_XINPUT_START | DINPUT_XINPUT_LEFT_THUMB |
                    DINPUT_XINPUT_RIGHT_THUMB;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;
    state.right_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD - 1;
    state.left_x = -32768;
    state.left_y = 32767;
    state.right_x = 32767;
    state.right_y = -32768;

    dinput_report_t report;
    dinput_build_generic_hid_report(&state, &report);

    CHECK(report.bytes[0] == 0x7F);
    CHECK(report.bytes[1] == 0x0F);
    CHECK(report.bytes[2] == 0x00);
    CHECK(report.bytes[3] == 0x00);
    CHECK(report.bytes[4] == 0xFF);
    CHECK(report.bytes[5] == 0xFF);
    CHECK(report.bytes[6] == DINPUT_TRIGGER_BUTTON_THRESHOLD);
    CHECK(report.bytes[7] == DINPUT_TRIGGER_BUTTON_THRESHOLD - 1);
}

int main(void) {
    test_axis_conversion();
    test_hat_values();
    test_ps3_neutral_report();
    test_ps3_buttons_and_analogs();
    test_ps4_neutral_report();
    test_ps4_buttons_and_counter();
    test_generic_hid_neutral_report();
    test_generic_hid_buttons_and_axes();
    if (failures != 0)
        return 1;
    puts("dinput_report tests passed");
    return 0;
}
