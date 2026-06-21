#pragma once

#include <stdbool.h>
#include <stdint.h>

#include "boot_mode_policy.h"
#include "bt_hid_report.h"

#define BT_HID_STATUS_STARTED 0x01u
#define BT_HID_STATUS_CONNECTED 0x02u
#define BT_HID_STATUS_SEND_REQUESTED 0x04u

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
} bt_hid_snapshot_t;

bool bt_hid_target_from_persona(run_persona_t persona, bt_hid_target_t *out);
bool bt_hid_init(bt_hid_target_t target);
void bt_hid_reset_stack_state(void);
void bt_hid_snapshot(bt_hid_snapshot_t *out);
void bt_hid_task(void);
