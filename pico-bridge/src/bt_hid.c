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

static uint32_t now_ms(void) {
    return to_ms_since_boot(get_absolute_time());
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
    case HCI_EVENT_PIN_CODE_REQUEST:
        hci_event_pin_code_request_get_bd_addr(packet, event_addr);
        gap_pin_code_response(event_addr, "0000");
        break;
    case HCI_EVENT_USER_CONFIRMATION_REQUEST:
        hci_event_user_confirmation_request_get_bd_addr(packet, event_addr);
        gap_ssp_confirmation_response(event_addr);
        break;
    case HCI_EVENT_HID_META:
        switch (hci_event_hid_meta_get_subevent_code(packet)) {
        case HID_SUBEVENT_CONNECTION_OPENED:
            status = hid_subevent_connection_opened_get_status(packet);
            bt_hid_last_status = status;
            bt_hid_last_event_ms = now_ms();
            if (status != ERROR_CODE_SUCCESS) {
                diag_log_printf("bt_hid: connection failed status=0x%02X", (unsigned)status);
                bt_hid_connected = false;
                bt_hid_cid = 0;
                return;
            }
            bt_hid_cid = hid_subevent_connection_opened_get_hid_cid(packet);
            bt_hid_connected = true;
            bt_hid_send_requested = false;
            bt_hid_next_send_at = get_absolute_time();
            bt_hid_open_count++;
            diag_log_msg("bt_hid: connected");
            request_send_now(true);
            break;
        case HID_SUBEVENT_CONNECTION_CLOSED:
            bt_hid_connected = false;
            bt_hid_send_requested = false;
            bt_hid_cid = 0;
            bt_hid_close_count++;
            bt_hid_last_event_ms = now_ms();
            diag_log_msg("bt_hid: disconnected");
            break;
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
    hci_power_control(HCI_POWER_ON);
    return true;
}

void bt_hid_reset_stack_state(void) {
    bt_hid_started = false;
    bt_hid_connected = false;
    bt_hid_send_requested = false;
    bt_hid_cid = 0;
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
}

void bt_hid_task(void) {
    request_send_now(false);
}
