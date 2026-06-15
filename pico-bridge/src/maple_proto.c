#include "maple_proto.h"

#include <stdbool.h>
#include <string.h>

#define MAPLE_CAP_STANDARD 0x000F06FEu
#define MAPLE_CAP_EXTENDED 0x003F0FFFu

static void dc_press(maple_controller_condition_t *condition, uint16_t mask) {
    condition->buttons = (uint16_t)(condition->buttons & (uint16_t)~mask);
}

static bool stick_inside_deadzone(int16_t x, int16_t y, int deadzone) {
    int64_t dx = x;
    int64_t dy = y;
    int64_t limit = deadzone;
    return (dx * dx + dy * dy) < (limit * limit);
}

uint8_t maple_xinput_axis_x_to_dc(int16_t value) {
    if (value < 0) {
        int32_t scaled = 128 + ((int32_t)value * 128) / 32768;
        return (uint8_t)scaled;
    }
    int32_t scaled = 128 + ((int32_t)value * 127) / 32767;
    return (uint8_t)scaled;
}

uint8_t maple_xinput_axis_y_to_dc(int16_t value) {
    if (value < 0) {
        int32_t scaled = 128 + ((int32_t)(-value) * 127) / 32768;
        return (uint8_t)scaled;
    }
    int32_t scaled = 128 - ((int32_t)value * 128) / 32767;
    return (uint8_t)scaled;
}

uint8_t maple_xinput_trigger_to_dc(uint8_t value) {
    return (value < MAPLE_XINPUT_TRIGGER_THRESHOLD) ? 0 : value;
}

uint32_t maple_controller_capabilities(maple_map_mode_t mode) {
    return (mode == MAPLE_MAP_EXTENDED) ? MAPLE_CAP_EXTENDED : MAPLE_CAP_STANDARD;
}

maple_controller_condition_t maple_translate_xinput(const gamepad_state_t *state,
                                                    maple_map_mode_t mode) {
    maple_controller_condition_t condition;
    condition.buttons = 0xFFFFu;
    condition.rtrigger = maple_xinput_trigger_to_dc(state->right_trigger);
    condition.ltrigger = maple_xinput_trigger_to_dc(state->left_trigger);
    condition.joyx = 128;
    condition.joyy = 128;
    condition.joyx2 = 128;
    condition.joyy2 = 128;

    if (!stick_inside_deadzone(state->left_x, state->left_y, MAPLE_XINPUT_LEFT_THUMB_DEADZONE)) {
        condition.joyx = maple_xinput_axis_x_to_dc(state->left_x);
        condition.joyy = maple_xinput_axis_y_to_dc(state->left_y);
    }

    uint16_t buttons = state->buttons;
    if (buttons & MAPLE_XINPUT_A)
        dc_press(&condition, MAPLE_DC_BTN_A);
    if (buttons & MAPLE_XINPUT_B)
        dc_press(&condition, MAPLE_DC_BTN_B);
    if (buttons & MAPLE_XINPUT_X)
        dc_press(&condition, MAPLE_DC_BTN_X);
    if (buttons & MAPLE_XINPUT_Y)
        dc_press(&condition, MAPLE_DC_BTN_Y);
    if (buttons & MAPLE_XINPUT_START)
        dc_press(&condition, MAPLE_DC_BTN_START);

    if (buttons & MAPLE_XINPUT_DPAD_UP)
        dc_press(&condition, MAPLE_DC_BTN_UP);
    if (buttons & MAPLE_XINPUT_DPAD_DOWN)
        dc_press(&condition, MAPLE_DC_BTN_DOWN);
    if (buttons & MAPLE_XINPUT_DPAD_LEFT)
        dc_press(&condition, MAPLE_DC_BTN_LEFT);
    if (buttons & MAPLE_XINPUT_DPAD_RIGHT)
        dc_press(&condition, MAPLE_DC_BTN_RIGHT);

    if (mode == MAPLE_MAP_EXTENDED) {
        if (buttons & MAPLE_XINPUT_LEFT_SHOULDER)
            dc_press(&condition, MAPLE_DC_BTN_Z);
        if (buttons & MAPLE_XINPUT_RIGHT_SHOULDER)
            dc_press(&condition, MAPLE_DC_BTN_C);
        if (buttons & MAPLE_XINPUT_BACK)
            dc_press(&condition, MAPLE_DC_BTN_D);

        if (!stick_inside_deadzone(state->right_x, state->right_y,
                                   MAPLE_XINPUT_RIGHT_THUMB_DEADZONE)) {
            condition.joyx2 = maple_xinput_axis_x_to_dc(state->right_x);
            condition.joyy2 = maple_xinput_axis_y_to_dc(state->right_y);
        }
    } else {
        if (buttons & MAPLE_XINPUT_LEFT_SHOULDER)
            condition.ltrigger = 255;
        if (buttons & MAPLE_XINPUT_RIGHT_SHOULDER)
            condition.rtrigger = 255;
    }

    return condition;
}

static void put_be32(uint8_t *out, uint32_t value) {
    out[0] = (uint8_t)((value >> 24) & 0xFFu);
    out[1] = (uint8_t)((value >> 16) & 0xFFu);
    out[2] = (uint8_t)((value >> 8) & 0xFFu);
    out[3] = (uint8_t)(value & 0xFFu);
}

static uint32_t get_be32(const uint8_t *in) {
    return ((uint32_t)in[0] << 24) | ((uint32_t)in[1] << 16) | ((uint32_t)in[2] << 8) |
           (uint32_t)in[3];
}

static void put_le16(uint8_t *out, uint16_t value) {
    out[0] = (uint8_t)(value & 0xFFu);
    out[1] = (uint8_t)((value >> 8) & 0xFFu);
}

void maple_encode_condition(const maple_controller_condition_t *condition, uint8_t out[8]) {
    put_le16(&out[0], condition->buttons);
    out[2] = condition->rtrigger;
    out[3] = condition->ltrigger;
    out[4] = condition->joyx;
    out[5] = condition->joyy;
    out[6] = condition->joyx2;
    out[7] = condition->joyy2;
}

uint8_t maple_frame_checksum(const uint8_t *frame, size_t frame_len) {
    uint8_t checksum = 0;
    for (size_t i = 0; i < frame_len; i++)
        checksum ^= frame[i];
    return checksum;
}

static void put_fixed_string(uint8_t *out, size_t len, const char *text) {
    memset(out, ' ', len);
    size_t n = strlen(text);
    if (n > len)
        n = len;
    memcpy(out, text, n);
}

static void encode_device_info(uint8_t out[MAPLE_DEVICE_INFO_BYTES], maple_map_mode_t mode) {
    memset(out, 0, MAPLE_DEVICE_INFO_BYTES);
    put_be32(&out[0], MAPLE_FUNCTION_CONTROLLER);
    put_be32(&out[4], maple_controller_capabilities(mode));
    out[16] = 0xFF; // all regions
    out[17] = 0x00; // connector exits upward
    put_fixed_string(&out[18], 30, "CouchLink Maple Pad");
    put_fixed_string(&out[48], 60, "Parsec CouchLink");
    put_le16(&out[108], 0);
    put_le16(&out[110], 0);
}

static bool request_function_is_controller(const maple_request_t *request) {
    if (request->payload_words < 1 || request->payload == NULL)
        return false;
    return get_be32(request->payload) == MAPLE_FUNCTION_CONTROLLER;
}

static size_t begin_response(const maple_request_t *request, uint8_t response,
                             uint8_t payload_words, uint8_t *out, size_t out_cap) {
    size_t frame_len = 4u + ((size_t)payload_words * 4u);
    size_t packet_len = frame_len + 1u;
    if (out_cap < packet_len)
        return 0;

    out[0] = response;
    out[1] = request->sender_addr;
    out[2] = request->recipient_addr;
    out[3] = payload_words;
    return packet_len;
}

static size_t finish_response(uint8_t *out, size_t packet_len) {
    size_t frame_len = packet_len - 1u;
    out[frame_len] = maple_frame_checksum(out, frame_len);
    return packet_len;
}

static size_t build_empty_response(const maple_request_t *request, uint8_t response, uint8_t *out,
                                   size_t out_cap) {
    size_t packet_len = begin_response(request, response, 0, out, out_cap);
    if (packet_len == 0)
        return 0;
    return finish_response(out, packet_len);
}

static size_t build_device_info_response(const maple_request_t *request, uint8_t response,
                                         maple_map_mode_t mode, uint8_t *out, size_t out_cap) {
    size_t packet_len =
        begin_response(request, response, MAPLE_DEVICE_INFO_BYTES / 4u, out, out_cap);
    if (packet_len == 0)
        return 0;
    encode_device_info(&out[4], mode);
    return finish_response(out, packet_len);
}

static size_t build_condition_response(const maple_request_t *request, const gamepad_state_t *state,
                                       maple_map_mode_t mode, uint8_t *out, size_t out_cap) {
    if (!request_function_is_controller(request))
        return build_empty_response(request, MAPLE_RESP_BAD_FUNC, out, out_cap);

    size_t packet_len = begin_response(request, MAPLE_RESP_DATA_TRANSFER, 3, out, out_cap);
    if (packet_len == 0)
        return 0;

    put_be32(&out[4], MAPLE_FUNCTION_CONTROLLER);
    maple_controller_condition_t condition = maple_translate_xinput(state, mode);
    maple_encode_condition(&condition, &out[8]);
    return finish_response(out, packet_len);
}

size_t maple_build_response(const maple_request_t *request, const gamepad_state_t *state,
                            maple_map_mode_t mode, uint8_t *out, size_t out_cap) {
    switch (request->command) {
    case MAPLE_CMD_DEVICE_INFO:
        return build_device_info_response(request, MAPLE_RESP_DEVICE_INFO, mode, out, out_cap);
    case MAPLE_CMD_EXT_DEVICE_INFO:
        return build_device_info_response(request, MAPLE_RESP_EXT_DEVICE_INFO, mode, out, out_cap);
    case MAPLE_CMD_RESET:
    case MAPLE_CMD_SHUTDOWN:
        return build_empty_response(request, MAPLE_RESP_ACK, out, out_cap);
    case MAPLE_CMD_GET_CONDITION:
        return build_condition_response(request, state, mode, out, out_cap);
    case MAPLE_CMD_SET_CONDITION:
        return build_empty_response(request, MAPLE_RESP_BAD_FUNC, out, out_cap);
    default:
        return build_empty_response(request, MAPLE_RESP_BAD_CMD, out, out_cap);
    }
}
