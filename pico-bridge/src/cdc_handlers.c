#include "cdc_handlers.h"

#include <string.h>

#include "tusb.h"
#include "pico/stdlib.h"
#include "pico/unique_id.h"

#include "boot_mode.h"
#include "boot_mode_policy.h"
#include "bt_hid.h"
#include "bt_hid_report.h"
#include "flash_creds.h"
#include "gamepad_state.h"
#include "diag_log.h"
#include "version.h"

#define RX_BUFFER_SIZE (CDC_MAX_FRAME + 64)

static uint8_t rx_buf[RX_BUFFER_SIZE];
static uint8_t tx_frame[CDC_MAX_FRAME];
static uint8_t log_payload[4 + DIAG_LOG_RING_SIZE];
static size_t rx_len;
static bool reboot_pending;
static bool bootsel_pending;
static uint32_t bt_cdc_state_count;
static uint32_t bt_cdc_heartbeat_count;
static uint32_t bt_cdc_bad_length_count;
static uint32_t bt_cdc_rejected_count;
static uint32_t bt_cdc_last_frame_ms;
static uint32_t bt_cdc_last_state_ms;
static uint32_t bt_cdc_last_heartbeat_ms;
static uint8_t bt_cdc_last_seq;
static uint8_t bt_cdc_last_command;
static uint8_t bt_cdc_last_flags;

static void write_cdc_frame(const uint8_t *frame, size_t n) {
    if (!frame || n == 0)
        return;

    size_t sent = 0;
    absolute_time_t deadline = make_timeout_time_ms(3000);
    while (sent < n) {
        tud_task();
        uint32_t avail = tud_cdc_write_available();
        if (avail == 0) {
            tud_cdc_write_flush();
            if (absolute_time_diff_us(get_absolute_time(), deadline) <= 0)
                break;
            sleep_ms(1);
            continue;
        }

        size_t chunk = n - sent;
        if (chunk > avail)
            chunk = avail;
        uint32_t wrote = tud_cdc_write(frame + sent, (uint32_t)chunk);
        if (wrote == 0) {
            tud_cdc_write_flush();
            if (absolute_time_diff_us(get_absolute_time(), deadline) <= 0)
                break;
            sleep_ms(1);
            continue;
        }
        sent += wrote;
        tud_cdc_write_flush();
    }

    if (sent < n) {
        diag_log_printf("cdc: response write timed out sent=%u total=%u", (unsigned)sent,
                        (unsigned)n);
    }
}

static void send_nack(uint8_t seq, uint8_t code, uint8_t detail) {
    uint8_t payload[2] = {code, detail};
    size_t n = cdc_encode(CDC_RSP_NACK, seq, payload, 2, tx_frame, sizeof(tx_frame));
    if (n)
        write_cdc_frame(tx_frame, n);
}

static size_t handle_hello(uint8_t seq, uint8_t *reply, size_t cap) {
    flash_creds_t cur;
    bool have = flash_creds_load(&cur);
    // Wipe the on-stack copy: it contains the cleartext password from
    // flash, and we only needed the presence bit here.
    memset(&cur, 0, sizeof(cur));
    uint8_t payload[12 + PICO_BRIDGE_FW_SUFFIX_LEN];
    payload[0] = CDC_PROTO_VERSION;
    payload[1] = PICO_BRIDGE_FW_WIRE_MAJOR;
    payload[2] = PICO_BRIDGE_FW_WIRE_MINOR;
    payload[3] = PICO_BRIDGE_FW_WIRE_PATCH;
    payload[4] = PICO_BRIDGE_BOARD_TYPE;
    payload[5] = (have ? CDC_HELLO_FLAG_CREDS_PRESENT : 0) | CDC_HELLO_FLAG_RUN_MODE_OK;
    if (boot_mode_current() == BOOT_MODE_RUN) {
        payload[5] |= CDC_HELLO_FLAG_RUN_MODE_ACTIVE;
    }
    payload[6] = (uint8_t)(PICO_BRIDGE_FW_YEAR & 0xFFu);
    payload[7] = (uint8_t)((PICO_BRIDGE_FW_YEAR >> 8) & 0xFFu);
    payload[8] = PICO_BRIDGE_FW_MONTH;
    payload[9] = PICO_BRIDGE_FW_DAY;
    payload[10] = PICO_BRIDGE_FW_REVISION;
    payload[11] = PICO_BRIDGE_FW_SUFFIX_LEN;
    if (PICO_BRIDGE_FW_SUFFIX_LEN > 0) {
        memcpy(&payload[12], PICO_BRIDGE_FW_SUFFIX, PICO_BRIDGE_FW_SUFFIX_LEN);
    }
    return cdc_encode(CDC_RSP_HELLO, seq, payload, sizeof(payload), reply, cap);
}

static size_t handle_set_wifi(const cdc_frame_view_t *req, uint8_t *reply, size_t cap) {
    if (req->payload_len < 2) {
        uint8_t err[2] = {CDC_ERR_BAD_LENGTH, 0};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    uint8_t ssid_len = req->payload[0];
    if (ssid_len == 0 || ssid_len > FLASH_CREDS_SSID_MAX ||
        (size_t)1 + ssid_len + 1 > req->payload_len) {
        uint8_t err[2] = {CDC_ERR_BAD_LENGTH, 1};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    uint8_t pass_len = req->payload[1 + ssid_len];
    if (pass_len > FLASH_CREDS_PASS_MAX ||
        (size_t)1 + ssid_len + 1 + pass_len != req->payload_len) {
        uint8_t err[2] = {CDC_ERR_BAD_LENGTH, 2};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }

    flash_creds_t rec;
    memset(&rec, 0, sizeof(rec));
    rec.ssid_len = ssid_len;
    rec.pass_len = pass_len;
    memcpy(rec.ssid, &req->payload[1], ssid_len);
    memcpy(rec.password, &req->payload[1 + ssid_len + 1], pass_len);

    // Preserve device_name and the chosen output persona across
    // re-provisioning if there was a prior record.
    flash_creds_t prev;
    if (flash_creds_load(&prev)) {
        rec.name_len = prev.name_len;
        memcpy(rec.device_name, prev.device_name, FLASH_CREDS_NAME_MAX);
        rec.usb_persona = prev.usb_persona;
    }

    int rc = flash_creds_store(&rec);
    // Zero the local copy of the password ASAP.
    memset(&rec, 0, sizeof(rec));

    if (rc == -2) {
        uint8_t err[2] = {CDC_ERR_FLASH_WRITE_FAIL, 0};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    } else if (rc == -3) {
        uint8_t err[2] = {CDC_ERR_FLASH_VERIFY_FAIL, 0};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    } else if (rc < 0) {
        uint8_t err[2] = {CDC_ERR_INTERNAL, (uint8_t)(-rc)};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    return cdc_encode(CDC_RSP_SET_WIFI, req->seq, NULL, 0, reply, cap);
}

static size_t handle_reboot(uint8_t seq, uint8_t *reply, size_t cap) {
    reboot_pending = true;
    return cdc_encode(CDC_RSP_REBOOT, seq, NULL, 0, reply, cap);
}

static size_t handle_reboot_to_bootsel(uint8_t seq, uint8_t *reply, size_t cap) {
    bootsel_pending = true;
    return cdc_encode(CDC_RSP_REBOOT_TO_BOOTSEL, seq, NULL, 0, reply, cap);
}

static void apply_bt_state_body(const uint8_t *payload) {
    const uint8_t flags = payload[0];
    const uint8_t *body = &payload[1];
    g_gamepad_state.buttons = (uint16_t)body[0] | ((uint16_t)body[1] << 8);
    g_gamepad_state.left_trigger = body[2];
    g_gamepad_state.right_trigger = body[3];
    g_gamepad_state.left_x = (int16_t)((uint16_t)body[4] | ((uint16_t)body[5] << 8));
    g_gamepad_state.left_y = (int16_t)((uint16_t)body[6] | ((uint16_t)body[7] << 8));
    g_gamepad_state.right_x = (int16_t)((uint16_t)body[8] | ((uint16_t)body[9] << 8));
    g_gamepad_state.right_y = (int16_t)((uint16_t)body[10] | ((uint16_t)body[11] << 8));
    g_parsec_connected = (flags & 0x01u) ? 1 : 0;
    g_last_packet_ms = to_ms_since_boot(get_absolute_time());
}

static void note_bt_cdc_frame(const cdc_frame_view_t *req) {
    uint32_t now = to_ms_since_boot(get_absolute_time());
    bt_cdc_last_frame_ms = now;
    bt_cdc_last_seq = req->seq;
    bt_cdc_last_command = req->command;
    bt_cdc_last_flags = req->payload_len > 0 ? req->payload[0] : 0;
}

static size_t handle_bt_state(const cdc_frame_view_t *req, uint8_t *reply, size_t cap) {
    note_bt_cdc_frame(req);
    if (boot_mode_current() != BOOT_MODE_RUN ||
        !boot_mode_persona_uses_bluetooth(boot_mode_run_persona())) {
        bt_cdc_rejected_count++;
        uint8_t err[2] = {CDC_ERR_INTERNAL, 1};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    if (req->payload_len != 13) {
        bt_cdc_bad_length_count++;
        uint8_t err[2] = {CDC_ERR_BAD_LENGTH, (uint8_t)req->payload_len};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    if (req->command == CDC_CMD_BT_STATE) {
        bt_cdc_state_count++;
        bt_cdc_last_state_ms = bt_cdc_last_frame_ms;
    } else {
        bt_cdc_heartbeat_count++;
        bt_cdc_last_heartbeat_ms = bt_cdc_last_frame_ms;
    }
    apply_bt_state_body(req->payload);
    return 0;
}

static size_t handle_bt_get_status(const cdc_frame_view_t *req, uint8_t *reply, size_t cap) {
    if (req->payload_len != 0) {
        uint8_t err[2] = {CDC_ERR_BAD_LENGTH, (uint8_t)req->payload_len};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    if (boot_mode_current() != BOOT_MODE_RUN ||
        !boot_mode_persona_uses_bluetooth(boot_mode_run_persona())) {
        uint8_t err[2] = {CDC_ERR_INTERNAL, 2};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }

    bt_hid_snapshot_t snap;
    bt_hid_snapshot(&snap);
    cdc_bt_status_view_t status = {
        .flags = snap.flags,
        .target = snap.target,
        .last_status = snap.last_status,
        .report_len = snap.report_len,
        .cid = snap.cid,
        .init_count = snap.init_count,
        .ready_count = snap.ready_count,
        .open_count = snap.open_count,
        .close_count = snap.close_count,
        .can_send_count = snap.can_send_count,
        .report_build_count = snap.report_build_count,
        .report_send_count = snap.report_send_count,
        .send_request_count = snap.send_request_count,
        .last_event_ms = snap.last_event_ms,
        .last_send_ms = snap.last_send_ms,
        .get_report_count = snap.get_report_count,
        .get_report_success_count = snap.get_report_success_count,
        .get_report_unsupported_count = snap.get_report_unsupported_count,
        .set_report_count = snap.set_report_count,
        .set_report_accepted_count = snap.set_report_accepted_count,
        .set_report_unsupported_count = snap.set_report_unsupported_count,
        .out_report_count = snap.out_report_count,
        .out_report_accepted_count = snap.out_report_accepted_count,
        .out_report_unsupported_count = snap.out_report_unsupported_count,
        .last_get_report_id = snap.last_get_report_id,
        .last_get_report_type = snap.last_get_report_type,
        .last_set_report_id = snap.last_set_report_id,
        .last_set_report_type = snap.last_set_report_type,
        .last_out_report_id = snap.last_out_report_id,
        .last_out_report_type = snap.last_out_report_type,
        .last_get_report_len = snap.last_get_report_len,
        .last_set_report_len = snap.last_set_report_len,
        .last_out_report_len = snap.last_out_report_len,
        .pin_code_request_count = snap.pin_code_request_count,
        .pin_code_response_count = snap.pin_code_response_count,
        .user_confirmation_request_count = snap.user_confirmation_request_count,
        .user_confirmation_response_count = snap.user_confirmation_response_count,
        .simple_pairing_complete_count = snap.simple_pairing_complete_count,
        .authentication_complete_count = snap.authentication_complete_count,
        .link_key_notification_count = snap.link_key_notification_count,
        .encryption_change_count = snap.encryption_change_count,
        .disconnection_complete_count = snap.disconnection_complete_count,
        .hid_open_failed_count = snap.hid_open_failed_count,
        .last_security_event_ms = snap.last_security_event_ms,
        .last_simple_pairing_status = snap.last_simple_pairing_status,
        .last_authentication_status = snap.last_authentication_status,
        .last_encryption_status = snap.last_encryption_status,
        .last_encryption_enabled = snap.last_encryption_enabled,
        .last_disconnection_reason = snap.last_disconnection_reason,
        .last_hid_open_status = snap.last_hid_open_status,
        .reconnect_state = snap.reconnect_state,
        .reconnect_cycle_attempts = snap.reconnect_cycle_attempts,
        .last_reconnect_status = snap.last_reconnect_status,
        .last_reconnect_reason = snap.last_reconnect_reason,
        .reconnect_schedule_count = snap.reconnect_schedule_count,
        .reconnect_attempt_count = snap.reconnect_attempt_count,
        .reconnect_success_count = snap.reconnect_success_count,
        .reconnect_failed_count = snap.reconnect_failed_count,
        .reconnect_blocked_count = snap.reconnect_blocked_count,
        .last_reconnect_ms = snap.last_reconnect_ms,
        .connection_complete_count = snap.connection_complete_count,
        .last_connection_complete_status = snap.last_connection_complete_status,
        .last_connection_complete_link_type = snap.last_connection_complete_link_type,
        .last_connection_complete_ms = snap.last_connection_complete_ms,
        .incoming_l2cap_connection_count = snap.incoming_l2cap_connection_count,
        .incoming_l2cap_hid_control_count = snap.incoming_l2cap_hid_control_count,
        .incoming_l2cap_hid_interrupt_count = snap.incoming_l2cap_hid_interrupt_count,
        .last_incoming_l2cap_psm = snap.last_incoming_l2cap_psm,
        .last_incoming_l2cap_local_cid = snap.last_incoming_l2cap_local_cid,
        .last_incoming_l2cap_ms = snap.last_incoming_l2cap_ms,
        .bt_cdc_state_count = bt_cdc_state_count,
        .bt_cdc_heartbeat_count = bt_cdc_heartbeat_count,
        .bt_cdc_bad_length_count = bt_cdc_bad_length_count,
        .bt_cdc_rejected_count = bt_cdc_rejected_count,
        .bt_cdc_last_frame_ms = bt_cdc_last_frame_ms,
        .bt_cdc_last_state_ms = bt_cdc_last_state_ms,
        .bt_cdc_last_heartbeat_ms = bt_cdc_last_heartbeat_ms,
        .bt_cdc_last_seq = bt_cdc_last_seq,
        .bt_cdc_last_command = bt_cdc_last_command,
        .bt_cdc_last_flags = bt_cdc_last_flags,
        .local_name = bt_hid_local_name((bt_hid_target_t)snap.target),
    };
    uint8_t payload[CDC_BT_STATUS_FIXED_LEN + CDC_BT_STATUS_MAX_NAME];
    size_t payload_len = cdc_build_bt_status_payload(&status, payload, sizeof(payload));
    if (!payload_len) {
        uint8_t err[2] = {CDC_ERR_INTERNAL, 3};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    return cdc_encode(CDC_RSP_BT_STATUS, req->seq, payload, payload_len, reply, cap);
}

static size_t handle_unique_id(uint8_t seq, uint8_t *reply, size_t cap) {
    pico_unique_board_id_t id;
    pico_get_unique_board_id(&id);
    return cdc_encode(CDC_RSP_UNIQUE_ID, seq, id.id, sizeof(id.id), reply, cap);
}

static size_t handle_device_name_get(uint8_t seq, uint8_t *reply, size_t cap) {
    flash_creds_t rec;
    if (!flash_creds_load(&rec)) {
        return cdc_encode(CDC_RSP_DEVICE_NAME, seq, NULL, 0, reply, cap);
    }
    return cdc_encode(CDC_RSP_DEVICE_NAME, seq, rec.device_name, rec.name_len, reply, cap);
}

static size_t handle_device_name_set(const cdc_frame_view_t *req, uint8_t *reply, size_t cap) {
    if (req->payload_len > FLASH_CREDS_NAME_MAX) {
        uint8_t err[2] = {CDC_ERR_BAD_LENGTH, 0};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    flash_creds_t rec;
    if (!flash_creds_load(&rec)) {
        // No creds yet; can't store a name on its own.
        uint8_t err[2] = {CDC_ERR_INTERNAL, 1};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    rec.name_len = (uint8_t)req->payload_len;
    memset(rec.device_name, 0, FLASH_CREDS_NAME_MAX);
    if (req->payload_len)
        memcpy(rec.device_name, req->payload, req->payload_len);
    int rc = flash_creds_store(&rec);
    memset(&rec, 0, sizeof(rec));
    if (rc != 0) {
        uint8_t err[2] = {CDC_ERR_FLASH_WRITE_FAIL, 0};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    return cdc_encode(CDC_RSP_SET_DEVICE_NAME, req->seq, NULL, 0, reply, cap);
}

static size_t handle_log_buffer(uint8_t seq, uint8_t *reply, size_t cap) {
    // Wire format: 4-byte little-endian `lost_bytes` count, followed
    // by the most-recent log bytes. The host gates parsing of the
    // prefix on `payload_len >= 4`, so older firmware (no prefix) and
    // older hosts (no understanding of the prefix) interoperate as
    // long as one or the other is patched.
    if (sizeof(log_payload) < 4)
        return 0;
    uint32_t lost = 0;
    size_t tail_cap = sizeof(log_payload) - 4;
    size_t n = diag_log_snapshot(log_payload + 4, tail_cap, &lost);
    log_payload[0] = (uint8_t)(lost & 0xFFu);
    log_payload[1] = (uint8_t)((lost >> 8) & 0xFFu);
    log_payload[2] = (uint8_t)((lost >> 16) & 0xFFu);
    log_payload[3] = (uint8_t)((lost >> 24) & 0xFFu);
    return cdc_encode(CDC_RSP_LOG_BUFFER, seq, log_payload, 4 + n, reply, cap);
}

static size_t handle_self_test(uint8_t seq, uint8_t *reply, size_t cap) {
    // Lightweight; runs entirely in software, no Wi-Fi.
    char buf[160];
    bool flash_ok = true;
    bool ok = true;
    flash_creds_t rec;
    bool have = flash_creds_load(&rec);
    int n = snprintf(buf, sizeof(buf), "result=%s flash=%s creds=%s fw=%s board=0x%02X",
                     ok ? "pass" : "fail", flash_ok ? "ok" : "bad", have ? "present" : "absent",
                     PICO_BRIDGE_FW_VERSION_STRING, PICO_BRIDGE_BOARD_TYPE);
    if (n < 0)
        n = 0;
    if ((size_t)n > sizeof(buf))
        n = sizeof(buf);
    uint8_t payload[1 + sizeof(buf)];
    payload[0] = ok ? 0 : 1;
    memcpy(&payload[1], buf, n);
    return cdc_encode(CDC_RSP_SELF_TEST, seq, payload, 1 + n, reply, cap);
}

size_t cdc_dispatch(const cdc_frame_view_t *req, uint8_t *reply, size_t reply_cap) {
    switch (req->command) {
    case CDC_CMD_HELLO:
        return handle_hello(req->seq, reply, reply_cap);
    case CDC_CMD_GET_STATUS:
        return handle_hello(req->seq, reply, reply_cap); // for now, same body
    case CDC_CMD_SET_WIFI:
        return handle_set_wifi(req, reply, reply_cap);
    case CDC_CMD_REBOOT_TO_RUN:
        return handle_reboot(req->seq, reply, reply_cap);
    case CDC_CMD_SELF_TEST:
        return handle_self_test(req->seq, reply, reply_cap);
    case CDC_CMD_GET_DEVICE_NAME:
        return handle_device_name_get(req->seq, reply, reply_cap);
    case CDC_CMD_SET_DEVICE_NAME:
        return handle_device_name_set(req, reply, reply_cap);
    case CDC_CMD_GET_UNIQUE_ID:
        return handle_unique_id(req->seq, reply, reply_cap);
    case CDC_CMD_GET_LOG_BUFFER:
        return handle_log_buffer(req->seq, reply, reply_cap);
    case CDC_CMD_REBOOT_TO_BOOTSEL:
        return handle_reboot_to_bootsel(req->seq, reply, reply_cap);
    case CDC_CMD_BT_STATE:
    case CDC_CMD_BT_HEARTBEAT:
        return handle_bt_state(req, reply, reply_cap);
    case CDC_CMD_BT_GET_STATUS:
        return handle_bt_get_status(req, reply, reply_cap);
    default: {
        uint8_t err[2] = {CDC_ERR_UNKNOWN_COMMAND, req->command};
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, reply_cap);
    }
    }
}

void cdc_handlers_init(void) {
    rx_len = 0;
    reboot_pending = false;
    bootsel_pending = false;
    bt_cdc_state_count = 0;
    bt_cdc_heartbeat_count = 0;
    bt_cdc_bad_length_count = 0;
    bt_cdc_rejected_count = 0;
    bt_cdc_last_frame_ms = 0;
    bt_cdc_last_state_ms = 0;
    bt_cdc_last_heartbeat_ms = 0;
    bt_cdc_last_seq = 0;
    bt_cdc_last_command = 0;
    bt_cdc_last_flags = 0;
}

bool cdc_handlers_reboot_pending(void) {
    // Reboot only once the TX FIFO is fully drained -- meaning the ACK
    // frame we just queued has actually left the device. tud_cdc_write_available()
    // returns the number of FREE bytes, so the FIFO is empty when it
    // equals the configured TX bufsize.
    return reboot_pending && (tud_cdc_write_available() == CFG_TUD_CDC_TX_BUFSIZE);
}

bool cdc_handlers_bootsel_pending(void) {
    return bootsel_pending && (tud_cdc_write_available() == CFG_TUD_CDC_TX_BUFSIZE);
}

void cdc_handlers_poll(void) {
    // Log DTR transitions so a bundle clearly shows whether the host
    // ever opened the port. DTR is informational only -- bytes are
    // processed whenever they arrive, regardless of DTR state. The
    // previous behaviour was to drop incoming bytes whenever DTR was
    // low, which silently ate HELLO from hosts that send before they
    // assert DTR (or that never assert it on `open`).
    static bool last_connected = false;
    bool connected = tud_cdc_connected();
    if (connected != last_connected) {
        diag_log_msg(connected ? "cdc: DTR asserted by host (line opened)"
                               : "cdc: DTR cleared by host (line closed)");
        last_connected = connected;
    }

    // Drain incoming bytes into rx_buf regardless of DTR state. Bytes
    // are queued in TinyUSB's CDC FIFO until we read them; if the host
    // never opens the line they stay there until reboot, which is fine.
    while (tud_cdc_available() && rx_len < sizeof(rx_buf)) {
        size_t want = sizeof(rx_buf) - rx_len;
        if (want > 64)
            want = 64;
        rx_len += tud_cdc_read(&rx_buf[rx_len], want);
    }

    // Try to decode as many complete frames as possible.
    while (rx_len > 0) {
        cdc_frame_view_t view;
        size_t consumed = 0;
        cdc_decode_status_t st = cdc_try_decode(rx_buf, rx_len, &view, &consumed);
        if (st == CDC_DECODE_NEED_MORE)
            break;

        if (st == CDC_DECODE_OK) {
            if (view.command != CDC_CMD_BT_STATE && view.command != CDC_CMD_BT_HEARTBEAT) {
                diag_log_printf("cdc: dispatching cmd=0x%02X seq=%u payload=%u bytes",
                                (unsigned)view.command, (unsigned)view.seq,
                                (unsigned)view.payload_len);
            }
            size_t n = cdc_dispatch(&view, tx_frame, sizeof(tx_frame));
            if (n > 0)
                write_cdc_frame(tx_frame, n);
            // Shift remaining bytes down.
            memmove(rx_buf, &rx_buf[consumed], rx_len - consumed);
            rx_len -= consumed;
        } else {
            // Resync: drop one byte and retry.
            if (st == CDC_DECODE_BAD_CRC) {
                diag_log_msg("cdc: BAD_CRC, resyncing one byte");
                send_nack(rx_buf[6], CDC_ERR_BAD_CRC, 0);
            } else if (st == CDC_DECODE_BAD_VERSION) {
                diag_log_printf("cdc: BAD_VERSION (got 0x%02X), resyncing one byte",
                                (unsigned)rx_buf[2]);
                send_nack(rx_buf[6], CDC_ERR_BAD_VERSION, rx_buf[2]);
            } else if (st == CDC_DECODE_BAD_LENGTH) {
                diag_log_msg("cdc: BAD_LENGTH, resyncing one byte");
                send_nack(0, CDC_ERR_BAD_LENGTH, 0);
            }
            tud_cdc_write_flush();
            memmove(rx_buf, &rx_buf[1], rx_len - 1);
            rx_len -= 1;
        }
    }

    // Safety: if rx_buf fills with junk and never decodes, reset it.
    if (rx_len >= sizeof(rx_buf)) {
        rx_len = 0;
        diag_log_msg("cdc: RX buffer reset (overflow with no valid frame)");
    }
}
