#include "xinput.h"

#include <string.h>

#include "tusb.h"

#include "gamepad_state.h"
#include "usb_diag.h"

// 20-byte XInput IN report layout matches the Microsoft wired Xbox 360
// pad exactly. The bitmask layout of XINPUT_GAMEPAD.wButtons is also
// directly compatible with the `buttons` field in our shared state, so
// no remapping happens here -- pure byte copy from g_gamepad_state into
// the 20-byte payload.
typedef struct __attribute__((packed)) {
    uint8_t  rid;            // 0x00
    uint8_t  rsize;          // 0x14 (20)
    uint16_t wButtons;       // 16-bit bitmask
    uint8_t  bLeftTrigger;
    uint8_t  bRightTrigger;
    int16_t  sThumbLX;
    int16_t  sThumbLY;
    int16_t  sThumbRX;
    int16_t  sThumbRY;
    uint8_t  reserved[6];
} xinput_report_t;

_Static_assert(sizeof(xinput_report_t) == 20, "xinput_report_t must be 20 bytes");

static xinput_report_t last_sent;
static bool have_last_sent;

void xinput_init(void) {
    memset(&last_sent, 0, sizeof(last_sent));
    last_sent.rid = 0x00;
    last_sent.rsize = 0x14;
    have_last_sent = false;
}

void xinput_note_usb_reset(void) {
    have_last_sent = false;
}

static void build_report(xinput_report_t *out) {
    out->rid          = 0x00;
    out->rsize        = 0x14;
    out->wButtons     = g_gamepad_state.buttons;
    out->bLeftTrigger = g_gamepad_state.left_trigger;
    out->bRightTrigger= g_gamepad_state.right_trigger;
    out->sThumbLX     = g_gamepad_state.left_x;
    out->sThumbLY     = g_gamepad_state.left_y;
    out->sThumbRX     = g_gamepad_state.right_x;
    out->sThumbRY     = g_gamepad_state.right_y;
    memset(out->reserved, 0, sizeof(out->reserved));
}

void xinput_task(void) {
    if (!tud_mounted()) return;
    if (tud_vendor_write_available() < sizeof(xinput_report_t)) return;

    xinput_report_t rep;
    build_report(&rep);

    // Skip identical re-sends to reduce bus noise; the adapter will keep
    // polling the same state anyway because the endpoint is interrupt-IN.
    if (have_last_sent && memcmp(&rep, &last_sent, sizeof(rep)) == 0) {
        return;
    }
    uint32_t wrote = tud_vendor_write(&rep, sizeof(rep));
    if (wrote == sizeof(rep)) {
        usb_diag_note_xinput_in_queued(wrote);
        tud_vendor_write_flush();
        last_sent = rep;
        have_last_sent = true;
    }
}
