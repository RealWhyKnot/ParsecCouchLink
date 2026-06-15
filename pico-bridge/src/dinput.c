#include "dinput.h"

#include <stdbool.h>
#include <string.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "dinput_report.h"
#include "gamepad_state.h"
#include "usb_diag.h"

#define DINPUT_IDLE_REPORT_INTERVAL_MS 8u

static dinput_report_t last_sent;
static bool have_last_sent;
static uint32_t last_report_ms;

static void read_gamepad_state(gamepad_state_t *out) {
    out->buttons = g_gamepad_state.buttons;
    out->left_trigger = g_gamepad_state.left_trigger;
    out->right_trigger = g_gamepad_state.right_trigger;
    out->left_x = g_gamepad_state.left_x;
    out->left_y = g_gamepad_state.left_y;
    out->right_x = g_gamepad_state.right_x;
    out->right_y = g_gamepad_state.right_y;
}

static void build_current_report(dinput_report_t *out) {
    gamepad_state_t state;
    read_gamepad_state(&state);
    dinput_build_report(&state, out);
}

void dinput_init(void) {
    memset(&last_sent, 0, sizeof(last_sent));
    have_last_sent = false;
    last_report_ms = 0;
}

void dinput_note_usb_reset(void) {
    have_last_sent = false;
    last_report_ms = 0;
}

void dinput_task(void) {
    if (!tud_mounted())
        return;
    if (!tud_hid_ready())
        return;

    dinput_report_t rep;
    build_current_report(&rep);

    uint32_t now = to_ms_since_boot(get_absolute_time());
    bool changed = !have_last_sent || memcmp(&rep, &last_sent, sizeof(rep)) != 0;
    if (!changed && (uint32_t)(now - last_report_ms) < DINPUT_IDLE_REPORT_INTERVAL_MS) {
        return;
    }

    if (tud_hid_report(DINPUT_REPORT_ID, &rep.bytes[1], DINPUT_PAYLOAD_REPORT_LEN)) {
        usb_diag_note_xinput_in_queued(DINPUT_WIRE_REPORT_LEN);
        last_sent = rep;
        have_last_sent = true;
        last_report_ms = now;
    }
}

uint16_t dinput_get_report_payload(uint8_t *buffer, uint16_t reqlen) {
    if (reqlen < DINPUT_PAYLOAD_REPORT_LEN)
        return 0;

    dinput_report_t rep;
    build_current_report(&rep);
    memcpy(buffer, &rep.bytes[1], DINPUT_PAYLOAD_REPORT_LEN);
    return DINPUT_PAYLOAD_REPORT_LEN;
}
