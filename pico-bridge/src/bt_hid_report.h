#pragma once

#include <stdint.h>

#include "gamepad_state.h"

typedef enum {
    BT_HID_TARGET_GENERIC = 0,
    BT_HID_TARGET_XBOX = 1,
    BT_HID_TARGET_PLAYSTATION = 2,
} bt_hid_target_t;

#define BT_HID_REPORT_ID 0x01u
#define BT_HID_BUTTON_COUNT 20u
#define BT_HID_PAYLOAD_REPORT_LEN 9u
#define BT_HID_WIRE_REPORT_LEN (1u + BT_HID_PAYLOAD_REPORT_LEN)
#define BT_HID_INTERRUPT_REPORT_LEN (1u + BT_HID_WIRE_REPORT_LEN)

typedef struct {
    uint8_t bytes[BT_HID_WIRE_REPORT_LEN];
    uint8_t len;
    uint8_t report_id;
} bt_hid_report_t;

const uint8_t *bt_hid_descriptor(bt_hid_target_t target, uint16_t *len);
const char *bt_hid_target_label(bt_hid_target_t target);
const char *bt_hid_local_name(bt_hid_target_t target);
const char *bt_hid_service_name(bt_hid_target_t target);
uint16_t bt_hid_product_id(bt_hid_target_t target);
void bt_hid_build_report(bt_hid_target_t target, const gamepad_state_t *state,
                         bt_hid_report_t *out);
