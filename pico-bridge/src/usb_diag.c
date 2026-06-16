#include "usb_diag.h"

#include <string.h>

#include "diag_log.h"
#include "pico/stdlib.h"
#include "tusb.h"

static usb_diag_snapshot_t state;
static bool logged_first_in_queued;
static bool logged_first_in_sent;
static bool logged_first_out;

static uint32_t now_ms(void) {
    return to_ms_since_boot(get_absolute_time());
}

void usb_diag_init(void) {
    memset(&state, 0, sizeof(state));
    logged_first_in_queued = false;
    logged_first_in_sent = false;
    logged_first_out = false;
}

void usb_diag_note_device_descriptor(void) {
    state.device_desc_count++;
}

void usb_diag_note_configuration_descriptor(void) {
    state.config_desc_count++;
}

void usb_diag_note_mount(void) {
    state.mounted = true;
    state.suspended = false;
    state.mount_count++;
    state.last_mount_ms = now_ms();
}

void usb_diag_note_umount(void) {
    state.mounted = false;
    state.suspended = false;
    state.umount_count++;
    state.last_umount_ms = now_ms();
}

void usb_diag_note_suspend(void) {
    state.suspended = true;
    state.suspend_count++;
}

void usb_diag_note_resume(void) {
    state.suspended = false;
    state.resume_count++;
}

void usb_diag_note_xinput_in_queued(uint32_t bytes) {
    if (bytes == 0)
        return;
    state.xinput_in_queued_count++;
    state.last_in_queued_ms = now_ms();
    if (!logged_first_in_queued) {
        logged_first_in_queued = true;
        diag_log_printf("usb: first IN report queued (bytes=%u)", (unsigned)bytes);
    }
}

void usb_diag_note_xinput_in_sent(uint32_t bytes) {
    if (bytes == 0)
        return;
    state.xinput_in_sent_count++;
    state.last_in_sent_ms = now_ms();
    if (!logged_first_in_sent) {
        logged_first_in_sent = true;
        diag_log_printf("usb: first IN report accepted by host (bytes=%u)", (unsigned)bytes);
    }
}

void usb_diag_note_xinput_out(uint8_t const *buffer, uint16_t len) {
    state.xinput_out_count++;
    state.last_out_ms = now_ms();
    state.last_out_len = (len > 255u) ? 255u : (uint8_t)len;
    state.last_out_byte0 = (len > 0 && buffer) ? buffer[0] : 0;
    state.last_out_byte1 = (len > 1 && buffer) ? buffer[1] : 0;
    if (!logged_first_out) {
        logged_first_out = true;
        diag_log_printf("usb: first host OUT report (bytes=%u)", (unsigned)len);
    }
}

void usb_diag_snapshot(usb_diag_snapshot_t *out) {
    if (!out)
        return;
    *out = state;
    out->now_ms = now_ms();
    out->mounted = tud_mounted();
    out->suspended = tud_suspended();
}
