#include <assert.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>

#include "pico/stdlib.h"
#include "usb_diag.h"

static uint32_t fake_now_ms;
static bool fake_mounted;
static bool fake_suspended;
static unsigned log_count;

absolute_time_t get_absolute_time(void) {
    return fake_now_ms;
}

uint32_t to_ms_since_boot(absolute_time_t t) {
    return t;
}

bool tud_mounted(void) {
    return fake_mounted;
}

bool tud_suspended(void) {
    return fake_suspended;
}

void diag_log_msg(const char *msg) {
    (void)msg;
    log_count++;
}

void diag_log_printf(const char *fmt, ...) {
    (void)fmt;
    va_list ap;
    va_start(ap, fmt);
    va_end(ap);
    log_count++;
}

static void reset_fake_env(void) {
    fake_now_ms = 1000;
    fake_mounted = true;
    fake_suspended = false;
    log_count = 0;
    usb_diag_init();
}

static void block_reasons_are_counted_and_snapshotted(void) {
    reset_fake_env();

    usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_MOUNTED, 20, 0);
    fake_now_ms += 10;
    usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, 20, 4);
    fake_now_ms += 10;
    usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_NOT_READY, 20, 0);
    fake_now_ms += 10;
    usb_diag_note_xinput_in_blocked(USB_DIAG_IN_BLOCKED_SHORT_WRITE, 20, 3);
    usb_diag_note_xinput_in_blocked(99, 20, 1);
    usb_diag_note_xinput_in_idle_suppressed();
    usb_diag_note_xinput_in_idle_suppressed();

    usb_diag_snapshot_t snap;
    usb_diag_snapshot(&snap);

    assert(log_count == 3);
    assert(snap.xinput_in_blocked_not_mounted_count == 1);
    assert(snap.xinput_in_blocked_not_ready_count == 2);
    assert(snap.xinput_in_blocked_short_write_count == 1);
    assert(snap.xinput_in_idle_suppressed_count == 2);
    assert(snap.last_in_blocked_reason == USB_DIAG_IN_BLOCKED_SHORT_WRITE);
    assert(snap.last_in_blocked_want == 20);
    assert(snap.last_in_blocked_got == 3);
    assert(snap.last_in_blocked_ms == 1030);
    assert(snap.mounted);
    assert(!snap.suspended);
}

static void report_activity_still_updates_existing_fields(void) {
    reset_fake_env();
    uint8_t out[] = {0x01, 0x02};

    usb_diag_note_mount();
    fake_now_ms += 1;
    usb_diag_note_xinput_in_queued(20);
    fake_now_ms += 1;
    usb_diag_note_xinput_in_sent(20);
    fake_now_ms += 1;
    usb_diag_note_xinput_out(out, sizeof(out));

    usb_diag_snapshot_t snap;
    usb_diag_snapshot(&snap);

    assert(snap.mount_count == 1);
    assert(snap.xinput_in_queued_count == 1);
    assert(snap.xinput_in_sent_count == 1);
    assert(snap.xinput_out_count == 1);
    assert(snap.last_out_len == 2);
    assert(snap.last_out_byte0 == 0x01);
    assert(snap.last_out_byte1 == 0x02);
}

int main(void) {
    block_reasons_are_counted_and_snapshotted();
    report_activity_still_updates_existing_fields();
    return 0;
}
