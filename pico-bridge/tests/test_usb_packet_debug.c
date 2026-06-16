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
static char log_text[32768];
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
    size_t used = strlen(log_text);
    if (used < sizeof(log_text) - 1) {
        snprintf(&log_text[used], sizeof(log_text) - used, "%s\n", last_line);
    }
    line_count++;
}

static void reset_log(void) {
    last_line[0] = 0;
    log_text[0] = 0;
    line_count = 0;
}

static void require_log_contains(const char *needle) {
    if (strstr(log_text, needle) == NULL) {
        printf("missing `%s` in `%s`\n", needle, log_text);
        assert(false);
    }
}

static void normal_persona_does_not_log_packets(void) {
    uint8_t data[] = {0x01, 0x02};
    current_persona = RUN_PERSONA_XINPUT;
    reset_log();
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
    reset_log();
    usb_packet_debug_note_out("vendor", data, sizeof(data));
    assert(line_count == 2);
    require_log_contains("dir=out");
    require_log_contains("src=vendor");
    require_log_contains("reason=host-out");
    require_log_contains("data=0102A0");
    require_log_contains("usb-packet-stats");
    require_log_contains("total=1");
    require_log_contains("out=1");
}

static void debug_in_packets_keep_changed_and_summarize_idle_suppression(void) {
    uint8_t data[] = {0x00, 0x14, 0x00, 0x00};

    fake_ms = 20;
    reset_log();
    usb_packet_debug_note_in("xinput", data, sizeof(data), true);
    assert(line_count == 1);
    require_log_contains("dir=in");
    require_log_contains("reason=changed");
    require_log_contains("suppressed=0");

    fake_ms = 30;
    usb_packet_debug_note_in("xinput", data, sizeof(data), false);
    assert(line_count == 2);
    require_log_contains("reason=idle-sample");
    require_log_contains("suppressed=0");

    fake_ms = 500;
    usb_packet_debug_note_in("xinput", data, sizeof(data), false);
    assert(line_count == 2);

    fake_ms = 1100;
    usb_packet_debug_note_in("xinput", data, sizeof(data), false);
    assert(line_count == 3);
    require_log_contains("reason=idle-sample");
    require_log_contains("suppressed=1");
}

static void debug_control_setup_logs_wire_bytes(void) {
    fake_ms = 1200;
    reset_log();
    usb_packet_debug_note_setup("vendor-control", 0xC0, 0x20, 0x0102, 0x0304, 0x4000);
    assert(line_count == 1);
    require_log_contains("dir=setup");
    require_log_contains("src=vendor-control");
    require_log_contains("len=8");
    require_log_contains("reason=control-setup");
    require_log_contains("bm=0xC0");
    require_log_contains("req=0x20");
    require_log_contains("value=0x0102");
    require_log_contains("index=0x0304");
    require_log_contains("wlen=16384");
    require_log_contains("data=C020020104030040");
}

static void debug_control_in_logs_reply_payload(void) {
    uint8_t data[] = {0x12, 0x01, 0x00, 0x02};
    fake_ms = 1210;
    reset_log();
    usb_packet_debug_note_control_in("desc-device", data, sizeof(data));
    assert(line_count == 1);
    require_log_contains("dir=control-in");
    require_log_contains("src=desc-device");
    require_log_contains("reason=control-reply");
    require_log_contains("data=12010002");
}

static void debug_packet_stats_repeat_periodically(void) {
    uint8_t data[] = {0x55};
    reset_log();
    for (unsigned i = 0; i < 58; i++) {
        fake_ms = 1300 + i;
        usb_packet_debug_note_out("vendor", data, sizeof(data));
    }
    assert(line_count == 59);
    require_log_contains("usb-packet-stats");
    require_log_contains("total=64");
    require_log_contains("out=59");
    require_log_contains("setup=1");
    require_log_contains("control_in=1");
    require_log_contains("idle_in_suppressed=1");
}

int main(void) {
    normal_persona_does_not_log_packets();
    debug_out_packet_logs_hex_payload();
    debug_in_packets_keep_changed_and_summarize_idle_suppression();
    debug_control_setup_logs_wire_bytes();
    debug_control_in_logs_reply_payload();
    debug_packet_stats_repeat_periodically();
    puts("usb_packet_debug tests passed");
    return 0;
}
