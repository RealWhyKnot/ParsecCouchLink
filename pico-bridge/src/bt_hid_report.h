#pragma once

#include <stdbool.h>
#include <stdint.h>

#include "gamepad_state.h"

typedef enum {
    BT_HID_TARGET_GENERIC = 0,
    BT_HID_TARGET_XBOX = 1,
    BT_HID_TARGET_PLAYSTATION = 2,
} bt_hid_target_t;

#define BT_HID_GENERIC_REPORT_ID 0x01u
#define BT_HID_XBOX_REPORT_ID 0x01u
#define BT_HID_XBOX_OUTPUT_REPORT_ID 0x03u
#define BT_HID_PS4_REPORT_ID 0x11u
#define BT_HID_PS4_FEATURE_PAIRING_REPORT_ID 0x02u
#define BT_HID_PS4_FEATURE_BUILD_DATE_REPORT_ID 0xA3u
#define BT_HID_PS4_FEATURE_AUTH_STATUS_REPORT_ID 0xF2u
#define BT_HID_BUTTON_COUNT 20u
#define BT_HID_REPORT_TYPE_INPUT 1u
#define BT_HID_REPORT_TYPE_OUTPUT 2u
#define BT_HID_REPORT_TYPE_FEATURE 3u
#define BT_HID_GENERIC_PAYLOAD_REPORT_LEN 9u
#define BT_HID_GENERIC_WIRE_REPORT_LEN (1u + BT_HID_GENERIC_PAYLOAD_REPORT_LEN)
#define BT_HID_XBOX_PAYLOAD_REPORT_LEN 16u
#define BT_HID_XBOX_WIRE_REPORT_LEN (1u + BT_HID_XBOX_PAYLOAD_REPORT_LEN)
#define BT_HID_XBOX_OUTPUT_PAYLOAD_REPORT_LEN 8u
#define BT_HID_PS4_PAYLOAD_REPORT_LEN 77u
#define BT_HID_PS4_WIRE_REPORT_LEN (1u + BT_HID_PS4_PAYLOAD_REPORT_LEN)
#define BT_HID_PS4_FEATURE_PAIRING_PAYLOAD_REPORT_LEN 36u
#define BT_HID_PS4_FEATURE_BUILD_DATE_PAYLOAD_REPORT_LEN 48u
#define BT_HID_PS4_FEATURE_AUTH_STATUS_PAYLOAD_REPORT_LEN 15u
#define BT_HID_MAX_WIRE_REPORT_LEN BT_HID_PS4_WIRE_REPORT_LEN
#define BT_HID_INTERRUPT_REPORT_LEN (1u + BT_HID_MAX_WIRE_REPORT_LEN)

#define BT_HID_REPORT_ID BT_HID_GENERIC_REPORT_ID
#define BT_HID_PAYLOAD_REPORT_LEN BT_HID_GENERIC_PAYLOAD_REPORT_LEN
#define BT_HID_WIRE_REPORT_LEN BT_HID_GENERIC_WIRE_REPORT_LEN

typedef struct {
    uint8_t bytes[BT_HID_MAX_WIRE_REPORT_LEN];
    uint8_t len;
    uint8_t report_id;
} bt_hid_report_t;

const uint8_t *bt_hid_descriptor(bt_hid_target_t target, uint16_t *len);
const char *bt_hid_target_label(bt_hid_target_t target);
const char *bt_hid_local_name(bt_hid_target_t target);
const char *bt_hid_service_name(bt_hid_target_t target);
uint16_t bt_hid_vendor_id(bt_hid_target_t target);
uint16_t bt_hid_product_id(bt_hid_target_t target);
uint16_t bt_hid_bcd_version(bt_hid_target_t target);
void bt_hid_build_report(bt_hid_target_t target, const gamepad_state_t *state,
                         bt_hid_report_t *out);
uint16_t bt_hid_get_report_payload(bt_hid_target_t target, uint8_t report_type, uint8_t report_id,
                                   const gamepad_state_t *state, uint8_t *buffer, uint16_t reqlen);
bool bt_hid_accept_set_report(bt_hid_target_t target, uint8_t report_type, const uint8_t *report,
                              uint16_t report_size, uint8_t *report_id, uint16_t *payload_len);
bool bt_hid_accept_output_payload(bt_hid_target_t target, uint8_t report_type, uint8_t report_id,
                                  const uint8_t *payload, uint16_t payload_len);
