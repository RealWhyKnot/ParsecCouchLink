#include <stdio.h>
#include <string.h>

#include "maple_proto.h"

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

static maple_request_t request(uint8_t command, const uint8_t *payload, uint8_t payload_words) {
    maple_request_t req;
    req.command = command;
    req.recipient_addr = MAPLE_ADDR_PORT_A_MAIN;
    req.sender_addr = MAPLE_ADDR_PORT_A_HOST;
    req.payload = payload;
    req.payload_words = payload_words;
    return req;
}

static void check_packet_checksum(const uint8_t *packet, size_t len) {
    CHECK(len > 0);
    CHECK(packet[len - 1] == maple_frame_checksum(packet, len - 1));
}

static void check_header(const uint8_t *packet, uint8_t code, uint8_t words) {
    CHECK(packet[0] == code);
    CHECK(packet[1] == MAPLE_ADDR_PORT_A_HOST);
    CHECK(packet[2] == MAPLE_ADDR_PORT_A_MAIN);
    CHECK(packet[3] == words);
}

static void test_axis_conversion(void) {
    CHECK(maple_xinput_axis_x_to_dc(-32768) == 0);
    CHECK(maple_xinput_axis_x_to_dc(0) == 128);
    CHECK(maple_xinput_axis_x_to_dc(32767) == 255);

    CHECK(maple_xinput_axis_y_to_dc(32767) == 0);
    CHECK(maple_xinput_axis_y_to_dc(0) == 128);
    CHECK(maple_xinput_axis_y_to_dc(-32768) == 255);
}

static void test_neutral_translation(void) {
    gamepad_state_t state = neutral_state();
    maple_controller_condition_t dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.buttons == 0xFFFFu);
    CHECK(dc.ltrigger == 0);
    CHECK(dc.rtrigger == 0);
    CHECK(dc.joyx == 128);
    CHECK(dc.joyy == 128);
    CHECK(dc.joyx2 == 128);
    CHECK(dc.joyy2 == 128);
}

static void test_standard_button_translation(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = MAPLE_XINPUT_A | MAPLE_XINPUT_B | MAPLE_XINPUT_X | MAPLE_XINPUT_Y |
                    MAPLE_XINPUT_START | MAPLE_XINPUT_DPAD_UP | MAPLE_XINPUT_DPAD_RIGHT;
    maple_controller_condition_t dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);

    CHECK((dc.buttons & MAPLE_DC_BTN_A) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_B) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_X) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_Y) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_START) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_UP) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_RIGHT) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_DOWN) != 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_LEFT) != 0);
}

static void test_trigger_threshold_and_bumpers(void) {
    gamepad_state_t state = neutral_state();
    state.left_trigger = MAPLE_XINPUT_TRIGGER_THRESHOLD - 1;
    state.right_trigger = MAPLE_XINPUT_TRIGGER_THRESHOLD;
    maple_controller_condition_t dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.ltrigger == 0);
    CHECK(dc.rtrigger == MAPLE_XINPUT_TRIGGER_THRESHOLD);

    state.buttons = MAPLE_XINPUT_LEFT_SHOULDER | MAPLE_XINPUT_RIGHT_SHOULDER;
    dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.ltrigger == 255);
    CHECK(dc.rtrigger == 255);
    CHECK((dc.buttons & MAPLE_DC_BTN_Z) != 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_C) != 0);
}

static void test_stick_deadzones_and_edges(void) {
    gamepad_state_t state = neutral_state();
    state.left_x = 100;
    state.left_y = -100;
    maple_controller_condition_t dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.joyx == 128);
    CHECK(dc.joyy == 128);

    state = neutral_state();
    state.left_x = -32768;
    dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.joyx == 0);

    state = neutral_state();
    state.left_x = 32767;
    dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.joyx == 255);

    state = neutral_state();
    state.left_y = 32767;
    dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.joyy == 0);

    state = neutral_state();
    state.left_y = -32768;
    dc = maple_translate_xinput(&state, MAPLE_MAP_STANDARD);
    CHECK(dc.joyy == 255);
}

static void test_extended_mapping(void) {
    gamepad_state_t state = neutral_state();
    state.buttons = MAPLE_XINPUT_LEFT_SHOULDER | MAPLE_XINPUT_RIGHT_SHOULDER | MAPLE_XINPUT_BACK;
    state.left_trigger = MAPLE_XINPUT_TRIGGER_THRESHOLD - 1;
    state.right_x = 32767;
    state.right_y = -32768;

    maple_controller_condition_t dc = maple_translate_xinput(&state, MAPLE_MAP_EXTENDED);
    CHECK((dc.buttons & MAPLE_DC_BTN_Z) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_C) == 0);
    CHECK((dc.buttons & MAPLE_DC_BTN_D) == 0);
    CHECK(dc.ltrigger == 0);
    CHECK(dc.joyx2 == 255);
    CHECK(dc.joyy2 == 255);
}

static void test_condition_encoding(void) {
    maple_controller_condition_t condition;
    condition.buttons = 0xFFFBu;
    condition.rtrigger = 31;
    condition.ltrigger = 30;
    condition.joyx = 1;
    condition.joyy = 2;
    condition.joyx2 = 3;
    condition.joyy2 = 4;

    uint8_t encoded[MAPLE_CONDITION_BYTES];
    maple_encode_condition(&condition, encoded);
    const uint8_t expected[MAPLE_CONDITION_BYTES] = {0xFB, 0xFF, 31, 30, 1, 2, 3, 4};
    CHECK(memcmp(encoded, expected, sizeof(expected)) == 0);
}

static void test_device_info_response(void) {
    gamepad_state_t state = neutral_state();
    uint8_t packet[MAPLE_MAX_RESPONSE_BYTES];
    maple_request_t req = request(MAPLE_CMD_DEVICE_INFO, NULL, 0);

    size_t len = maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet));
    CHECK(len == 4 + MAPLE_DEVICE_INFO_BYTES + 1);
    check_header(packet, MAPLE_RESP_DEVICE_INFO, MAPLE_DEVICE_INFO_BYTES / 4);
    CHECK(memcmp(&packet[4], "\x00\x00\x00\x01", 4) == 0);
    CHECK(memcmp(&packet[8], "\x00\x0F\x06\xFE", 4) == 0);
    CHECK(packet[22] == 'C');
    CHECK(packet[52] == 'P');
    check_packet_checksum(packet, len);
}

static void test_extended_device_info_response(void) {
    gamepad_state_t state = neutral_state();
    uint8_t packet[MAPLE_MAX_RESPONSE_BYTES];
    maple_request_t req = request(MAPLE_CMD_EXT_DEVICE_INFO, NULL, 0);

    size_t len = maple_build_response(&req, &state, MAPLE_MAP_EXTENDED, packet, sizeof(packet));
    CHECK(len == 4 + MAPLE_DEVICE_INFO_BYTES + 1);
    check_header(packet, MAPLE_RESP_EXT_DEVICE_INFO, MAPLE_DEVICE_INFO_BYTES / 4);
    CHECK(memcmp(&packet[8], "\x00\x3F\x0F\xFF", 4) == 0);
    check_packet_checksum(packet, len);
}

static void test_get_condition_response(void) {
    const uint8_t controller_func[4] = {0x00, 0x00, 0x00, 0x01};
    gamepad_state_t state = neutral_state();
    state.buttons = MAPLE_XINPUT_A;
    state.left_trigger = 30;
    state.right_trigger = 31;

    uint8_t packet[MAPLE_MAX_RESPONSE_BYTES];
    maple_request_t req = request(MAPLE_CMD_GET_CONDITION, controller_func, 1);
    size_t len = maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet));

    CHECK(len == 17);
    check_header(packet, MAPLE_RESP_DATA_TRANSFER, 3);
    CHECK(memcmp(&packet[4], controller_func, sizeof(controller_func)) == 0);

    const uint8_t expected_condition[8] = {0xFB, 0xFF, 31, 30, 128, 128, 128, 128};
    CHECK(memcmp(&packet[8], expected_condition, sizeof(expected_condition)) == 0);
    check_packet_checksum(packet, len);
}

static void test_error_and_ack_responses(void) {
    const uint8_t controller_func[4] = {0x00, 0x00, 0x00, 0x01};
    const uint8_t keyboard_func[4] = {0x00, 0x00, 0x00, 0x40};
    gamepad_state_t state = neutral_state();
    uint8_t packet[MAPLE_MAX_RESPONSE_BYTES];

    maple_request_t req = request(MAPLE_CMD_RESET, NULL, 0);
    size_t len = maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet));
    CHECK(len == 5);
    check_header(packet, MAPLE_RESP_ACK, 0);
    check_packet_checksum(packet, len);

    req = request(MAPLE_CMD_GET_CONDITION, keyboard_func, 1);
    len = maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet));
    CHECK(len == 5);
    check_header(packet, MAPLE_RESP_BAD_FUNC, 0);
    check_packet_checksum(packet, len);

    req = request(MAPLE_CMD_SET_CONDITION, controller_func, 1);
    len = maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet));
    CHECK(len == 5);
    check_header(packet, MAPLE_RESP_BAD_FUNC, 0);
    check_packet_checksum(packet, len);

    req = request(0x55, NULL, 0);
    len = maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet));
    CHECK(len == 5);
    check_header(packet, MAPLE_RESP_BAD_CMD, 0);
    check_packet_checksum(packet, len);
}

static void test_rejects_small_output_buffer(void) {
    gamepad_state_t state = neutral_state();
    uint8_t packet[4];
    maple_request_t req = request(MAPLE_CMD_DEVICE_INFO, NULL, 0);
    CHECK(maple_build_response(&req, &state, MAPLE_MAP_STANDARD, packet, sizeof(packet)) == 0);
}

int main(void) {
    test_axis_conversion();
    test_neutral_translation();
    test_standard_button_translation();
    test_trigger_threshold_and_bumpers();
    test_stick_deadzones_and_edges();
    test_extended_mapping();
    test_condition_encoding();
    test_device_info_response();
    test_extended_device_info_response();
    test_get_condition_response();
    test_error_and_ack_responses();
    test_rejects_small_output_buffer();

    if (failures == 0) {
        printf("OK: all maple_proto tests passed\n");
        return 0;
    }
    printf("FAILED: %d check(s)\n", failures);
    return 1;
}
