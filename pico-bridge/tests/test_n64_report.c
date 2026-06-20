#include <assert.h>
#include <stdio.h>

#include "dinput_report.h"
#include "n64_report.h"

static gamepad_state_t neutral_state(void) {
    gamepad_state_t state = {0};
    return state;
}

static void neutral_report_is_centered(void) {
    gamepad_state_t state = neutral_state();
    struct joybus_n64_controller_state report = n64_report_from_gamepad(&state);
    assert(report.buttons == 0);
    assert(report.stick_x == 0);
    assert(report.stick_y == 0);
}

static void maps_buttons_and_triggers(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = DINPUT_XINPUT_A | DINPUT_XINPUT_B | DINPUT_XINPUT_START |
                    DINPUT_XINPUT_DPAD_UP | DINPUT_XINPUT_DPAD_LEFT | DINPUT_XINPUT_LEFT_SHOULDER |
                    DINPUT_XINPUT_RIGHT_SHOULDER;
    state.left_trigger = DINPUT_TRIGGER_BUTTON_THRESHOLD;

    struct joybus_n64_controller_state report = n64_report_from_gamepad(&state);
    assert(report.buttons & JOYBUS_N64_BUTTON_A);
    assert(report.buttons & JOYBUS_N64_BUTTON_B);
    assert(report.buttons & JOYBUS_N64_BUTTON_START);
    assert(report.buttons & JOYBUS_N64_BUTTON_UP);
    assert(report.buttons & JOYBUS_N64_BUTTON_LEFT);
    assert(report.buttons & JOYBUS_N64_BUTTON_L);
    assert(report.buttons & JOYBUS_N64_BUTTON_R);
    assert(report.buttons & JOYBUS_N64_BUTTON_Z);
}

static void maps_c_buttons_from_right_stick(void) {
    gamepad_state_t state = neutral_state();
    state.right_x = N64_C_BUTTON_THRESHOLD;
    state.right_y = -N64_C_BUTTON_THRESHOLD;

    struct joybus_n64_controller_state report = n64_report_from_gamepad(&state);
    assert(report.buttons & JOYBUS_N64_BUTTON_C_RIGHT);
    assert(report.buttons & JOYBUS_N64_BUTTON_C_DOWN);

    state.right_x = -N64_C_BUTTON_THRESHOLD;
    state.right_y = N64_C_BUTTON_THRESHOLD;
    report = n64_report_from_gamepad(&state);
    assert(report.buttons & JOYBUS_N64_BUTTON_C_LEFT);
    assert(report.buttons & JOYBUS_N64_BUTTON_C_UP);
}

static void scales_and_clamps_axes(void) {
    gamepad_state_t state = neutral_state();
    state.left_x = INT16_MIN;
    state.left_y = INT16_MAX;

    struct joybus_n64_controller_state report = n64_report_from_gamepad(&state);
    assert(report.stick_x == N64_STICK_MIN);
    assert(report.stick_y == N64_STICK_MAX);

    state.left_x = INT16_MAX;
    state.left_y = INT16_MIN;
    report = n64_report_from_gamepad(&state);
    assert(report.stick_x == N64_STICK_MAX);
    assert(report.stick_y == N64_STICK_MIN);
}

static void pack_round_trip_preserves_wire_bytes(void) {
    struct joybus_n64_controller_state report = {
        .buttons = JOYBUS_N64_BUTTON_A | JOYBUS_N64_BUTTON_C_UP | JOYBUS_N64_BUTTON_LEFT,
        .stick_x = -40,
        .stick_y = 63,
    };
    uint32_t packed = n64_report_pack(&report);
    struct joybus_n64_controller_state back = n64_report_unpack(packed);
    assert(back.buttons == report.buttons);
    assert(back.stick_x == report.stick_x);
    assert(back.stick_y == report.stick_y);
}

int main(void) {
    neutral_report_is_centered();
    maps_buttons_and_triggers();
    maps_c_buttons_from_right_stick();
    scales_and_clamps_axes();
    pack_round_trip_preserves_wire_bytes();
    puts("n64_report tests passed");
    return 0;
}
