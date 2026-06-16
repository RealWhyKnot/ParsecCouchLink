#pragma once

#include <stdint.h>

#include "gamepad_state.h"

#define DINPUT_PS3_REPORT_ID 0x01u
#define DINPUT_PS3_WIRE_REPORT_LEN 49u
#define DINPUT_PS3_PAYLOAD_REPORT_LEN 48u

#define DINPUT_PS4_REPORT_ID 0x01u
#define DINPUT_PS4_WIRE_REPORT_LEN 64u
#define DINPUT_PS4_PAYLOAD_REPORT_LEN 63u

#define DINPUT_MAX_WIRE_REPORT_LEN 64u

#define DINPUT_HAT_UP 0x00u
#define DINPUT_HAT_UP_RIGHT 0x01u
#define DINPUT_HAT_RIGHT 0x02u
#define DINPUT_HAT_DOWN_RIGHT 0x03u
#define DINPUT_HAT_DOWN 0x04u
#define DINPUT_HAT_DOWN_LEFT 0x05u
#define DINPUT_HAT_LEFT 0x06u
#define DINPUT_HAT_UP_LEFT 0x07u
#define DINPUT_HAT_NEUTRAL 0x0Fu

#define DINPUT_XINPUT_DPAD_UP 0x0001u
#define DINPUT_XINPUT_DPAD_DOWN 0x0002u
#define DINPUT_XINPUT_DPAD_LEFT 0x0004u
#define DINPUT_XINPUT_DPAD_RIGHT 0x0008u
#define DINPUT_XINPUT_START 0x0010u
#define DINPUT_XINPUT_BACK 0x0020u
#define DINPUT_XINPUT_LEFT_THUMB 0x0040u
#define DINPUT_XINPUT_RIGHT_THUMB 0x0080u
#define DINPUT_XINPUT_LEFT_SHOULDER 0x0100u
#define DINPUT_XINPUT_RIGHT_SHOULDER 0x0200u
#define DINPUT_XINPUT_GUIDE 0x0400u
#define DINPUT_XINPUT_A 0x1000u
#define DINPUT_XINPUT_B 0x2000u
#define DINPUT_XINPUT_X 0x4000u
#define DINPUT_XINPUT_Y 0x8000u

#define DINPUT_TRIGGER_BUTTON_THRESHOLD 30u

#define DINPUT_BUTTON_A 0x0001u
#define DINPUT_BUTTON_B 0x0002u
#define DINPUT_BUTTON_X 0x0004u
#define DINPUT_BUTTON_Y 0x0008u
#define DINPUT_BUTTON_LB 0x0010u
#define DINPUT_BUTTON_RB 0x0020u
#define DINPUT_BUTTON_BACK 0x0040u
#define DINPUT_BUTTON_START 0x0080u
#define DINPUT_BUTTON_LS 0x0100u
#define DINPUT_BUTTON_RS 0x0200u
#define DINPUT_BUTTON_LT_DIGITAL 0x0400u
#define DINPUT_BUTTON_RT_DIGITAL 0x0800u
#define DINPUT_BUTTON_HOME 0x1000u
#define DINPUT_BUTTON_STAR_TURBO 0x8000u

typedef struct {
    uint8_t bytes[DINPUT_MAX_WIRE_REPORT_LEN];
    uint8_t len;
    uint8_t report_id;
} dinput_report_t;

uint8_t dinput_axis_x_to_hid(int16_t value);
uint8_t dinput_axis_y_to_hid(int16_t value);
uint8_t dinput_hat_from_buttons(uint16_t buttons);
uint16_t dinput_buttons_from_gamepad(const gamepad_state_t *state);
void dinput_build_ps3_report(const gamepad_state_t *state, dinput_report_t *out);
void dinput_build_ps4_report(const gamepad_state_t *state, uint8_t report_counter,
                             dinput_report_t *out);
