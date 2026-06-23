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
    uint32_t pin_code_request_count;
    uint32_t pin_code_response_count;
    uint32_t user_confirmation_request_count;
    uint32_t user_confirmation_response_count;
    uint32_t simple_pairing_complete_count;
    uint32_t authentication_complete_count;
    uint32_t link_key_notification_count;
    uint32_t encryption_change_count;
    uint32_t disconnection_complete_count;
    uint32_t hid_open_failed_count;
    uint32_t last_security_event_ms;
    uint8_t last_simple_pairing_status;
    uint8_t last_authentication_status;
    uint8_t last_encryption_status;
    uint8_t last_encryption_enabled;
    uint8_t last_disconnection_reason;
    uint8_t last_hid_open_status;
    uint8_t reconnect_state;
    uint8_t reconnect_cycle_attempts;
    uint8_t last_reconnect_status;
    uint8_t last_reconnect_reason;
    uint32_t reconnect_schedule_count;
    uint32_t reconnect_attempt_count;
    uint32_t reconnect_success_count;
    uint32_t reconnect_failed_count;
    uint32_t reconnect_blocked_count;
    uint32_t last_reconnect_ms;
    uint32_t connection_complete_count;
    uint8_t last_connection_complete_status;
    uint8_t last_connection_complete_link_type;
    uint32_t last_connection_complete_ms;
    uint32_t incoming_l2cap_connection_count;
    uint32_t incoming_l2cap_hid_control_count;
    uint32_t incoming_l2cap_hid_interrupt_count;
    uint16_t last_incoming_l2cap_psm;
    uint16_t last_incoming_l2cap_local_cid;
    uint32_t last_incoming_l2cap_ms;
} bt_hid_snapshot_t;

bool bt_hid_target_from_persona(run_persona_t persona, bt_hid_target_t *out);
bool bt_hid_init(bt_hid_target_t target);
void bt_hid_reset_stack_state(void);
void bt_hid_snapshot(bt_hid_snapshot_t *out);
void bt_hid_task(void);
