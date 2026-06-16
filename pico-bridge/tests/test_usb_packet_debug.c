#include <assert.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#include "pico/stdlib.h"

#include "boot_mode.h"
#include "usb_packet_debug.h"

static run_persona_t current_persona = RUN_PERSONA_DEBUG;
static uint32_t fake_ms;
static char last_line[512];
static unsigned line_count;

absolute_time_t get_absolute_time(void) {
    return fake_ms;
}

uint32_t to_ms_since_boot(absolute_time_t t) {
    return t;
}

run_persona_t boot_mode_run_persona(void) {
    return current_persona;
}

void diag_log_printf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(last_line, sizeof(last_line), fmt, ap);
    va_end(ap);
    line_count++;
}

static void require_contains(const char *needle) {
    if (strstr(last_line, needle) == NULL) {
        printf("missing `%s` in `%s`\n", needle, last_line);
        assert(false);
    }
}

static void normal_persona_does_not_log_packets(void) {
    uint8_t data[] = {0x01, 0x02};
    current_persona = RUN_PERSONA_XINPUT;
    line_count = 0;
    usb_packet_debug_note_out("vendor", data, sizeof(data));
    usb_packet_debug_note_in("xinput", data, sizeof(data), true);
    usb_packet_debug_note_setup("vendor-control", 0xC0, 0x20, 0x0102, 0x0304, 0x4000);
    usb_packet_debug_note_control_in("desc-device", data, sizeof(data));
    assert(line_count == 0);
    current_persona = RUN_PERSONA_DEBUG;
}

static void debug_out_packet_logs_hex_payload(void) {
    uint8_t data[] = {0x01, 0x02, 0xA0};
    fake_ms = 10;
    line_count = 0;
    usb_packet_debug_note_out("vendor", data, sizeof(data));
    assert(line_count == 1);
    require_contains("dir=out");
    require_contains("src=vendor");
    require_contains("reason=host-out");
    require_contains("data=0102A0");
}

static void debug_in_packets_keep_changed_and_summarize_idle_suppression(void) {
    uint8_t data[] = {0x00, 0x14, 0x00, 0x00};

    fake_ms = 20;
    line_count = 0;
    usb_packet_debug_note_in("xinput", data, sizeof(data), true);
    assert(line_count == 1);
    require_contains("dir=in");
    require_contains("reason=changed");
    require_contains("suppressed=0");

    fake_ms = 30;
    usb_packet_debug_note_in("xinput", data, sizeof(data), false);
    assert(line_count == 2);
    require_contains("reason=idle-sample");
    require_contains("suppressed=0");

    fake_ms = 500;
    usb_packet_debug_note_in("xinput", data, sizeof(data), false);
    assert(line_count == 2);

    fake_ms = 1100;
    usb_packet_debug_note_in("xinput", data, sizeof(data), false);
    assert(line_count == 3);
    require_contains("reason=idle-sample");
    require_contains("suppressed=1");
}

static void debug_control_setup_logs_wire_bytes(void) {
    fake_ms = 1200;
    line_count = 0;
    usb_packet_debug_note_setup("vendor-control", 0xC0, 0x20, 0x0102, 0x0304, 0x4000);
    assert(line_count == 1);
    require_contains("dir=setup");
    require_contains("src=vendor-control");
    require_contains("len=8");
    require_contains("reason=control-setup");
    require_contains("data=C020020104030040");
}

static void debug_control_in_logs_reply_payload(void) {
    uint8_t data[] = {0x12, 0x01, 0x00, 0x02};
    fake_ms = 1210;
    line_count = 0;
    usb_packet_debug_note_control_in("desc-device", data, sizeof(data));
    assert(line_count == 1);
    require_contains("dir=control-in");
    require_contains("src=desc-device");
    require_contains("reason=control-reply");
    require_contains("data=12010002");
}

int main(void) {
    normal_persona_does_not_log_packets();
    debug_out_packet_logs_hex_payload();
    debug_in_packets_keep_changed_and_summarize_idle_suppression();
    debug_control_setup_logs_wire_bytes();
    debug_control_in_logs_reply_payload();
    puts("usb_packet_debug tests passed");
    return 0;
}
