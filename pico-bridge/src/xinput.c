#include "xinput.h"

#include <string.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "gamepad_state.h"
#include "usb_diag.h"
#include "usb_packet_debug.h"

#define XINPUT_IDLE_REPORT_INTERVAL_MS 8u

// 20-byte XInput IN report layout matches the Microsoft wired Xbox 360
// pad exactly. The bitmask layout of XINPUT_GAMEPAD.wButtons is also
// directly compatible with the `buttons` field in our shared state, so
// no remapping happens here -- pure byte copy from g_gamepad_state into
// the 20-byte payload.
typedef struct __attribute__((packed)) {
    uint8_t rid;       // 0x00
    uint8_t rsize;     // 0x14 (20)
    uint16_t wButtons; // 16-bit bitmask
    uint8_t bLeftTrigger;
    uint8_t bRightTrigger;
    int16_t sThumbLX;
    int16_t sThumbLY;
    int16_t sThumbRX;
    int16_t sThumbRY;
    uint8_t reserved[6];
} xinput_report_t;

_Static_assert(sizeof(xinput_report_t) == 20, "xinput_report_t must be 20 bytes");

static xinput_report_t last_sent;
static bool have_last_sent;
static uint32_t last_report_ms;

void xinput_init(void) {
    memset(&last_sent, 0, sizeof(last_sent));
    last_sent.rid = 0x00;
    last_sent.rsize = 0x14;
    have_last_sent = false;
    last_report_ms = 0;
}

void xinput_note_usb_reset(void) {
    have_last_sent = false;
    last_report_ms = 0;
}

static void build_report(xinput_report_t *out) {
    out->rid = 0x00;
    out->rsize = 0x14;
    out->wButtons = g_gamepad_state.buttons;
    out->bLeftTrigger = g_gamepad_state.left_trigger;
    out->bRightTrigger = g_gamepad_state.right_trigger;
    out->sThumbLX = g_gamepad_state.left_x;
    out->sThumbLY = g_gamepad_state.left_y;
    out->sThumbRX = g_gamepad_state.right_x;
    out->sThumbRY = g_gamepad_state.right_y;
    memset(out->reserved, 0, sizeof(out->reserved));
}

void xinput_task(void) {
    if (!tud_mounted()) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_MOUNTED,
                                        (uint16_t)sizeof(xinput_report_t), 0);
        return;
    }
    uint32_t available = tud_vendor_write_available();
    if (available < sizeof(xinput_report_t)) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY,
                                        (uint16_t)sizeof(xinput_report_t), (uint16_t)available);
        return;
    }

    xinput_report_t rep;
    build_report(&rep);

    uint32_t now = to_ms_since_boot(get_absolute_time());
    bool changed = !have_last_sent || memcmp(&rep, &last_sent, sizeof(rep)) != 0;
    if (!changed && (uint32_t)(now - last_report_ms) < XINPUT_IDLE_REPORT_INTERVAL_MS) {
        usb_diag_note_xinput_in_idle_suppressed();
        return;
    }
    uint32_t wrote = tud_vendor_write(&rep, sizeof(rep));
    if (wrote == sizeof(rep)) {
        usb_diag_note_xinput_in_queued(wrote);
        usb_packet_debug_note_in("xinput", (uint8_t const *)&rep, (uint16_t)sizeof(rep), changed);
        tud_vendor_write_flush();
        last_sent = rep;
        have_last_sent = true;
        last_report_ms = now;
    } else {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_SHORT_WRITE,
                                        (uint16_t)sizeof(xinput_report_t), (uint16_t)wrote);
    }
}
