#include "bt_hid.h"

#include <string.h>

#include "pico/stdlib.h"
#include "btstack.h"

#include "diag_log.h"
#include "gamepad_state.h"

#define BT_HID_SEND_INTERVAL_MS 10u
#define BT_HID_CLASS_OF_DEVICE 0x2508u
#define BT_HID_COUNTRY_CODE 0u
#define BT_HID_BOOT_DEVICE 0u
#define BT_HID_HOST_MAX_LATENCY 1600u
#define BT_HID_HOST_MIN_TIMEOUT 3200u
#define BT_HID_SUPERVISION_TIMEOUT 3200u
#define BT_HID_RECONNECT_DELAY_MS 600u
#define BT_HID_RECONNECT_RETRY_DELAY_MS 2500u
#define BT_HID_RECONNECT_AFTER_CLOSE_DELAY_MS 3000u
#define BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS 6u
#define BT_HID_RECONNECT_STATE_HAVE_PEER 0x01u
#define BT_HID_RECONNECT_STATE_PENDING 0x02u
#define BT_HID_RECONNECT_STATE_IN_PROGRESS 0x04u
#define BT_HID_RECONNECT_STATE_MAXED 0x08u
#define BT_HID_RECONNECT_REASON_NONE 0u
#define BT_HID_RECONNECT_REASON_LINK_KEY 1u
#define BT_HID_RECONNECT_REASON_PAIRING_DISCONNECT 2u
#define BT_HID_RECONNECT_REASON_HID_OPEN_FAILED 3u
#define BT_HID_RECONNECT_REASON_API_RETURN 4u
#define BT_HID_RECONNECT_REASON_NO_PEER 5u
#define BT_HID_RECONNECT_REASON_CONNECTED 6u
#define BT_HID_RECONNECT_REASON_MAX_ATTEMPTS 7u
#define BT_HID_RECONNECT_REASON_NOT_STARTED 8u
#define BT_HID_RECONNECT_REASON_HID_CLOSED 9u

static uint8_t hid_service_buffer[1024];
static uint8_t device_id_sdp_service_buffer[100];
static btstack_packet_callback_registration_t hci_event_callback_registration;

static bool bt_hid_started;
static bool bt_hid_connected;
static bool bt_hid_send_requested;
static uint16_t bt_hid_cid;
static bt_hid_target_t bt_hid_target = BT_HID_TARGET_GENERIC;
static absolute_time_t bt_hid_next_send_at;
static bt_hid_report_t bt_hid_pending_report;
static uint8_t bt_hid_last_status;
static uint32_t bt_hid_init_count;
static uint32_t bt_hid_ready_count;
static uint32_t bt_hid_open_count;
static uint32_t bt_hid_close_count;
static uint32_t bt_hid_can_send_count;
static uint32_t bt_hid_report_build_count;
static uint32_t bt_hid_report_send_count;
static uint32_t bt_hid_send_request_count;
static uint32_t bt_hid_last_event_ms;
static uint32_t bt_hid_last_send_ms;
static uint32_t bt_hid_get_report_count;
static uint32_t bt_hid_get_report_success_count;
static uint32_t bt_hid_get_report_unsupported_count;
static uint32_t bt_hid_set_report_count;
static uint32_t bt_hid_set_report_accepted_count;
static uint32_t bt_hid_set_report_unsupported_count;
static uint32_t bt_hid_out_report_count;
static uint32_t bt_hid_out_report_accepted_count;
static uint32_t bt_hid_out_report_unsupported_count;
static uint8_t bt_hid_last_get_report_id;
static uint8_t bt_hid_last_get_report_type;
static uint8_t bt_hid_last_set_report_id;
static uint8_t bt_hid_last_set_report_type;
static uint8_t bt_hid_last_out_report_id;
static uint8_t bt_hid_last_out_report_type;
static uint16_t bt_hid_last_get_report_len;
static uint16_t bt_hid_last_set_report_len;
static uint16_t bt_hid_last_out_report_len;
static uint32_t bt_hid_pin_code_request_count;
static uint32_t bt_hid_pin_code_response_count;
static uint32_t bt_hid_user_confirmation_request_count;
static uint32_t bt_hid_user_confirmation_response_count;
static uint32_t bt_hid_simple_pairing_complete_count;
static uint32_t bt_hid_authentication_complete_count;
static uint32_t bt_hid_link_key_notification_count;
static uint32_t bt_hid_encryption_change_count;
static uint32_t bt_hid_disconnection_complete_count;
static uint32_t bt_hid_hid_open_failed_count;
static uint32_t bt_hid_last_security_event_ms;
static uint8_t bt_hid_last_simple_pairing_status;
static uint8_t bt_hid_last_authentication_status;
static uint8_t bt_hid_last_encryption_status;
static uint8_t bt_hid_last_encryption_enabled;
static uint8_t bt_hid_last_disconnection_reason;
static uint8_t bt_hid_last_hid_open_status;
static bd_addr_t bt_hid_peer_addr;
static bool bt_hid_have_peer_addr;
static bool bt_hid_reconnect_pending;
static bool bt_hid_reconnect_in_progress;
static absolute_time_t bt_hid_next_reconnect_at;
static uint8_t bt_hid_reconnect_cycle_attempts;
static uint8_t bt_hid_last_reconnect_status;
static uint8_t bt_hid_last_reconnect_reason;
static uint32_t bt_hid_reconnect_schedule_count;
static uint32_t bt_hid_reconnect_attempt_count;
static uint32_t bt_hid_reconnect_success_count;
static uint32_t bt_hid_reconnect_failed_count;
static uint32_t bt_hid_reconnect_blocked_count;
static uint32_t bt_hid_last_reconnect_ms;
static uint32_t bt_hid_connection_complete_count;
static uint8_t bt_hid_last_connection_complete_status;
static uint8_t bt_hid_last_connection_complete_link_type;
static uint32_t bt_hid_last_connection_complete_ms;
static uint32_t bt_hid_incoming_l2cap_connection_count;
static uint32_t bt_hid_incoming_l2cap_hid_control_count;
static uint32_t bt_hid_incoming_l2cap_hid_interrupt_count;
static uint16_t bt_hid_last_incoming_l2cap_psm;
static uint16_t bt_hid_last_incoming_l2cap_local_cid;
static uint32_t bt_hid_last_incoming_l2cap_ms;

static uint32_t now_ms(void) {
    return to_ms_since_boot(get_absolute_time());
}

static void note_security_event(void) {
    uint32_t now = now_ms();
    bt_hid_last_event_ms = now;
    bt_hid_last_security_event_ms = now;
}

static void remember_peer_addr(const bd_addr_t addr) {
    memcpy(bt_hid_peer_addr, addr, sizeof(bt_hid_peer_addr));
    bt_hid_have_peer_addr = true;
}

static bool peer_addr_matches(const bd_addr_t addr) {
    return bt_hid_have_peer_addr && memcmp(bt_hid_peer_addr, addr, sizeof(bt_hid_peer_addr)) == 0;
}

static uint8_t reconnect_state_flags(void) {
    uint8_t flags = 0;
    if (bt_hid_have_peer_addr)
        flags |= BT_HID_RECONNECT_STATE_HAVE_PEER;
    if (bt_hid_reconnect_pending)
        flags |= BT_HID_RECONNECT_STATE_PENDING;
    if (bt_hid_reconnect_in_progress)
        flags |= BT_HID_RECONNECT_STATE_IN_PROGRESS;
    if (bt_hid_reconnect_cycle_attempts >= BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS)
        flags |= BT_HID_RECONNECT_STATE_MAXED;
    return flags;
}

static void block_reconnect(uint8_t reason) {
    bt_hid_reconnect_pending = false;
    bt_hid_reconnect_in_progress = false;
    bt_hid_reconnect_blocked_count++;
    bt_hid_last_reconnect_reason = reason;
    bt_hid_last_reconnect_ms = now_ms();
}

static void schedule_reconnect(uint8_t reason, uint32_t delay_ms, bool reset_cycle) {
    if (!bt_hid_started) {
        block_reconnect(BT_HID_RECONNECT_REASON_NOT_STARTED);
        return;
    }
    if (!bt_hid_have_peer_addr) {
        block_reconnect(BT_HID_RECONNECT_REASON_NO_PEER);
        diag_log_msg("bt_hid: reconnect blocked reason=no_peer");
        return;
    }
    if (bt_hid_connected) {
        bt_hid_last_reconnect_reason = BT_HID_RECONNECT_REASON_CONNECTED;
        return;
    }
    if (reset_cycle)
        bt_hid_reconnect_cycle_attempts = 0;
    if (bt_hid_reconnect_cycle_attempts >= BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS) {
        block_reconnect(BT_HID_RECONNECT_REASON_MAX_ATTEMPTS);
        diag_log_printf("bt_hid: reconnect blocked reason=max_attempts attempts=%u",
                        (unsigned)bt_hid_reconnect_cycle_attempts);
        return;
    }

    bt_hid_reconnect_pending = true;
    bt_hid_reconnect_in_progress = false;
    bt_hid_next_reconnect_at = make_timeout_time_ms(delay_ms);
    bt_hid_reconnect_schedule_count++;
    bt_hid_last_reconnect_reason = reason;
    bt_hid_last_reconnect_ms = now_ms();
    diag_log_printf("bt_hid: reconnect scheduled reason=%u attempt=%u/%u delay_ms=%u",
                    (unsigned)reason, (unsigned)(bt_hid_reconnect_cycle_attempts + 1u),
                    (unsigned)BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS, (unsigned)delay_ms);
}

static void try_reconnect_now(void) {
    if (!bt_hid_started || bt_hid_connected || bt_hid_reconnect_in_progress ||
        !bt_hid_reconnect_pending || !time_reached(bt_hid_next_reconnect_at)) {
        return;
    }
    if (!bt_hid_have_peer_addr) {
        block_reconnect(BT_HID_RECONNECT_REASON_NO_PEER);
        diag_log_msg("bt_hid: reconnect blocked reason=no_peer");
        return;
    }
    if (bt_hid_reconnect_cycle_attempts >= BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS) {
        block_reconnect(BT_HID_RECONNECT_REASON_MAX_ATTEMPTS);
        diag_log_printf("bt_hid: reconnect blocked reason=max_attempts attempts=%u",
                        (unsigned)bt_hid_reconnect_cycle_attempts);
        return;
    }

    bt_hid_reconnect_pending = false;
    bt_hid_reconnect_cycle_attempts++;
    bt_hid_reconnect_attempt_count++;
    bt_hid_last_reconnect_ms = now_ms();
    bt_hid_last_event_ms = bt_hid_last_reconnect_ms;
    uint8_t status = hid_device_connect(bt_hid_peer_addr, &bt_hid_cid);
    bt_hid_last_reconnect_status = status;
    if (status == ERROR_CODE_SUCCESS) {
        bt_hid_reconnect_in_progress = true;
        diag_log_printf("bt_hid: reconnect attempt started attempt=%u/%u",
                        (unsigned)bt_hid_reconnect_cycle_attempts,
                        (unsigned)BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS);
    } else {
        bt_hid_reconnect_failed_count++;
        diag_log_printf("bt_hid: reconnect attempt returned status=0x%02X attempt=%u/%u",
                        (unsigned)status, (unsigned)bt_hid_reconnect_cycle_attempts,
                        (unsigned)BT_HID_RECONNECT_MAX_CYCLE_ATTEMPTS);
        schedule_reconnect(BT_HID_RECONNECT_REASON_API_RETURN, BT_HID_RECONNECT_RETRY_DELAY_MS,
                           false);
    }
}

static void clear_reconnect_runtime_state(void) {
    bt_hid_have_peer_addr = false;
    memset(bt_hid_peer_addr, 0, sizeof(bt_hid_peer_addr));
    bt_hid_reconnect_pending = false;
    bt_hid_reconnect_in_progress = false;
    bt_hid_reconnect_cycle_attempts = 0;
}

static void copy_gamepad_state(gamepad_state_t *out) {
    out->buttons = g_gamepad_state.buttons;
    out->left_trigger = g_gamepad_state.left_trigger;
    out->right_trigger = g_gamepad_state.right_trigger;
    out->left_x = g_gamepad_state.left_x;
    out->left_y = g_gamepad_state.left_y;
    out->right_x = g_gamepad_state.right_x;
    out->right_y = g_gamepad_state.right_y;
}

static void request_send_now(bool force) {
    if (!bt_hid_connected || bt_hid_send_requested)
        return;
    if (!force && !time_reached(bt_hid_next_send_at))
        return;

    gamepad_state_t state;
    copy_gamepad_state(&state);
    bt_hid_build_report(bt_hid_target, &state, &bt_hid_pending_report);
    bt_hid_report_build_count++;
    bt_hid_send_requested = true;
    bt_hid_send_request_count++;
    hid_device_request_can_send_now_event(bt_hid_cid);
}

static void send_pending_report(void) {
    uint8_t interrupt_report[BT_HID_INTERRUPT_REPORT_LEN];
    if (bt_hid_pending_report.len > BT_HID_MAX_WIRE_REPORT_LEN)
        return;
    interrupt_report[0] = 0xA1u;
    memcpy(&interrupt_report[1], bt_hid_pending_report.bytes, bt_hid_pending_report.len);
    hid_device_send_interrupt_message(bt_hid_cid, interrupt_report,
                                      (uint16_t)(bt_hid_pending_report.len + 1u));
    bt_hid_report_send_count++;
    bt_hid_last_send_ms = now_ms();
    bt_hid_next_send_at = make_timeout_time_ms(BT_HID_SEND_INTERVAL_MS);
}

static int bt_hid_get_report_callback(uint16_t hid_cid, hid_report_type_t report_type,
                                      uint16_t report_id, int *out_report_size,
                                      uint8_t *out_report) {
    (void)hid_cid;
    bt_hid_get_report_count++;
    bt_hid_last_get_report_id = (uint8_t)report_id;
    bt_hid_last_get_report_type = (uint8_t)report_type;
    bt_hid_last_get_report_len = 0;
    bt_hid_last_event_ms = now_ms();

    if (!out_report_size || !out_report) {
        bt_hid_get_report_unsupported_count++;
        return -1;
    }

    gamepad_state_t state;
    copy_gamepad_state(&state);
    uint16_t len =
        bt_hid_get_report_payload(bt_hid_target, (uint8_t)report_type, (uint8_t)report_id, &state,
                                  out_report, BT_HID_MAX_WIRE_REPORT_LEN);
    *out_report_size = len;
    bt_hid_last_get_report_len = len;
    if (len == 0) {
        bt_hid_get_report_unsupported_count++;
        return -1;
    }

    bt_hid_get_report_success_count++;
    return 1;
}

static uint16_t safe_report_size(int report_size) {
    if (report_size <= 0)
        return 0;
    if (report_size > 0xFFFF)
        return 0xFFFFu;
    return (uint16_t)report_size;
}

static void bt_hid_set_report_callback(uint16_t hid_cid, hid_report_type_t report_type,
                                       int report_size, uint8_t *report) {
    (void)hid_cid;
    uint8_t report_id = 0;
    uint16_t payload_len = 0;
    uint16_t size = safe_report_size(report_size);
    bool accepted = bt_hid_accept_set_report(bt_hid_target, (uint8_t)report_type, report, size,
                                             &report_id, &payload_len);

    bt_hid_set_report_count++;
    bt_hid_last_set_report_id = report_id;
    bt_hid_last_set_report_type = (uint8_t)report_type;
    bt_hid_last_set_report_len = payload_len;
    bt_hid_last_event_ms = now_ms();
    if (accepted)
        bt_hid_set_report_accepted_count++;
    else
        bt_hid_set_report_unsupported_count++;
}

static void bt_hid_report_data_callback(uint16_t hid_cid, hid_report_type_t report_type,
                                        uint16_t report_id, int report_size, uint8_t *report) {
    (void)hid_cid;
    uint16_t payload_len = safe_report_size(report_size);
    bool accepted = bt_hid_accept_output_payload(bt_hid_target, (uint8_t)report_type,
                                                 (uint8_t)report_id, report, payload_len);

    bt_hid_out_report_count++;
    bt_hid_last_out_report_id = (uint8_t)report_id;
    bt_hid_last_out_report_type = (uint8_t)report_type;
    bt_hid_last_out_report_len = payload_len;
    bt_hid_last_event_ms = now_ms();
    if (accepted)
        bt_hid_out_report_accepted_count++;
    else
        bt_hid_out_report_unsupported_count++;
}

static void packet_handler(uint8_t packet_type, uint16_t channel, uint8_t *packet,
                           uint16_t packet_size) {
    (void)channel;
    (void)packet_size;

    bd_addr_t event_addr;
    uint8_t status;

    if (packet_type != HCI_EVENT_PACKET)
        return;

    switch (hci_event_packet_get_type(packet)) {
    case BTSTACK_EVENT_STATE:
        if (btstack_event_state_get_state(packet) == HCI_STATE_WORKING) {
            bt_hid_ready_count++;
            bt_hid_last_event_ms = now_ms();
            diag_log_printf("bt_hid: ready target=%s", bt_hid_target_label(bt_hid_target));
        }
        break;
    case HCI_EVENT_CONNECTION_COMPLETE:
        hci_event_connection_complete_get_bd_addr(packet, event_addr);
        status = hci_event_connection_complete_get_status(packet);
        if (!bt_hid_have_peer_addr || peer_addr_matches(event_addr) ||
            bt_hid_reconnect_in_progress) {
            bt_hid_connection_complete_count++;
            bt_hid_last_connection_complete_status = status;
            bt_hid_last_connection_complete_link_type =
                hci_event_connection_complete_get_link_type(packet);
            bt_hid_last_connection_complete_ms = now_ms();
            bt_hid_last_event_ms = bt_hid_last_connection_complete_ms;
            diag_log_printf("bt_hid: connection_complete status=0x%02X link_type=%u",
                            (unsigned)bt_hid_last_connection_complete_status,
                            (unsigned)bt_hid_last_connection_complete_link_type);
            if (bt_hid_reconnect_in_progress && status != ERROR_CODE_SUCCESS) {
                bt_hid_reconnect_in_progress = false;
                bt_hid_reconnect_failed_count++;
                bt_hid_last_reconnect_status = status;
                schedule_reconnect(BT_HID_RECONNECT_REASON_HID_OPEN_FAILED,
                                   BT_HID_RECONNECT_RETRY_DELAY_MS, false);
            }
        }
        break;
    case HCI_EVENT_PIN_CODE_REQUEST:
        hci_event_pin_code_request_get_bd_addr(packet, event_addr);
        remember_peer_addr(event_addr);
        bt_hid_pin_code_request_count++;
        note_security_event();
        diag_log_msg("bt_hid: pin_code_request");
        gap_pin_code_response(event_addr, "0000");
        bt_hid_pin_code_response_count++;
        break;
    case HCI_EVENT_USER_CONFIRMATION_REQUEST:
        hci_event_user_confirmation_request_get_bd_addr(packet, event_addr);
        remember_peer_addr(event_addr);
        bt_hid_user_confirmation_request_count++;
        note_security_event();
        diag_log_msg("bt_hid: user_confirmation_request");
        gap_ssp_confirmation_response(event_addr);
        bt_hid_user_confirmation_response_count++;
        break;
    case HCI_EVENT_SIMPLE_PAIRING_COMPLETE:
        bt_hid_simple_pairing_complete_count++;
        bt_hid_last_simple_pairing_status = hci_event_simple_pairing_complete_get_status(packet);
        note_security_event();
        diag_log_printf("bt_hid: simple_pairing_complete status=0x%02X",
                        (unsigned)bt_hid_last_simple_pairing_status);
        break;
    case HCI_EVENT_AUTHENTICATION_COMPLETE:
        bt_hid_authentication_complete_count++;
        bt_hid_last_authentication_status = hci_event_authentication_complete_get_status(packet);
        note_security_event();
        diag_log_printf("bt_hid: authentication_complete status=0x%02X",
                        (unsigned)bt_hid_last_authentication_status);
        break;
    case HCI_EVENT_LINK_KEY_NOTIFICATION:
        bt_hid_link_key_notification_count++;
        note_security_event();
        diag_log_msg("bt_hid: link_key_notification");
        schedule_reconnect(BT_HID_RECONNECT_REASON_LINK_KEY, BT_HID_RECONNECT_DELAY_MS, true);
        break;
    case HCI_EVENT_ENCRYPTION_CHANGE:
        bt_hid_encryption_change_count++;
        bt_hid_last_encryption_status = hci_event_encryption_change_get_status(packet);
        bt_hid_last_encryption_enabled = hci_event_encryption_change_get_encryption_enabled(packet);
        note_security_event();
        diag_log_printf("bt_hid: encryption_change status=0x%02X enabled=%u",
                        (unsigned)bt_hid_last_encryption_status,
                        (unsigned)bt_hid_last_encryption_enabled);
        break;
    case HCI_EVENT_DISCONNECTION_COMPLETE:
        bt_hid_disconnection_complete_count++;
        bt_hid_last_disconnection_reason = hci_event_disconnection_complete_get_reason(packet);
        note_security_event();
        diag_log_printf("bt_hid: disconnect_complete status=0x%02X reason=0x%02X",
                        (unsigned)hci_event_disconnection_complete_get_status(packet),
                        (unsigned)bt_hid_last_disconnection_reason);
        if (!bt_hid_connected && !bt_hid_reconnect_pending && !bt_hid_reconnect_in_progress &&
            bt_hid_have_peer_addr) {
            schedule_reconnect(BT_HID_RECONNECT_REASON_PAIRING_DISCONNECT,
                               BT_HID_RECONNECT_DELAY_MS, false);
        }
        break;
    case L2CAP_EVENT_INCOMING_CONNECTION: {
        uint16_t psm = l2cap_event_incoming_connection_get_psm(packet);
        if (psm == PSM_HID_CONTROL || psm == PSM_HID_INTERRUPT) {
            bt_hid_incoming_l2cap_connection_count++;
            if (psm == PSM_HID_CONTROL)
                bt_hid_incoming_l2cap_hid_control_count++;
            else
                bt_hid_incoming_l2cap_hid_interrupt_count++;
            bt_hid_last_incoming_l2cap_psm = psm;
            bt_hid_last_incoming_l2cap_local_cid =
                l2cap_event_incoming_connection_get_local_cid(packet);
            bt_hid_last_incoming_l2cap_ms = now_ms();
            bt_hid_last_event_ms = bt_hid_last_incoming_l2cap_ms;
            diag_log_printf("bt_hid: incoming_l2cap psm=0x%04X local_cid=0x%04X",
                            (unsigned)bt_hid_last_incoming_l2cap_psm,
                            (unsigned)bt_hid_last_incoming_l2cap_local_cid);
        }
        break;
    }
    case HCI_EVENT_HID_META:
        switch (hci_event_hid_meta_get_subevent_code(packet)) {
        case HID_SUBEVENT_CONNECTION_OPENED:
            status = hid_subevent_connection_opened_get_status(packet);
            bt_hid_last_status = status;
            bt_hid_last_event_ms = now_ms();
            if (status != ERROR_CODE_SUCCESS) {
                bt_hid_reconnect_in_progress = false;
                if (bt_hid_reconnect_cycle_attempts > 0)
                    bt_hid_reconnect_failed_count++;
                bt_hid_hid_open_failed_count++;
                bt_hid_last_hid_open_status = status;
                bt_hid_last_reconnect_status = status;
                diag_log_printf("bt_hid: connection failed status=0x%02X", (unsigned)status);
                bt_hid_connected = false;
                bt_hid_cid = 0;
                schedule_reconnect(BT_HID_RECONNECT_REASON_HID_OPEN_FAILED,
                                   BT_HID_RECONNECT_RETRY_DELAY_MS, false);
                return;
            }
            bt_hid_cid = hid_subevent_connection_opened_get_hid_cid(packet);
            bt_hid_connected = true;
            bt_hid_send_requested = false;
            bt_hid_reconnect_pending = false;
            if (bt_hid_reconnect_in_progress)
                bt_hid_reconnect_success_count++;
            bt_hid_reconnect_in_progress = false;
            bt_hid_reconnect_cycle_attempts = 0;
            bt_hid_next_send_at = get_absolute_time();
            bt_hid_open_count++;
            diag_log_msg("bt_hid: connected");
            request_send_now(true);
            break;
        case HID_SUBEVENT_CONNECTION_CLOSED: {
            bool was_connected = bt_hid_connected;
            bt_hid_connected = false;
            bt_hid_send_requested = false;
            bt_hid_reconnect_in_progress = false;
            bt_hid_cid = 0;
            bt_hid_close_count++;
            bt_hid_last_event_ms = now_ms();
            diag_log_msg("bt_hid: disconnected");
            if (was_connected)
                schedule_reconnect(BT_HID_RECONNECT_REASON_HID_CLOSED,
                                   BT_HID_RECONNECT_AFTER_CLOSE_DELAY_MS, true);
            break;
        }
        case HID_SUBEVENT_CAN_SEND_NOW:
            bt_hid_can_send_count++;
            bt_hid_last_event_ms = now_ms();
            if (bt_hid_connected && bt_hid_send_requested) {
                bt_hid_send_requested = false;
                send_pending_report();
            }
            break;
        default:
            break;
        }
        break;
    default:
        break;
    }
}

bool bt_hid_target_from_persona(run_persona_t persona, bt_hid_target_t *out) {
    switch (persona) {
    case RUN_PERSONA_BT_HID:
        *out = BT_HID_TARGET_GENERIC;
        return true;
    case RUN_PERSONA_BT_XBOX:
        *out = BT_HID_TARGET_XBOX;
        return true;
    case RUN_PERSONA_BT_PS:
        *out = BT_HID_TARGET_PLAYSTATION;
        return true;
    default:
        return false;
    }
}

bool bt_hid_init(bt_hid_target_t target) {
    if (bt_hid_started)
        return bt_hid_target == target;

    uint16_t descriptor_len = 0;
    const uint8_t *descriptor = bt_hid_descriptor(target, &descriptor_len);

    bt_hid_target = target;
    bt_hid_next_send_at = get_absolute_time();

    gap_discoverable_control(1);
    gap_connectable_control(1);
    gap_set_class_of_device(BT_HID_CLASS_OF_DEVICE);
    gap_set_local_name(bt_hid_local_name(target));
    gap_set_default_link_policy_settings(LM_LINK_POLICY_ENABLE_ROLE_SWITCH |
                                         LM_LINK_POLICY_ENABLE_SNIFF_MODE);
    gap_set_allow_role_switch(true);
    gap_set_bondable_mode(1);
    gap_ssp_set_enable(1);
    gap_ssp_set_io_capability(SSP_IO_CAPABILITY_NO_INPUT_NO_OUTPUT);
    gap_ssp_set_authentication_requirement(
        SSP_IO_AUTHREQ_MITM_PROTECTION_NOT_REQUIRED_GENERAL_BONDING);
    gap_ssp_set_auto_accept(1);

    l2cap_init();
    sdp_init();

    memset(hid_service_buffer, 0, sizeof(hid_service_buffer));
    hid_sdp_record_t hid_params = {
        BT_HID_CLASS_OF_DEVICE,
        BT_HID_COUNTRY_CODE,
        0,
        1,
        1,
        1,
        BT_HID_BOOT_DEVICE,
        BT_HID_HOST_MAX_LATENCY,
        BT_HID_HOST_MIN_TIMEOUT,
        BT_HID_SUPERVISION_TIMEOUT,
        descriptor,
        descriptor_len,
        bt_hid_service_name(target),
    };
    hid_create_sdp_record(hid_service_buffer, sdp_create_service_record_handle(), &hid_params);
    sdp_register_service(hid_service_buffer);

    memset(device_id_sdp_service_buffer, 0, sizeof(device_id_sdp_service_buffer));
    device_id_create_sdp_record(device_id_sdp_service_buffer, sdp_create_service_record_handle(),
                                DEVICE_ID_VENDOR_ID_SOURCE_USB, bt_hid_vendor_id(target),
                                bt_hid_product_id(target), bt_hid_bcd_version(target));
    sdp_register_service(device_id_sdp_service_buffer);

    hid_device_init(BT_HID_BOOT_DEVICE, descriptor_len, descriptor);

    hci_event_callback_registration.callback = &packet_handler;
    hci_add_event_handler(&hci_event_callback_registration);
    hid_device_register_packet_handler(&packet_handler);
    hid_device_register_report_request_callback(&bt_hid_get_report_callback);
    hid_device_register_set_report_callback(&bt_hid_set_report_callback);
    hid_device_register_report_data_callback(&bt_hid_report_data_callback);

    bt_hid_started = true;
    bt_hid_init_count++;
    bt_hid_last_event_ms = now_ms();
    diag_log_printf("bt_hid: init target=%s", bt_hid_target_label(target));
    diag_log_printf("bt_hid: discoverable name=%s class=0x%04X", bt_hid_local_name(target),
                    (unsigned)BT_HID_CLASS_OF_DEVICE);
    hci_power_control(HCI_POWER_ON);
    return true;
}

void bt_hid_reset_stack_state(void) {
    bt_hid_started = false;
    bt_hid_connected = false;
    bt_hid_send_requested = false;
    bt_hid_cid = 0;
    clear_reconnect_runtime_state();
    memset(&hci_event_callback_registration, 0, sizeof(hci_event_callback_registration));
    memset(&bt_hid_pending_report, 0, sizeof(bt_hid_pending_report));
}

void bt_hid_snapshot(bt_hid_snapshot_t *out) {
    memset(out, 0, sizeof(*out));
    if (bt_hid_started)
        out->flags |= BT_HID_STATUS_STARTED;
    if (bt_hid_connected)
        out->flags |= BT_HID_STATUS_CONNECTED;
    if (bt_hid_send_requested)
        out->flags |= BT_HID_STATUS_SEND_REQUESTED;
    out->target = (uint8_t)bt_hid_target;
    out->last_status = bt_hid_last_status;
    out->report_len = bt_hid_pending_report.len;
    out->cid = bt_hid_cid;
    out->init_count = bt_hid_init_count;
    out->ready_count = bt_hid_ready_count;
    out->open_count = bt_hid_open_count;
    out->close_count = bt_hid_close_count;
    out->can_send_count = bt_hid_can_send_count;
    out->report_build_count = bt_hid_report_build_count;
    out->report_send_count = bt_hid_report_send_count;
    out->send_request_count = bt_hid_send_request_count;
    out->last_event_ms = bt_hid_last_event_ms;
    out->last_send_ms = bt_hid_last_send_ms;
    out->get_report_count = bt_hid_get_report_count;
    out->get_report_success_count = bt_hid_get_report_success_count;
    out->get_report_unsupported_count = bt_hid_get_report_unsupported_count;
    out->set_report_count = bt_hid_set_report_count;
    out->set_report_accepted_count = bt_hid_set_report_accepted_count;
    out->set_report_unsupported_count = bt_hid_set_report_unsupported_count;
    out->out_report_count = bt_hid_out_report_count;
    out->out_report_accepted_count = bt_hid_out_report_accepted_count;
    out->out_report_unsupported_count = bt_hid_out_report_unsupported_count;
    out->last_get_report_id = bt_hid_last_get_report_id;
    out->last_get_report_type = bt_hid_last_get_report_type;
    out->last_set_report_id = bt_hid_last_set_report_id;
    out->last_set_report_type = bt_hid_last_set_report_type;
    out->last_out_report_id = bt_hid_last_out_report_id;
    out->last_out_report_type = bt_hid_last_out_report_type;
    out->last_get_report_len = bt_hid_last_get_report_len;
    out->last_set_report_len = bt_hid_last_set_report_len;
    out->last_out_report_len = bt_hid_last_out_report_len;
    out->pin_code_request_count = bt_hid_pin_code_request_count;
    out->pin_code_response_count = bt_hid_pin_code_response_count;
    out->user_confirmation_request_count = bt_hid_user_confirmation_request_count;
    out->user_confirmation_response_count = bt_hid_user_confirmation_response_count;
    out->simple_pairing_complete_count = bt_hid_simple_pairing_complete_count;
    out->authentication_complete_count = bt_hid_authentication_complete_count;
    out->link_key_notification_count = bt_hid_link_key_notification_count;
    out->encryption_change_count = bt_hid_encryption_change_count;
    out->disconnection_complete_count = bt_hid_disconnection_complete_count;
    out->hid_open_failed_count = bt_hid_hid_open_failed_count;
    out->last_security_event_ms = bt_hid_last_security_event_ms;
    out->last_simple_pairing_status = bt_hid_last_simple_pairing_status;
    out->last_authentication_status = bt_hid_last_authentication_status;
    out->last_encryption_status = bt_hid_last_encryption_status;
    out->last_encryption_enabled = bt_hid_last_encryption_enabled;
    out->last_disconnection_reason = bt_hid_last_disconnection_reason;
    out->last_hid_open_status = bt_hid_last_hid_open_status;
    out->reconnect_state = reconnect_state_flags();
    out->reconnect_cycle_attempts = bt_hid_reconnect_cycle_attempts;
    out->last_reconnect_status = bt_hid_last_reconnect_status;
    out->last_reconnect_reason = bt_hid_last_reconnect_reason;
    out->reconnect_schedule_count = bt_hid_reconnect_schedule_count;
    out->reconnect_attempt_count = bt_hid_reconnect_attempt_count;
    out->reconnect_success_count = bt_hid_reconnect_success_count;
    out->reconnect_failed_count = bt_hid_reconnect_failed_count;
    out->reconnect_blocked_count = bt_hid_reconnect_blocked_count;
    out->last_reconnect_ms = bt_hid_last_reconnect_ms;
    out->connection_complete_count = bt_hid_connection_complete_count;
    out->last_connection_complete_status = bt_hid_last_connection_complete_status;
    out->last_connection_complete_link_type = bt_hid_last_connection_complete_link_type;
    out->last_connection_complete_ms = bt_hid_last_connection_complete_ms;
    out->incoming_l2cap_connection_count = bt_hid_incoming_l2cap_connection_count;
    out->incoming_l2cap_hid_control_count = bt_hid_incoming_l2cap_hid_control_count;
    out->incoming_l2cap_hid_interrupt_count = bt_hid_incoming_l2cap_hid_interrupt_count;
    out->last_incoming_l2cap_psm = bt_hid_last_incoming_l2cap_psm;
    out->last_incoming_l2cap_local_cid = bt_hid_last_incoming_l2cap_local_cid;
    out->last_incoming_l2cap_ms = bt_hid_last_incoming_l2cap_ms;
}

void bt_hid_task(void) {
    try_reconnect_now();
    request_send_now(false);
}
