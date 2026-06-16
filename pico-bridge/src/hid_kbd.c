#include "hid_kbd.h"

#include <string.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "keyboard_state.h"
#include "usb_diag.h"

// Resend an unchanged report at least this often so a host that drops a
// frame still re-converges, mirroring the XInput idle cadence. Keypresses
// are change-driven, so this is just a keep-fresh floor, not the input
// rate.
#define HID_KBD_IDLE_REPORT_INTERVAL_MS 8u

static uint8_t last_mods;
static uint8_t last_keys[6];
static bool have_last_sent;
static uint32_t last_report_ms;

void hid_kbd_init(void) {
    last_mods = 0;
    memset(last_keys, 0, sizeof(last_keys));
    have_last_sent = false;
    last_report_ms = 0;
}

void hid_kbd_note_usb_reset(void) {
    have_last_sent = false;
    last_report_ms = 0;
}

void hid_kbd_task(void) {
    if (!tud_mounted()) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_MOUNTED, 8, 0);
        return;
    }
    if (!tud_hid_ready()) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, 8, 0);
        return;
    }

    // Read the shared state field-by-field so each access is a volatile
    // load (single-writer net_udp, single-reader here).
    uint8_t mods = g_keyboard_state.modifiers;
    uint8_t keys[6];
    for (int i = 0; i < 6; i++)
        keys[i] = g_keyboard_state.keys[i];

    uint32_t now = to_ms_since_boot(get_absolute_time());
    bool changed =
        !have_last_sent || mods != last_mods || memcmp(keys, last_keys, sizeof(keys)) != 0;
    if (!changed && (uint32_t)(now - last_report_ms) < HID_KBD_IDLE_REPORT_INTERVAL_MS) {
        usb_diag_note_xinput_in_idle_suppressed();
        return;
    }

    // Report ID 0: single boot-keyboard report. tud_hid_keyboard_report
    // packs [modifier, reserved, keycode[6]] into the 8-byte IN report.
    if (tud_hid_keyboard_report(0, mods, keys)) {
        usb_diag_note_xinput_in_queued(8);
        last_mods = mods;
        memcpy(last_keys, keys, sizeof(keys));
        have_last_sent = true;
        last_report_ms = now;
    } else {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, 8, 0);
    }
}
