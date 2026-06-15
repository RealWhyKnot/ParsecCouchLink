#pragma once

#include <stddef.h>
#include <stdint.h>

#include "gamepad_state.h"

// Maple frame response codes. Negative Maple responses are represented by
// their single-byte two's-complement wire values.
#define MAPLE_CMD_DEVICE_INFO 0x01
#define MAPLE_CMD_EXT_DEVICE_INFO 0x02
#define MAPLE_CMD_RESET 0x03
#define MAPLE_CMD_SHUTDOWN 0x04
#define MAPLE_CMD_GET_CONDITION 0x09
#define MAPLE_CMD_SET_CONDITION 0x0E

#define MAPLE_RESP_DEVICE_INFO 0x05
#define MAPLE_RESP_EXT_DEVICE_INFO 0x06
#define MAPLE_RESP_ACK 0x07
#define MAPLE_RESP_DATA_TRANSFER 0x08
#define MAPLE_RESP_BAD_FUNC 0xFE
#define MAPLE_RESP_BAD_CMD 0xFD

#define MAPLE_FUNCTION_CONTROLLER 0x00000001u

#define MAPLE_ADDR_PORT_A_HOST 0x00
#define MAPLE_ADDR_PORT_A_MAIN 0x20
#define MAPLE_ADDR_PORT_B_HOST 0x40
#define MAPLE_ADDR_PORT_B_MAIN 0x60
#define MAPLE_ADDR_PORT_C_HOST 0x80
#define MAPLE_ADDR_PORT_C_MAIN 0xA0
#define MAPLE_ADDR_PORT_D_HOST 0xC0
#define MAPLE_ADDR_PORT_D_MAIN 0xE0

#define MAPLE_DC_BTN_C 0x0001u
#define MAPLE_DC_BTN_B 0x0002u
#define MAPLE_DC_BTN_A 0x0004u
#define MAPLE_DC_BTN_START 0x0008u
#define MAPLE_DC_BTN_UP 0x0010u
#define MAPLE_DC_BTN_DOWN 0x0020u
#define MAPLE_DC_BTN_LEFT 0x0040u
#define MAPLE_DC_BTN_RIGHT 0x0080u
#define MAPLE_DC_BTN_Z 0x0100u
#define MAPLE_DC_BTN_Y 0x0200u
#define MAPLE_DC_BTN_X 0x0400u
#define MAPLE_DC_BTN_D 0x0800u
#define MAPLE_DC_BTN_UP2 0x1000u
#define MAPLE_DC_BTN_DOWN2 0x2000u
#define MAPLE_DC_BTN_LEFT2 0x4000u
#define MAPLE_DC_BTN_RIGHT2 0x8000u

// XINPUT_GAMEPAD.wButtons masks, duplicated here so the firmware protocol
// layer stays SDK-free and can be compiled in host tests.
#define MAPLE_XINPUT_DPAD_UP 0x0001u
#define MAPLE_XINPUT_DPAD_DOWN 0x0002u
#define MAPLE_XINPUT_DPAD_LEFT 0x0004u
#define MAPLE_XINPUT_DPAD_RIGHT 0x0008u
#define MAPLE_XINPUT_START 0x0010u
#define MAPLE_XINPUT_BACK 0x0020u
#define MAPLE_XINPUT_LEFT_THUMB 0x0040u
#define MAPLE_XINPUT_RIGHT_THUMB 0x0080u
#define MAPLE_XINPUT_LEFT_SHOULDER 0x0100u
#define MAPLE_XINPUT_RIGHT_SHOULDER 0x0200u
#define MAPLE_XINPUT_A 0x1000u
#define MAPLE_XINPUT_B 0x2000u
#define MAPLE_XINPUT_X 0x4000u
#define MAPLE_XINPUT_Y 0x8000u

#define MAPLE_XINPUT_LEFT_THUMB_DEADZONE 7849
#define MAPLE_XINPUT_RIGHT_THUMB_DEADZONE 8689
#define MAPLE_XINPUT_TRIGGER_THRESHOLD 30

#define MAPLE_CONDITION_BYTES 8
#define MAPLE_DEVICE_INFO_BYTES 112
#define MAPLE_MAX_RESPONSE_BYTES (4 + MAPLE_DEVICE_INFO_BYTES + 1)

typedef enum {
    MAPLE_MAP_STANDARD = 0,
    MAPLE_MAP_EXTENDED = 1,
} maple_map_mode_t;

typedef struct {
    uint16_t buttons; // Active-low on the Maple wire: 0 = pressed.
    uint8_t rtrigger;
    uint8_t ltrigger;
    uint8_t joyx;
    uint8_t joyy;
    uint8_t joyx2;
    uint8_t joyy2;
} maple_controller_condition_t;

typedef struct {
    uint8_t command;
    uint8_t recipient_addr;
    uint8_t sender_addr;
    const uint8_t *payload;
    uint8_t payload_words;
} maple_request_t;

uint8_t maple_xinput_axis_x_to_dc(int16_t value);
uint8_t maple_xinput_axis_y_to_dc(int16_t value);
uint8_t maple_xinput_trigger_to_dc(uint8_t value);
uint32_t maple_controller_capabilities(maple_map_mode_t mode);

maple_controller_condition_t maple_translate_xinput(const gamepad_state_t *state,
                                                    maple_map_mode_t mode);

void maple_encode_condition(const maple_controller_condition_t *condition, uint8_t out[8]);
uint8_t maple_frame_checksum(const uint8_t *frame, size_t frame_len);

// Builds a complete Maple response packet: frame header, payload words, and
// trailing XOR checksum byte. Returns 0 if the output buffer is too small.
size_t maple_build_response(const maple_request_t *request, const gamepad_state_t *state,
                            maple_map_mode_t mode, uint8_t *out, size_t out_cap);
