#include "xbone.h"

#include <stdbool.h>
#include <stddef.h>
#include <string.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "gamepad_state.h"
#include "usb_diag.h"

#define XBONE_ANNOUNCE_DELAY_MS 500u
#define XBONE_IDLE_REPORT_INTERVAL_MS 8u
#define XBONE_KEEPALIVE_INTERVAL_MS 15000u

typedef struct __attribute__((packed)) {
    uint8_t command;
    uint8_t flags;
    uint8_t sequence;
    uint8_t length;
    uint8_t buttons0;
    uint8_t buttons1;
    uint16_t left_trigger;
    uint16_t right_trigger;
    int16_t left_x;
    int16_t left_y;
    int16_t right_x;
    int16_t right_y;
    uint8_t reserved[18];
} xbone_input_report_t;

_Static_assert(sizeof(xbone_input_report_t) == 36, "xbone input report must be 36 bytes");

static const uint8_t xbone_announce_data[] = {
    0x00, 0x2a, 0x00, 0xff, 0xff, 0xff, 0x00, 0x00, 0xdf, 0x33, 0x14, 0x00, 0x01, 0x00,
    0x01, 0x00, 0x17, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00,
};

static xbone_input_report_t last_sent;
static bool have_last_sent;
static bool announce_sent;
static uint32_t mount_ms;
static uint32_t last_report_ms;
static uint32_t last_keepalive_ms;
static uint8_t input_sequence;
static uint8_t keepalive_sequence;

static uint16_t trigger10(uint8_t value) {
    return (uint16_t)(((uint32_t)value * 1023u + 127u) / 255u);
}

static void put_header(uint8_t *packet, uint8_t command, uint8_t flags, uint8_t sequence,
                       uint8_t length) {
    packet[0] = command;
    packet[1] = flags;
    packet[2] = sequence;
    packet[3] = length;
}

static bool vendor_send(uint8_t const *packet, uint16_t len) {
    if (!tud_mounted()) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_MOUNTED, len, 0);
        return false;
    }
    uint32_t available = tud_vendor_write_available();
    if (available < len) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, len, (uint16_t)available);
        return false;
    }
    uint32_t wrote = tud_vendor_write(packet, len);
    if (wrote == len) {
        usb_diag_note_xinput_in_queued(wrote);
        tud_vendor_write_flush();
        return true;
    }
    usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_SHORT_WRITE, len, (uint16_t)wrote);
    return false;
}

static bool send_announce(uint32_t now) {
    uint8_t packet[sizeof(xbone_announce_data) + 4];
    put_header(packet, 0x02, 0x20, 1, sizeof(xbone_announce_data));
    memcpy(&packet[4], xbone_announce_data, sizeof(xbone_announce_data));
    packet[7] = (uint8_t)(now & 0xFFu);
    packet[8] = (uint8_t)((now >> 8) & 0xFFu);
    packet[9] = (uint8_t)((now >> 16) & 0xFFu);
    return vendor_send(packet, sizeof(packet));
}

static bool send_keepalive(void) {
    uint8_t packet[8];
    put_header(packet, 0x03, 0x20, keepalive_sequence, 4);
    packet[4] = 0x80;
    packet[5] = 0x00;
    packet[6] = 0x00;
    packet[7] = 0x00;
    if (vendor_send(packet, sizeof(packet))) {
        keepalive_sequence++;
        if (keepalive_sequence == 0)
            keepalive_sequence = 1;
        return true;
    }
    return false;
}

static void build_input_report(xbone_input_report_t *out) {
    memset(out, 0, sizeof(*out));
    out->command = 0x20;
    out->flags = 0x00;
    out->sequence = input_sequence;
    out->length = sizeof(*out) - 4u;

    uint16_t buttons = g_gamepad_state.buttons;
    if (buttons & 0x0010u)
        out->buttons0 |= 1u << 2; // Menu / Start
    if (buttons & 0x0020u)
        out->buttons0 |= 1u << 3; // View / Back
    if (buttons & 0x1000u)
        out->buttons0 |= 1u << 4; // A
    if (buttons & 0x2000u)
        out->buttons0 |= 1u << 5; // B
    if (buttons & 0x4000u)
        out->buttons0 |= 1u << 6; // X
    if (buttons & 0x8000u)
        out->buttons0 |= 1u << 7; // Y

    if (buttons & 0x0001u)
        out->buttons1 |= 1u << 0;
    if (buttons & 0x0002u)
        out->buttons1 |= 1u << 1;
    if (buttons & 0x0004u)
        out->buttons1 |= 1u << 2;
    if (buttons & 0x0008u)
        out->buttons1 |= 1u << 3;
    if (buttons & 0x0100u)
        out->buttons1 |= 1u << 4;
    if (buttons & 0x0200u)
        out->buttons1 |= 1u << 5;
    if (buttons & 0x0040u)
        out->buttons1 |= 1u << 6;
    if (buttons & 0x0080u)
        out->buttons1 |= 1u << 7;

    out->left_trigger = trigger10(g_gamepad_state.left_trigger);
    out->right_trigger = trigger10(g_gamepad_state.right_trigger);
    out->left_x = g_gamepad_state.left_x;
    out->left_y = g_gamepad_state.left_y;
    out->right_x = g_gamepad_state.right_x;
    out->right_y = g_gamepad_state.right_y;
}

void xbone_init(void) {
    memset(&last_sent, 0, sizeof(last_sent));
    have_last_sent = false;
    announce_sent = false;
    mount_ms = to_ms_since_boot(get_absolute_time());
    last_report_ms = 0;
    last_keepalive_ms = 0;
    input_sequence = 1;
    keepalive_sequence = 1;
}

void xbone_note_usb_reset(void) {
    have_last_sent = false;
    announce_sent = false;
    mount_ms = to_ms_since_boot(get_absolute_time());
    last_report_ms = 0;
    last_keepalive_ms = 0;
    input_sequence = 1;
    keepalive_sequence = 1;
}

void xbone_task(void) {
    if (!tud_mounted())
        return;

    uint32_t now = to_ms_since_boot(get_absolute_time());
    if (!announce_sent && (uint32_t)(now - mount_ms) >= XBONE_ANNOUNCE_DELAY_MS) {
        announce_sent = send_announce(now);
        if (announce_sent)
            return;
    }

    if ((uint32_t)(now - last_keepalive_ms) >= XBONE_KEEPALIVE_INTERVAL_MS) {
        if (send_keepalive()) {
            last_keepalive_ms = now;
            return;
        }
    }

    xbone_input_report_t report;
    build_input_report(&report);
    bool changed =
        !have_last_sent || memcmp(&report.buttons0, &last_sent.buttons0,
                                  sizeof(report) - offsetof(xbone_input_report_t, buttons0)) != 0;
    if (!changed && (uint32_t)(now - last_report_ms) < XBONE_IDLE_REPORT_INTERVAL_MS) {
        usb_diag_note_xinput_in_idle_suppressed();
        return;
    }

    input_sequence++;
    if (input_sequence == 0)
        input_sequence = 1;
    report.sequence = input_sequence;
    if (vendor_send((uint8_t const *)&report, sizeof(report))) {
        last_sent = report;
        have_last_sent = true;
        last_report_ms = now;
    }
}

void xbone_on_out(uint8_t const *buffer, uint16_t len) {
    (void)buffer;
    (void)len;
}
