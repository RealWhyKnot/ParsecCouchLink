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

#define CHECK(cond)                                                  \
    do {                                                             \
        if (!(cond)) {                                               \
            printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);   \
            failures++;                                              \
        }                                                            \
    } while (0)

// CRC-16/CCITT-FALSE canonical check value: crc("123456789") == 0x29B1.
static void test_crc16_check_vector(void) {
    const uint8_t data[] = {'1', '2', '3', '4', '5', '6', '7', '8', '9'};
    CHECK(cdc_crc16(data, sizeof(data)) == 0x29B1);
}

static void roundtrip(uint8_t cmd, uint8_t seq, size_t len) {
    uint8_t payload[CDC_MAX_PAYLOAD];
    for (size_t i = 0; i < len; i++) payload[i] = (uint8_t)(i * 7 + 1);

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
    if (len) CHECK(memcmp(view.payload, payload, len) == 0);
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
    frame[2] = CDC_PROTO_VERSION + 1;  // version is checked before the CRC
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
    frame[total - 1] ^= 0xFF;  // corrupt the trailing CRC byte
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

int main(void) {
    test_crc16_check_vector();
    test_roundtrips();
    test_need_more();
    test_bad_magic();
    test_bad_version();
    test_bad_length();
    test_bad_crc();
    test_encode_rejects_oversize();

    if (failures == 0) {
        printf("OK: all cdc_proto tests passed\n");
        return 0;
    }
    printf("FAILED: %d check(s)\n", failures);
    return 1;
}
