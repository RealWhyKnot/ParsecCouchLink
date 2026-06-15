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

static uint16_t report_buttons(const dinput_report_t *report) {
    return (uint16_t)report->bytes[8] | ((uint16_t)report->bytes[9] << 8);
}

static void test_neutral_report_matches_usb4maple_sample(void) {
    static const uint8_t expected[DINPUT_WIRE_REPORT_LEN] = {
        0x03, 0x0F, 0x7F, 0x7F, 0x7F, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x64,
    };
    gamepad_state_t state = neutral_state();
    dinput_report_t report;
    dinput_build_report(&state, &report);
    CHECK(memcmp(report.bytes, expected, sizeof(expected)) == 0);
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

static void test_report_axis_and_trigger_order(void) {
    gamepad_state_t state = neutral_state();
    state.left_x = -32768;
    state.left_y = 32767;
    state.right_x = 32767;
    state.right_y = -32768;
    state.left_trigger = 0x22;
    state.right_trigger = 0xCC;

    dinput_report_t report;
    dinput_build_report(&state, &report);

    CHECK(report.bytes[2] == 0x00);
    CHECK(report.bytes[3] == 0x00);
    CHECK(report.bytes[4] == 0xFF);
    CHECK(report.bytes[5] == 0xFF);
    CHECK(report.bytes[6] == 0xCC);
    CHECK(report.bytes[7] == 0x22);
}

static void test_button_mapping(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_X | DINPUT_XINPUT_Y |
                    DINPUT_XINPUT_LEFT_SHOULDER | DINPUT_XINPUT_RIGHT_SHOULDER |
                    DINPUT_XINPUT_BACK | DINPUT_XINPUT_START | DINPUT_XINPUT_LEFT_THUMB |
                    DINPUT_XINPUT_RIGHT_THUMB | DINPUT_XINPUT_GUIDE;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD - 1;
    state.right_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;

    dinput_report_t report;
    dinput_build_report(&state, &report);

    uint16_t expected = DINPUT_BUTTON_A | DINPUT_BUTTON_B | DINPUT_BUTTON_X | DINPUT_BUTTON_Y |
                        DINPUT_BUTTON_LB | DINPUT_BUTTON_RB | DINPUT_BUTTON_BACK |
                        DINPUT_BUTTON_START | DINPUT_BUTTON_LS | DINPUT_BUTTON_RS |
                        DINPUT_BUTTON_RT_DIGITAL | DINPUT_BUTTON_HOME;
    CHECK(report_buttons(&report) == expected);
    CHECK((report_buttons(&report) & DINPUT_BUTTON_LT_DIGITAL) == 0);

    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;
    dinput_build_report(&state, &report);
    CHECK((report_buttons(&report) & DINPUT_BUTTON_LT_DIGITAL) != 0);
}

int main(void) {
    test_neutral_report_matches_usb4maple_sample();
    test_axis_conversion();
    test_hat_values();
    test_report_axis_and_trigger_order();
    test_button_mapping();
    if (failures != 0)
        return 1;
    puts("dinput_report tests passed");
    return 0;
}
