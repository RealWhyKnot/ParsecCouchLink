#include "cdc_handlers.h"

#include <string.h>

#include "tusb.h"
#include "pico/unique_id.h"

#include "flash_creds.h"
#include "diag_log.h"
#include "version.h"

#define RX_BUFFER_SIZE  (CDC_MAX_FRAME * 2)
#define TX_BUFFER_SIZE  (CDC_MAX_FRAME * 2)

static uint8_t rx_buf[RX_BUFFER_SIZE];
static size_t  rx_len;
static bool    reboot_pending;
static bool    bootsel_pending;

static void send_nack(uint8_t seq, uint8_t code, uint8_t detail) {
    uint8_t payload[2] = { code, detail };
    uint8_t frame[CDC_MAX_FRAME];
    size_t n = cdc_encode(CDC_RSP_NACK, seq, payload, 2, frame, sizeof(frame));
    if (n) tud_cdc_write(frame, n);
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
    payload[5] = (have ? CDC_HELLO_FLAG_CREDS_PRESENT : 0)
               | CDC_HELLO_FLAG_RUN_MODE_OK;
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

static size_t handle_set_wifi(const cdc_frame_view_t *req,
                              uint8_t *reply, size_t cap) {
    if (req->payload_len < 2) {
        uint8_t err[2] = { CDC_ERR_BAD_LENGTH, 0 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    uint8_t ssid_len = req->payload[0];
    if (ssid_len == 0 || ssid_len > FLASH_CREDS_SSID_MAX
        || (size_t)1 + ssid_len + 1 > req->payload_len) {
        uint8_t err[2] = { CDC_ERR_BAD_LENGTH, 1 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    uint8_t pass_len = req->payload[1 + ssid_len];
    if (pass_len > FLASH_CREDS_PASS_MAX
        || (size_t)1 + ssid_len + 1 + pass_len != req->payload_len) {
        uint8_t err[2] = { CDC_ERR_BAD_LENGTH, 2 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }

    flash_creds_t rec;
    memset(&rec, 0, sizeof(rec));
    rec.ssid_len = ssid_len;
    rec.pass_len = pass_len;
    memcpy(rec.ssid, &req->payload[1], ssid_len);
    memcpy(rec.password, &req->payload[1 + ssid_len + 1], pass_len);

    // Preserve device_name across re-provisioning if there was one.
    flash_creds_t prev;
    if (flash_creds_load(&prev)) {
        rec.name_len = prev.name_len;
        memcpy(rec.device_name, prev.device_name, FLASH_CREDS_NAME_MAX);
    }

    int rc = flash_creds_store(&rec);
    // Zero the local copy of the password ASAP.
    memset(&rec, 0, sizeof(rec));

    if (rc == -2) {
        uint8_t err[2] = { CDC_ERR_FLASH_WRITE_FAIL, 0 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    } else if (rc == -3) {
        uint8_t err[2] = { CDC_ERR_FLASH_VERIFY_FAIL, 0 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    } else if (rc < 0) {
        uint8_t err[2] = { CDC_ERR_INTERNAL, (uint8_t)(-rc) };
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
    return cdc_encode(CDC_RSP_DEVICE_NAME, seq, rec.device_name, rec.name_len,
                      reply, cap);
}

static size_t handle_device_name_set(const cdc_frame_view_t *req,
                                     uint8_t *reply, size_t cap) {
    if (req->payload_len > FLASH_CREDS_NAME_MAX) {
        uint8_t err[2] = { CDC_ERR_BAD_LENGTH, 0 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    flash_creds_t rec;
    if (!flash_creds_load(&rec)) {
        // No creds yet; can't store a name on its own.
        uint8_t err[2] = { CDC_ERR_INTERNAL, 1 };
        return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, cap);
    }
    rec.name_len = (uint8_t)req->payload_len;
    memset(rec.device_name, 0, FLASH_CREDS_NAME_MAX);
    if (req->payload_len) memcpy(rec.device_name, req->payload, req->payload_len);
    int rc = flash_creds_store(&rec);
    memset(&rec, 0, sizeof(rec));
    if (rc != 0) {
        uint8_t err[2] = { CDC_ERR_FLASH_WRITE_FAIL, 0 };
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
    uint8_t buf[CDC_MAX_PAYLOAD];
    if (sizeof(buf) < 4) return 0;
    uint32_t lost = 0;
    size_t   tail_cap = sizeof(buf) - 4;
    size_t   n = diag_log_snapshot(buf + 4, tail_cap, &lost);
    buf[0] = (uint8_t)(lost & 0xFFu);
    buf[1] = (uint8_t)((lost >> 8) & 0xFFu);
    buf[2] = (uint8_t)((lost >> 16) & 0xFFu);
    buf[3] = (uint8_t)((lost >> 24) & 0xFFu);
    return cdc_encode(CDC_RSP_LOG_BUFFER, seq, buf, 4 + n, reply, cap);
}

static size_t handle_self_test(uint8_t seq, uint8_t *reply, size_t cap) {
    // Lightweight; runs entirely in software, no Wi-Fi.
    char buf[160];
    bool flash_ok = true;
    bool ok = true;
    flash_creds_t rec;
    bool have = flash_creds_load(&rec);
    int n = snprintf(buf, sizeof(buf),
                     "result=%s flash=%s creds=%s fw=%s board=0x%02X",
                     ok ? "pass" : "fail",
                     flash_ok ? "ok" : "bad",
                     have ? "present" : "absent",
                     PICO_BRIDGE_FW_VERSION_STRING, PICO_BRIDGE_BOARD_TYPE);
    if (n < 0) n = 0;
    if ((size_t)n > sizeof(buf)) n = sizeof(buf);
    uint8_t payload[1 + sizeof(buf)];
    payload[0] = ok ? 0 : 1;
    memcpy(&payload[1], buf, n);
    return cdc_encode(CDC_RSP_SELF_TEST, seq, payload, 1 + n, reply, cap);
}

size_t cdc_dispatch(const cdc_frame_view_t *req, uint8_t *reply, size_t reply_cap) {
    switch (req->command) {
        case CDC_CMD_HELLO:           return handle_hello(req->seq, reply, reply_cap);
        case CDC_CMD_GET_STATUS:      return handle_hello(req->seq, reply, reply_cap); // for now, same body
        case CDC_CMD_SET_WIFI:        return handle_set_wifi(req, reply, reply_cap);
        case CDC_CMD_REBOOT_TO_RUN:   return handle_reboot(req->seq, reply, reply_cap);
        case CDC_CMD_SELF_TEST:       return handle_self_test(req->seq, reply, reply_cap);
        case CDC_CMD_GET_DEVICE_NAME: return handle_device_name_get(req->seq, reply, reply_cap);
        case CDC_CMD_SET_DEVICE_NAME: return handle_device_name_set(req, reply, reply_cap);
        case CDC_CMD_GET_UNIQUE_ID:   return handle_unique_id(req->seq, reply, reply_cap);
        case CDC_CMD_GET_LOG_BUFFER:  return handle_log_buffer(req->seq, reply, reply_cap);
        case CDC_CMD_REBOOT_TO_BOOTSEL:
                                      return handle_reboot_to_bootsel(req->seq, reply, reply_cap);
        default: {
            uint8_t err[2] = { CDC_ERR_UNKNOWN_COMMAND, req->command };
            return cdc_encode(CDC_RSP_NACK, req->seq, err, 2, reply, reply_cap);
        }
    }
}

void cdc_handlers_init(void) {
    rx_len = 0;
    reboot_pending = false;
    bootsel_pending = false;
}

bool cdc_handlers_reboot_pending(void) {
    // Reboot only once the TX FIFO is fully drained -- meaning the ACK
    // frame we just queued has actually left the device. tud_cdc_write_available()
    // returns the number of FREE bytes, so the FIFO is empty when it
    // equals the configured TX bufsize.
    return reboot_pending
        && (tud_cdc_write_available() == CFG_TUD_CDC_TX_BUFSIZE);
}

bool cdc_handlers_bootsel_pending(void) {
    return bootsel_pending
        && (tud_cdc_write_available() == CFG_TUD_CDC_TX_BUFSIZE);
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
        if (want > 64) want = 64;
        rx_len += tud_cdc_read(&rx_buf[rx_len], want);
    }

    // Try to decode as many complete frames as possible.
    while (rx_len > 0) {
        cdc_frame_view_t view;
        size_t consumed = 0;
        cdc_decode_status_t st = cdc_try_decode(rx_buf, rx_len, &view, &consumed);
        if (st == CDC_DECODE_NEED_MORE) break;

        if (st == CDC_DECODE_OK) {
            diag_log_printf("cdc: dispatching cmd=0x%02X seq=%u payload=%u bytes",
                            (unsigned)view.command, (unsigned)view.seq,
                            (unsigned)view.payload_len);
            uint8_t reply[CDC_MAX_FRAME];
            size_t n = cdc_dispatch(&view, reply, sizeof(reply));
            if (n > 0) tud_cdc_write(reply, n);
            tud_cdc_write_flush();
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
