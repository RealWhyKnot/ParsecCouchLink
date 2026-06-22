#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#include "diag_log.h"

// Mirrors bridge/src/cdc.rs and wiki/Protocol.md v1. Frame format:
//
//   magic(2)=A5 5A | proto_version(1) | command(1) | payload_len(2 LE)
//   | seq(1) | reserved(1) | payload(N) | crc16(2 LE)
//
// CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no reflect, no xor-out.

#define CDC_FRAME_MAGIC0 0xA5
#define CDC_FRAME_MAGIC1 0x5A
#define CDC_PROTO_VERSION 1
#define CDC_MAX_PAYLOAD (4u + DIAG_LOG_RING_SIZE)
#define CDC_HEADER_LEN 8
#define CDC_CRC_LEN 2
#define CDC_MAX_FRAME (CDC_HEADER_LEN + CDC_MAX_PAYLOAD + CDC_CRC_LEN)

// Request opcodes (>= 0x01, <= 0x7F by convention).
#define CDC_CMD_HELLO 0x01
#define CDC_CMD_GET_STATUS 0x02
#define CDC_CMD_SET_WIFI 0x03
#define CDC_CMD_REBOOT_TO_RUN 0x05
#define CDC_CMD_SELF_TEST 0x06
#define CDC_CMD_GET_DEVICE_NAME 0x07
#define CDC_CMD_SET_DEVICE_NAME 0x08
#define CDC_CMD_GET_UNIQUE_ID 0x09
#define CDC_CMD_GET_LOG_BUFFER 0x0A
#define CDC_CMD_REBOOT_TO_BOOTSEL 0x0B
#define CDC_CMD_BT_STATE 0x0C
#define CDC_CMD_BT_HEARTBEAT 0x0D
#define CDC_CMD_BT_GET_STATUS 0x0E

// Response opcodes (high bit set).
#define CDC_RSP_HELLO 0x81
#define CDC_RSP_STATUS 0x82
#define CDC_RSP_SET_WIFI 0x83
#define CDC_RSP_REBOOT 0x85
#define CDC_RSP_SELF_TEST 0x86
#define CDC_RSP_DEVICE_NAME 0x87
#define CDC_RSP_SET_DEVICE_NAME 0x88
#define CDC_RSP_UNIQUE_ID 0x89
#define CDC_RSP_LOG_BUFFER 0x8A
#define CDC_RSP_REBOOT_TO_BOOTSEL 0x8B
#define CDC_RSP_BT_STATE 0x8C
#define CDC_RSP_BT_HEARTBEAT 0x8D
#define CDC_RSP_BT_STATUS 0x8E
#define CDC_RSP_NACK 0xFE

// Error codes carried in a NACK payload (first byte; second byte is detail).
#define CDC_ERR_BAD_CRC 0x01
#define CDC_ERR_BAD_VERSION 0x02
#define CDC_ERR_UNKNOWN_COMMAND 0x03
#define CDC_ERR_BAD_LENGTH 0x04
#define CDC_ERR_FLASH_WRITE_FAIL 0x05
#define CDC_ERR_FLASH_VERIFY_FAIL 0x06
#define CDC_ERR_WIFI_JOIN_TIMEOUT 0x10
#define CDC_ERR_AUTH_FAIL 0x11
#define CDC_ERR_NO_2G_NETWORK 0x12
#define CDC_ERR_INTERNAL 0xFF

// HELLO_ACK flags (byte 5 of the HELLO_ACK payload).
#define CDC_HELLO_FLAG_CREDS_PRESENT 0x01
#define CDC_HELLO_FLAG_WIFI_JOINED 0x02
#define CDC_HELLO_FLAG_RUN_MODE_OK 0x04
#define CDC_HELLO_FLAG_RUN_MODE_ACTIVE 0x08

#define CDC_BT_STATUS_VERSION 2
#define CDC_BT_STATUS_V1_VERSION 1
#define CDC_BT_STATUS_V1_FIXED_LEN 49
#define CDC_BT_STATUS_FIXED_LEN 99
#define CDC_BT_STATUS_MAX_NAME 64

typedef struct {
    uint8_t flags;
    uint8_t target;
    uint8_t last_status;
    uint8_t report_len;
    uint16_t cid;
    uint32_t init_count;
    uint32_t ready_count;
    uint32_t open_count;
    uint32_t close_count;
    uint32_t can_send_count;
    uint32_t report_build_count;
    uint32_t report_send_count;
    uint32_t send_request_count;
    uint32_t last_event_ms;
    uint32_t last_send_ms;
    uint32_t get_report_count;
    uint32_t get_report_success_count;
    uint32_t get_report_unsupported_count;
    uint32_t set_report_count;
    uint32_t set_report_accepted_count;
    uint32_t set_report_unsupported_count;
    uint32_t out_report_count;
    uint32_t out_report_accepted_count;
    uint32_t out_report_unsupported_count;
    uint8_t last_get_report_id;
    uint8_t last_get_report_type;
    uint8_t last_set_report_id;
    uint8_t last_set_report_type;
    uint8_t last_out_report_id;
    uint8_t last_out_report_type;
    uint16_t last_get_report_len;
    uint16_t last_set_report_len;
    uint16_t last_out_report_len;
    const char *local_name;
} cdc_bt_status_view_t;

uint16_t cdc_crc16(const uint8_t *data, size_t n);

// Build the BT_STATUS payload body. Returns payload bytes written, or 0
// when `out_cap` is too small.
size_t cdc_build_bt_status_payload(const cdc_bt_status_view_t *status, uint8_t *out,
                                   size_t out_cap);

// Build a response or NACK frame into `out`. Returns total bytes written
// (always HEADER + payload_len + CRC). `out` must be at least
// CDC_MAX_FRAME bytes.
size_t cdc_encode(uint8_t command, uint8_t seq, const uint8_t *payload, size_t payload_len,
                  uint8_t *out, size_t out_cap);

// Decoded frame view; pointers are into the original buffer.
typedef struct {
    uint8_t command;
    uint8_t seq;
    const uint8_t *payload;
    size_t payload_len;
} cdc_frame_view_t;

typedef enum {
    CDC_DECODE_OK = 0,
    CDC_DECODE_NEED_MORE, // not enough bytes yet, keep accumulating
    CDC_DECODE_BAD_MAGIC,
    CDC_DECODE_BAD_VERSION,
    CDC_DECODE_BAD_LENGTH,
    CDC_DECODE_BAD_CRC,
} cdc_decode_status_t;

// Try to decode a complete frame from the head of `buf`. On success,
// `*consumed` is set to how many bytes to drop from the front of the
// buffer. On NEED_MORE, `*consumed` is 0 (don't drop; keep reading).
cdc_decode_status_t cdc_try_decode(const uint8_t *buf, size_t buf_len, cdc_frame_view_t *out,
                                   size_t *consumed);

// On any non-NEED_MORE error, callers should skip past the first byte to
// resync rather than dropping the whole buffer.
