// Host-compiled unit tests for the setup-mode CDC framing in cdc_proto.c.
// That file is pure C (no Pico SDK), so it builds and runs on the dev/CI
// host. This is the C-side counterpart to the framing tests in
// bridge/src/cdc.rs -- both sides must agree on the wire format.
//
//   cc test_cdc_proto.c ../src/cdc_proto.c -I../src -o test_cdc_proto
//   ./test_cdc_proto

#include <stdio.h>
#include <string.h>

#include "cdc_proto.h"

static int failures = 0;

#define CHECK(cond)                                                                                \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);                                 \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

// CRC-16/CCITT-FALSE canonical check value: crc("123456789") == 0x29B1.
static void test_crc16_check_vector(void) {
    const uint8_t data[] = {'1', '2', '3', '4', '5', '6', '7', '8', '9'};
    CHECK(cdc_crc16(data, sizeof(data)) == 0x29B1);
}

static void roundtrip(uint8_t cmd, uint8_t seq, size_t len) {
    uint8_t payload[CDC_MAX_PAYLOAD];
    for (size_t i = 0; i < len; i++)
        payload[i] = (uint8_t)(i * 7 + 1);

    uint8_t frame[CDC_MAX_FRAME];
    size_t total = cdc_encode(cmd, seq, len ? payload : NULL, len, frame, sizeof(frame));
    CHECK(total == CDC_HEADER_LEN + len + CDC_CRC_LEN);

    cdc_frame_view_t view;
    size_t consumed = 0;
    cdc_decode_status_t st = cdc_try_decode(frame, total, &view, &consumed);
    CHECK(st == CDC_DECODE_OK);
    CHECK(consumed == total);
    CHECK(view.command == cmd);
    CHECK(view.seq == seq);
    CHECK(view.payload_len == len);
    if (len)
        CHECK(memcmp(view.payload, payload, len) == 0);
}

static void test_roundtrips(void) {
    roundtrip(0x01, 0x00, 0);
    roundtrip(0x03, 0x2A, 1);
    roundtrip(0x8A, 0xFF, CDC_MAX_PAYLOAD);
}

static void test_need_more(void) {
    uint8_t frame[CDC_MAX_FRAME];
    size_t total = cdc_encode(0x01, 0, NULL, 0, frame, sizeof(frame));
    cdc_frame_view_t view;
    size_t consumed = 99;
    // Fewer bytes than a bare header+crc -> NEED_MORE, consumed cleared.
    CHECK(cdc_try_decode(frame, 5, &view, &consumed) == CDC_DECODE_NEED_MORE);
    CHECK(consumed == 0);
    // A complete frame minus its last byte -> NEED_MORE.
    CHECK(cdc_try_decode(frame, total - 1, &view, &consumed) == CDC_DECODE_NEED_MORE);
}

static void test_bad_magic(void) {
    uint8_t frame[CDC_MAX_FRAME];
    size_t total = cdc_encode(0x01, 0, NULL, 0, frame, sizeof(frame));
    frame[0] ^= 0xFF;
    cdc_frame_view_t view;
    size_t consumed;
    CHECK(cdc_try_decode(frame, total, &view, &consumed) == CDC_DECODE_BAD_MAGIC);
}

static void test_bad_version(void) {
    uint8_t frame[CDC_MAX_FRAME];
    size_t total = cdc_encode(0x01, 0, NULL, 0, frame, sizeof(frame));
    frame[2] = CDC_PROTO_VERSION + 1; // version is checked before the CRC
    cdc_frame_view_t view;
    size_t consumed;
    CHECK(cdc_try_decode(frame, total, &view, &consumed) == CDC_DECODE_BAD_VERSION);
}

static void test_bad_length(void) {
    // Header claims a payload longer than the max, with just enough bytes
    // present to clear the NEED_MORE gate.
    uint8_t frame[CDC_HEADER_LEN + CDC_CRC_LEN] = {0};
    frame[0] = CDC_FRAME_MAGIC0;
    frame[1] = CDC_FRAME_MAGIC1;
    frame[2] = CDC_PROTO_VERSION;
    frame[4] = (uint8_t)((CDC_MAX_PAYLOAD + 1) & 0xFF);
    frame[5] = (uint8_t)(((CDC_MAX_PAYLOAD + 1) >> 8) & 0xFF);
    cdc_frame_view_t view;
    size_t consumed;
    CHECK(cdc_try_decode(frame, sizeof(frame), &view, &consumed) == CDC_DECODE_BAD_LENGTH);
}

static void test_bad_crc(void) {
    uint8_t payload[4] = {1, 2, 3, 4};
    uint8_t frame[CDC_MAX_FRAME];
    size_t total = cdc_encode(0x03, 7, payload, sizeof(payload), frame, sizeof(frame));
    frame[total - 1] ^= 0xFF; // corrupt the trailing CRC byte
    cdc_frame_view_t view;
    size_t consumed;
    CHECK(cdc_try_decode(frame, total, &view, &consumed) == CDC_DECODE_BAD_CRC);
}

static void test_encode_rejects_oversize(void) {
    uint8_t payload[CDC_MAX_PAYLOAD + 1] = {0};
    uint8_t frame[CDC_MAX_FRAME + 8];
    // Payload larger than the max -> refuse.
    CHECK(cdc_encode(0x01, 0, payload, CDC_MAX_PAYLOAD + 1, frame, sizeof(frame)) == 0);
    // Output buffer too small for even an empty frame -> refuse.
    uint8_t small[4];
    CHECK(cdc_encode(0x01, 0, NULL, 0, small, sizeof(small)) == 0);
}

static uint16_t get_u16_le(const uint8_t *buf) {
    return (uint16_t)buf[0] | ((uint16_t)buf[1] << 8);
}

static uint32_t get_u32_le(const uint8_t *buf) {
    return (uint32_t)buf[0] | ((uint32_t)buf[1] << 8) | ((uint32_t)buf[2] << 16) |
           ((uint32_t)buf[3] << 24);
}

static void test_bt_status_payload_shape(void) {
    cdc_bt_status_view_t status = {
        .flags = 0x03,
        .target = 2,
        .last_status = 0x44,
        .report_len = 10,
        .cid = 0x1234,
        .init_count = 1,
        .ready_count = 2,
        .open_count = 3,
        .close_count = 4,
        .can_send_count = 5,
        .report_build_count = 6,
        .report_send_count = 7,
        .send_request_count = 8,
        .last_event_ms = 0x11223344,
        .last_send_ms = 0x55667788,
        .get_report_count = 9,
        .get_report_success_count = 10,
        .get_report_unsupported_count = 11,
        .set_report_count = 12,
        .set_report_accepted_count = 13,
        .set_report_unsupported_count = 14,
        .out_report_count = 15,
        .out_report_accepted_count = 16,
        .out_report_unsupported_count = 17,
        .last_get_report_id = 0x02,
        .last_get_report_type = 3,
        .last_set_report_id = 0x11,
        .last_set_report_type = 2,
        .last_out_report_id = 0x03,
        .last_out_report_type = 2,
        .last_get_report_len = 36,
        .last_set_report_len = 77,
        .last_out_report_len = 8,
        .pin_code_request_count = 18,
        .pin_code_response_count = 19,
        .user_confirmation_request_count = 20,
        .user_confirmation_response_count = 21,
        .simple_pairing_complete_count = 22,
        .authentication_complete_count = 23,
        .link_key_notification_count = 24,
        .encryption_change_count = 25,
        .disconnection_complete_count = 26,
        .hid_open_failed_count = 27,
        .last_security_event_ms = 0x99AABBCC,
        .last_simple_pairing_status = 0x31,
        .last_authentication_status = 0x32,
        .last_encryption_status = 0x33,
        .last_encryption_enabled = 1,
        .last_disconnection_reason = 0x13,
        .last_hid_open_status = 0x44,
        .local_name = "CouchLink BT HID",
    };
    uint8_t payload[CDC_BT_STATUS_FIXED_LEN + CDC_BT_STATUS_MAX_NAME];
    size_t n = cdc_build_bt_status_payload(&status, payload, sizeof(payload));

    CHECK(n == CDC_BT_STATUS_FIXED_LEN + strlen(status.local_name));
    CHECK(payload[0] == CDC_BT_STATUS_VERSION);
    CHECK(payload[1] == status.flags);
    CHECK(payload[2] == status.target);
    CHECK(payload[3] == status.last_status);
    CHECK(payload[4] == status.report_len);
    CHECK(payload[5] == 0);
    CHECK(get_u16_le(&payload[6]) == status.cid);
    CHECK(get_u32_le(&payload[8]) == status.init_count);
    CHECK(get_u32_le(&payload[12]) == status.ready_count);
    CHECK(get_u32_le(&payload[16]) == status.open_count);
    CHECK(get_u32_le(&payload[20]) == status.close_count);
    CHECK(get_u32_le(&payload[24]) == status.can_send_count);
    CHECK(get_u32_le(&payload[28]) == status.report_build_count);
    CHECK(get_u32_le(&payload[32]) == status.report_send_count);
    CHECK(get_u32_le(&payload[36]) == status.send_request_count);
    CHECK(get_u32_le(&payload[40]) == status.last_event_ms);
    CHECK(get_u32_le(&payload[44]) == status.last_send_ms);
    CHECK(get_u32_le(&payload[48]) == status.get_report_count);
    CHECK(get_u32_le(&payload[52]) == status.get_report_success_count);
    CHECK(get_u32_le(&payload[56]) == status.get_report_unsupported_count);
    CHECK(get_u32_le(&payload[60]) == status.set_report_count);
    CHECK(get_u32_le(&payload[64]) == status.set_report_accepted_count);
    CHECK(get_u32_le(&payload[68]) == status.set_report_unsupported_count);
    CHECK(get_u32_le(&payload[72]) == status.out_report_count);
    CHECK(get_u32_le(&payload[76]) == status.out_report_accepted_count);
    CHECK(get_u32_le(&payload[80]) == status.out_report_unsupported_count);
    CHECK(payload[84] == status.last_get_report_id);
    CHECK(payload[85] == status.last_get_report_type);
    CHECK(payload[86] == status.last_set_report_id);
    CHECK(payload[87] == status.last_set_report_type);
    CHECK(payload[88] == status.last_out_report_id);
    CHECK(payload[89] == status.last_out_report_type);
    CHECK(get_u16_le(&payload[92]) == status.last_get_report_len);
    CHECK(get_u16_le(&payload[94]) == status.last_set_report_len);
    CHECK(get_u16_le(&payload[96]) == status.last_out_report_len);
    CHECK(get_u32_le(&payload[98]) == status.pin_code_request_count);
    CHECK(get_u32_le(&payload[102]) == status.pin_code_response_count);
    CHECK(get_u32_le(&payload[106]) == status.user_confirmation_request_count);
    CHECK(get_u32_le(&payload[110]) == status.user_confirmation_response_count);
    CHECK(get_u32_le(&payload[114]) == status.simple_pairing_complete_count);
    CHECK(get_u32_le(&payload[118]) == status.authentication_complete_count);
    CHECK(get_u32_le(&payload[122]) == status.link_key_notification_count);
    CHECK(get_u32_le(&payload[126]) == status.encryption_change_count);
    CHECK(get_u32_le(&payload[130]) == status.disconnection_complete_count);
    CHECK(get_u32_le(&payload[134]) == status.hid_open_failed_count);
    CHECK(get_u32_le(&payload[138]) == status.last_security_event_ms);
    CHECK(payload[142] == status.last_simple_pairing_status);
    CHECK(payload[143] == status.last_authentication_status);
    CHECK(payload[144] == status.last_encryption_status);
    CHECK(payload[145] == status.last_encryption_enabled);
    CHECK(payload[146] == status.last_disconnection_reason);
    CHECK(payload[147] == status.last_hid_open_status);
    CHECK(payload[148] == 0);
    CHECK(payload[149] == 0);
    CHECK(payload[150] == 0);
    CHECK(payload[151] == 0);
    CHECK(payload[152] == strlen(status.local_name));
    CHECK(memcmp(&payload[153], status.local_name, strlen(status.local_name)) == 0);
}

static void test_bt_status_version_lengths(void) {
    CHECK(CDC_BT_STATUS_VERSION == 3);
    CHECK(CDC_BT_STATUS_V1_FIXED_LEN == 49);
    CHECK(CDC_BT_STATUS_V2_VERSION == 2);
    CHECK(CDC_BT_STATUS_V2_FIXED_LEN == 99);
    CHECK(CDC_BT_STATUS_FIXED_LEN == 153);
}

int main(void) {
    test_crc16_check_vector();
    test_roundtrips();
    test_need_more();
    test_bad_magic();
    test_bad_version();
    test_bad_length();
    test_bad_crc();
    test_encode_rejects_oversize();
    test_bt_status_payload_shape();
    test_bt_status_version_lengths();

    if (failures == 0) {
        printf("OK: all cdc_proto tests passed\n");
        return 0;
    }
    printf("FAILED: %d check(s)\n", failures);
    return 1;
}
