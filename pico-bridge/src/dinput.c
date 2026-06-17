#include "dinput.h"

#include <stdbool.h>
#include <string.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "boot_mode.h"
#include "dinput_report.h"
#include "gamepad_state.h"
#include "usb_diag.h"

#define DINPUT_IDLE_REPORT_INTERVAL_MS 8u

static dinput_report_t last_sent;
static bool have_last_sent;
static uint32_t last_report_ms;
static uint8_t ps4_report_counter;
static uint8_t ps3_feature_ef_byte;

static const uint8_t ps3_feature_01[] = {
    0x01, 0x04, 0x00, 0x0b, 0x0c, 0x01, 0x02, 0x18, 0x18, 0x18, 0x18, 0x09, 0x0a, 0x10, 0x11, 0x12,
    0x13, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x02, 0x02, 0x02, 0x02, 0x00, 0x00, 0x00, 0x04, 0x04,
    0x04, 0x04, 0x00, 0x00, 0x04, 0x00, 0x01, 0x02, 0x07, 0x00, 0x17, 0x00, 0x00, 0x00, 0x00, 0x00,
};

static const uint8_t ps3_feature_ef[] = {
    0xef, 0x04, 0x00, 0x0b, 0x03, 0x01, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0xff, 0x01, 0xff, 0x01, 0xff, 0x01, 0xff, 0x01, 0xff, 0x01, 0xff, 0x01, 0xff, 0x01, 0xff,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06,
};

static const uint8_t ps3_feature_f2[] = {
    0xff, 0xff, 0x00, 0x20, 0x40, 0xce, 0x21, 0x43, 0x65,
    0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x00,
};

static const uint8_t ps3_feature_f7[] = {
    0x02, 0x01, 0xf8, 0x02, 0xe2, 0x01, 0x05, 0xff,
};

static const uint8_t ps3_feature_f8[] = {
    0x01,
};

static const uint8_t ps4_feature_02[] = {
    0xfe, 0xff, 0x0e, 0x00, 0x04, 0x00, 0xd4, 0x22, 0x2a, 0xdd, 0xbb, 0x22,
    0x5e, 0xdd, 0x81, 0x22, 0x84, 0xdd, 0x1c, 0x02, 0x1c, 0x02, 0x85, 0x1f,
    0xb0, 0xe0, 0xc6, 0x20, 0xb5, 0xe0, 0xb1, 0x20, 0x83, 0xdf, 0x0c, 0x00,
};

static const uint8_t ps4_feature_03[] = {
    0x21, 0x27, 0x04, 0xcf, 0x00, 0x2c, 0x56, 0x08, 0x00, 0x3d, 0x00, 0xe8, 0x03, 0x04, 0x00, 0xff,
    0x7f, 0x0d, 0x0d, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x84, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
};

static const uint8_t ps4_feature_12[] = {
    0x00, 0x25, 0x00, 0x12, 0x34, 0x56, 0x08, 0x25, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
};

static const uint8_t ps4_feature_a3[] = {
    0x4a, 0x75, 0x6e, 0x20, 0x20, 0x39, 0x20, 0x32, 0x30, 0x31, 0x37, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x31, 0x32, 0x3a, 0x33, 0x36, 0x3a, 0x34, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x08, 0xb4, 0x01, 0x00, 0x00, 0x00, 0x07, 0xa0, 0x10, 0x20, 0x00, 0xa0, 0x02, 0x00,
};

static const uint8_t ps4_feature_f2[] = {
    0x01, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x55, 0xf8, 0x2a,
};

static const uint8_t ps4_feature_f3[] = {
    0x00, 0x38, 0x38, 0x00, 0x00, 0x00, 0x00,
};

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
    if (boot_mode_run_persona() == RUN_PERSONA_GENERIC_HID)
        dinput_build_generic_hid_report(&state, out);
    else if (boot_mode_run_persona() == RUN_PERSONA_PS4)
        dinput_build_ps4_report(&state, ps4_report_counter, out);
    else
        dinput_build_ps3_report(&state, out);
}

static uint16_t expected_wire_report_len(void) {
    if (boot_mode_run_persona() == RUN_PERSONA_GENERIC_HID)
        return DINPUT_GENERIC_HID_WIRE_REPORT_LEN;
    return (boot_mode_run_persona() == RUN_PERSONA_PS4) ? DINPUT_PS4_WIRE_REPORT_LEN
                                                        : DINPUT_PS3_WIRE_REPORT_LEN;
}

void dinput_init(void) {
    memset(&last_sent, 0, sizeof(last_sent));
    have_last_sent = false;
    last_report_ms = 0;
    ps4_report_counter = 0;
    ps3_feature_ef_byte = 0xa0;
}

void dinput_note_usb_reset(void) {
    have_last_sent = false;
    last_report_ms = 0;
}

void dinput_task(void) {
    uint16_t want = expected_wire_report_len();
    if (!tud_mounted()) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_MOUNTED, want, 0);
        return;
    }
    if (!tud_hid_ready()) {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, want, 0);
        return;
    }

    dinput_report_t rep;
    build_current_report(&rep);

    uint32_t now = to_ms_since_boot(get_absolute_time());
    bool changed = !have_last_sent || memcmp(&rep, &last_sent, sizeof(rep)) != 0;
    if (!changed && (uint32_t)(now - last_report_ms) < DINPUT_IDLE_REPORT_INTERVAL_MS) {
        usb_diag_note_xinput_in_idle_suppressed();
        return;
    }

    uint8_t const *payload = rep.report_id == 0 ? rep.bytes : &rep.bytes[1];
    uint8_t payload_len = rep.report_id == 0 ? rep.len : (uint8_t)(rep.len - 1u);
    if (tud_hid_report(rep.report_id, payload, payload_len)) {
        usb_diag_note_xinput_in_queued(rep.len);
        last_sent = rep;
        have_last_sent = true;
        last_report_ms = now;
        if (boot_mode_run_persona() == RUN_PERSONA_PS4)
            ps4_report_counter = (uint8_t)((ps4_report_counter + 1u) & 0x3Fu);
    } else {
        usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, rep.len, 0);
    }
}

static uint16_t copy_report(uint8_t *buffer, uint16_t reqlen, const uint8_t *src, uint16_t len) {
    if (reqlen == 0)
        return 0;
    uint16_t copy = reqlen < len ? reqlen : len;
    memcpy(buffer, src, copy);
    return copy;
}

uint16_t dinput_get_report_payload(uint8_t report_id, uint8_t report_type, uint8_t *buffer,
                                   uint16_t reqlen) {
    dinput_report_t rep;
    run_persona_t persona = boot_mode_run_persona();
    if (report_type == HID_REPORT_TYPE_INPUT) {
        build_current_report(&rep);
        if (report_id != rep.report_id)
            return 0;
        if (rep.report_id == 0)
            return copy_report(buffer, reqlen, rep.bytes, rep.len);
        return copy_report(buffer, reqlen, &rep.bytes[1], (uint16_t)(rep.len - 1u));
    }
    if (report_type != HID_REPORT_TYPE_FEATURE)
        return 0;

    if (persona == RUN_PERSONA_PS3) {
        if (report_id == 0x01)
            return copy_report(buffer, reqlen, ps3_feature_01, sizeof(ps3_feature_01));
        if (report_id == 0xEF) {
            if (reqlen < sizeof(ps3_feature_ef))
                return 0;
            memcpy(buffer, ps3_feature_ef, sizeof(ps3_feature_ef));
            buffer[6] = ps3_feature_ef_byte;
            return sizeof(ps3_feature_ef);
        }
        if (report_id == 0xF2)
            return copy_report(buffer, reqlen, ps3_feature_f2, sizeof(ps3_feature_f2));
        if (report_id == 0xF5) {
            memset(buffer, 0, reqlen);
            uint16_t copy = reqlen < 64 ? reqlen : 64;
            if (copy >= 7)
                memcpy(&buffer[1], &ps3_feature_f2[10], 6);
            return copy;
        }
        if (report_id == 0xF7)
            return copy_report(buffer, reqlen, ps3_feature_f7, sizeof(ps3_feature_f7));
        if (report_id == 0xF8)
            return copy_report(buffer, reqlen, ps3_feature_f8, sizeof(ps3_feature_f8));
        return 0;
    }

    if (persona == RUN_PERSONA_PS4) {
        if (report_id == 0x02)
            return copy_report(buffer, reqlen, ps4_feature_02, sizeof(ps4_feature_02));
        if (report_id == 0x03)
            return copy_report(buffer, reqlen, ps4_feature_03, sizeof(ps4_feature_03));
        if (report_id == 0x12)
            return copy_report(buffer, reqlen, ps4_feature_12, sizeof(ps4_feature_12));
        if (report_id == 0xA3)
            return copy_report(buffer, reqlen, ps4_feature_a3, sizeof(ps4_feature_a3));
        if (report_id == 0xF2)
            return copy_report(buffer, reqlen, ps4_feature_f2, sizeof(ps4_feature_f2));
        if (report_id == 0xF3)
            return copy_report(buffer, reqlen, ps4_feature_f3, sizeof(ps4_feature_f3));
    }
    return 0;
}

void dinput_set_report(uint8_t report_id, uint8_t report_type, uint8_t const *buffer,
                       uint16_t bufsize) {
    if (boot_mode_run_persona() == RUN_PERSONA_PS3 && report_type == HID_REPORT_TYPE_FEATURE &&
        report_id == 0xEF && bufsize > 6) {
        ps3_feature_ef_byte = buffer[6];
    }
}
