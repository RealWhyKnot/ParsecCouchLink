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

static void require_log_not_contains(const char *needle) {
    if (strstr(log_text, needle) != NULL) {
        printf("unexpected `%s` in `%s`\n", needle, log_text);
        assert(false);
    }
}

static void normal_persona_retains_boot_snapshot_without_in_payloads(void) {
    uint8_t data[] = {0x01, 0x02};
    current_persona = RUN_PERSONA_XINPUT;
    usb_packet_debug_set_capture_enabled(false);
    reset_log();
    usb_packet_debug_note_out("vendor", data, sizeof(data));
    usb_packet_debug_note_in("xinput", data, sizeof(data), true);
    usb_packet_debug_note_setup("vendor-control", 0xC0, 0x20, 0x0102, 0x0304, 0x4000);
    usb_packet_debug_note_control_in("desc-device", data, sizeof(data));
    usb_packet_debug_note_hid_get_report(2, 0xEF, 3, 64);
    usb_packet_debug_note_hid_set_report(2, 0x01, 2, sizeof(data));
    usb_packet_debug_note_out_report("hid-output", 0x01, 2, data, sizeof(data));
    usb_packet_debug_note_event("mount", "");
    require_log_contains("dir=out");
    require_log_contains("dir=setup");
    require_log_contains("dir=control-in");
    require_log_contains("event=mount");
    require_log_not_contains("dir=in");
    current_persona = RUN_PERSONA_DEBUG;
}

static void debug_out_packet_logs_hex_payload(void) {
    uint8_t data[] = {0x01, 0x02, 0xA0};
    fake_ms = 10;
    reset_log();
    usb_packet_debug_note_out("vendor", data, sizeof(data));
    assert(line_count == 3);
    require_log_contains("usb-event t=10 event=first-host-out src=vendor len=3");
    require_log_contains("dir=out");
    require_log_contains("src=vendor");
    require_log_contains("reason=host-out");
    require_log_contains("truncated=0");
    require_log_contains("data=0102A0");
    require_log_contains("usb-packet-stats");
    require_log_contains("total=1");
    require_log_contains("out=1");
    require_log_contains("truncated_packets=0");
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

static void debug_in_accepted_logs_once(void) {
    fake_ms = 1110;
    reset_log();
    usb_packet_debug_note_in_accepted("xinput", 0);
    assert(line_count == 0);
    usb_packet_debug_note_in_accepted("xinput", 20);
    usb_packet_debug_note_in_accepted("xinput", 20);
    assert(line_count == 1);
    require_log_contains("usb-event t=1110 event=first-in-accepted src=xinput bytes=20");
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

static void debug_hid_control_requests_log_setup_metadata(void) {
    fake_ms = 1220;
    reset_log();
    usb_packet_debug_note_hid_get_report(2, 0xEF, 3, 64);
    usb_packet_debug_note_hid_set_report(2, 0x01, 2, 4);
    assert(line_count == 2);
    require_log_contains("src=hid-get-report");
    require_log_contains("bm=0xA1");
    require_log_contains("req=0x01");
    require_log_contains("value=0x03EF");
    require_log_contains("index=0x0002");
    require_log_contains("wlen=64");
    require_log_contains("data=A101EF0302004000");
    require_log_contains("src=hid-set-report");
    require_log_contains("bm=0x21");
    require_log_contains("req=0x09");
    require_log_contains("value=0x0201");
    require_log_contains("wlen=4");
    require_log_contains("data=2109010202000400");
}

static void debug_hid_out_report_logs_report_metadata(void) {
    uint8_t data[] = {0x05, 0x06, 0x07};
    fake_ms = 1230;
    reset_log();
    usb_packet_debug_note_out_report("hid-output", 0x01, 2, data, sizeof(data));
    assert(line_count == 1);
    require_log_contains("dir=out");
    require_log_contains("src=hid-output");
    require_log_contains("report_id=0x01");
    require_log_contains("report_type=2");
    require_log_contains("data=050607");
}

static void debug_packet_logs_per_packet_truncation(void) {
    uint8_t data[70];
    for (unsigned i = 0; i < sizeof(data); i++)
        data[i] = (uint8_t)i;

    fake_ms = 1240;
    reset_log();
    usb_packet_debug_note_out("vendor", data, sizeof(data));
    assert(line_count == 1);
    require_log_contains("len=70");
    require_log_contains("captured=64");
    require_log_contains("truncated=6");
    require_log_contains("dropped=6");
}

static void debug_usb_event_logs_lifecycle_without_packet_stats(void) {
    fake_ms = 1250;
    reset_log();
    usb_packet_debug_note_event("mount", "");
    usb_packet_debug_note_event("suspend", "remote_wakeup=1");
    assert(line_count == 2);
    require_log_contains("usb-event t=1250 event=mount");
    require_log_contains("usb-event t=1250 event=suspend remote_wakeup=1");
    assert(strstr(log_text, "usb-packet-stats") == NULL);
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
    require_log_contains("out=57");
    require_log_contains("setup=3");
    require_log_contains("control_in=1");
    require_log_contains("truncated_packets=1");
    require_log_contains("idle_in_suppressed=1");
}

static void capture_enabled_normal_persona_logs_packets(void) {
    uint8_t data[] = {0x12, 0x34};
    current_persona = RUN_PERSONA_PS4;
    usb_packet_debug_set_capture_enabled(true);
    fake_ms = 1400;
    reset_log();
    usb_packet_debug_note_setup("desc-device", 0x80, 0x06, 0x0100, 0, 18);
    assert(line_count >= 1);
    require_log_contains("dir=setup");
    require_log_contains("src=desc-device");
    require_log_contains("data=8006000100001200");
    assert(usb_packet_debug_capture_enabled());

    usb_packet_debug_set_capture_enabled(false);
    reset_log();
    usb_packet_debug_note_in("ps4", data, sizeof(data), true);
    assert(line_count == 0);
    current_persona = RUN_PERSONA_DEBUG;
}

int main(void) {
    debug_out_packet_logs_hex_payload();
    debug_in_packets_keep_changed_and_summarize_idle_suppression();
    debug_in_accepted_logs_once();
    debug_control_setup_logs_wire_bytes();
    debug_control_in_logs_reply_payload();
    debug_hid_control_requests_log_setup_metadata();
    debug_hid_out_report_logs_report_metadata();
    debug_packet_logs_per_packet_truncation();
    debug_usb_event_logs_lifecycle_without_packet_stats();
    debug_packet_stats_repeat_periodically();
    normal_persona_retains_boot_snapshot_without_in_payloads();
    capture_enabled_normal_persona_logs_packets();
    puts("usb_packet_debug tests passed");
    return 0;
}
