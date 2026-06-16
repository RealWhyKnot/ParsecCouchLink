#include "usb_packet_debug.h"

#include <stddef.h>
#include <stdio.h>
#include <string.h>

#include "pico/stdlib.h"

#include "boot_mode.h"
#include "diag_log.h"

#define USB_PACKET_DEBUG_MAX_BYTES 64u
#define USB_PACKET_DEBUG_IDLE_SAMPLE_MS 1000u
#define USB_PACKET_DEBUG_STATS_EVERY_PACKETS 64u

static uint32_t seq;
static uint32_t dropped_bytes;
static uint32_t suppressed_idle_in_reports;
static uint32_t total_suppressed_idle_in_reports;
static uint32_t last_idle_in_sample_ms;
static uint32_t total_packet_lines;
static uint32_t in_packet_lines;
static uint32_t out_packet_lines;
static uint32_t setup_packet_lines;
static uint32_t control_in_packet_lines;

static char hex_digit(uint8_t v) {
    v &= 0x0Fu;
    return (char)(v < 10u ? ('0' + v) : ('A' + (v - 10u)));
}

static void count_direction(const char *direction) {
    total_packet_lines++;
    if (strcmp(direction, "in") == 0) {
        in_packet_lines++;
    } else if (strcmp(direction, "out") == 0) {
        out_packet_lines++;
    } else if (strcmp(direction, "setup") == 0) {
        setup_packet_lines++;
    } else if (strcmp(direction, "control-in") == 0) {
        control_in_packet_lines++;
    }
}

static void maybe_log_stats(uint32_t now_ms) {
    if (total_packet_lines != 1u &&
        (total_packet_lines % USB_PACKET_DEBUG_STATS_EVERY_PACKETS) != 0u) {
        return;
    }
    diag_log_printf("usb-packet-stats t=%u total=%u in=%u out=%u setup=%u control_in=%u "
                    "truncated_bytes=%u idle_in_suppressed=%u",
                    (unsigned)now_ms, (unsigned)total_packet_lines, (unsigned)in_packet_lines,
                    (unsigned)out_packet_lines, (unsigned)setup_packet_lines,
                    (unsigned)control_in_packet_lines, (unsigned)dropped_bytes,
                    (unsigned)total_suppressed_idle_in_reports);
}

static void note_packet_extra(const char *direction, const char *source, uint8_t const *buffer,
                              uint16_t len, const char *reason, uint32_t suppressed,
                              const char *extra_fields) {
    if (boot_mode_run_persona() != RUN_PERSONA_DEBUG)
        return;

    uint16_t capture_len = len;
    if (capture_len > USB_PACKET_DEBUG_MAX_BYTES) {
        dropped_bytes += (uint32_t)(capture_len - USB_PACKET_DEBUG_MAX_BYTES);
        capture_len = USB_PACKET_DEBUG_MAX_BYTES;
    }

    char hex[(USB_PACKET_DEBUG_MAX_BYTES * 2u) + 1u];
    for (uint16_t i = 0; i < capture_len; i++) {
        uint8_t b = (buffer != NULL) ? buffer[i] : 0;
        hex[i * 2u] = hex_digit((uint8_t)(b >> 4));
        hex[(i * 2u) + 1u] = hex_digit(b);
    }
    hex[capture_len * 2u] = 0;

    uint32_t now_ms = to_ms_since_boot(get_absolute_time());
    count_direction(direction);
    diag_log_printf("usb-packet seq=%u t=%u dir=%s src=%s len=%u captured=%u dropped=%u "
                    "suppressed=%u reason=%s %sdata=%s",
                    (unsigned)seq++, (unsigned)now_ms, direction, source ? source : "unknown",
                    (unsigned)len, (unsigned)capture_len, (unsigned)dropped_bytes,
                    (unsigned)suppressed, reason, extra_fields ? extra_fields : "", hex);
    maybe_log_stats(now_ms);
}

static void note_packet(const char *direction, const char *source, uint8_t const *buffer,
                        uint16_t len, const char *reason, uint32_t suppressed) {
    note_packet_extra(direction, source, buffer, len, reason, suppressed, "");
}

void usb_packet_debug_note_out(const char *source, uint8_t const *buffer, uint16_t len) {
    note_packet("out", source, buffer, len, "host-out", 0);
}

void usb_packet_debug_note_out_report(const char *source, uint8_t report_id, uint8_t report_type,
                                      uint8_t const *buffer, uint16_t len) {
    char extra[64];
    snprintf(extra, sizeof(extra), "report_id=0x%02X report_type=%u ", (unsigned)report_id,
             (unsigned)report_type);
    note_packet_extra("out", source, buffer, len, "host-out", 0, extra);
}

void usb_packet_debug_note_in(const char *source, uint8_t const *buffer, uint16_t len,
                              bool changed) {
    if (boot_mode_run_persona() != RUN_PERSONA_DEBUG)
        return;

    uint32_t now = to_ms_since_boot(get_absolute_time());
    if (!changed && last_idle_in_sample_ms != 0 &&
        (uint32_t)(now - last_idle_in_sample_ms) < USB_PACKET_DEBUG_IDLE_SAMPLE_MS) {
        suppressed_idle_in_reports++;
        total_suppressed_idle_in_reports++;
        return;
    }

    uint32_t suppressed = suppressed_idle_in_reports;
    suppressed_idle_in_reports = 0;
    if (!changed)
        last_idle_in_sample_ms = now;
    note_packet("in", source, buffer, len, changed ? "changed" : "idle-sample", suppressed);
}

void usb_packet_debug_note_setup(const char *source, uint8_t bm_request_type, uint8_t b_request,
                                 uint16_t w_value, uint16_t w_index, uint16_t w_length) {
    uint8_t setup[8] = {
        bm_request_type,
        b_request,
        (uint8_t)(w_value & 0xFFu),
        (uint8_t)((w_value >> 8) & 0xFFu),
        (uint8_t)(w_index & 0xFFu),
        (uint8_t)((w_index >> 8) & 0xFFu),
        (uint8_t)(w_length & 0xFFu),
        (uint8_t)((w_length >> 8) & 0xFFu),
    };
    char extra[80];
    snprintf(extra, sizeof(extra), "bm=0x%02X req=0x%02X value=0x%04X index=0x%04X wlen=%u ",
             (unsigned)bm_request_type, (unsigned)b_request, (unsigned)w_value, (unsigned)w_index,
             (unsigned)w_length);
    note_packet_extra("setup", source, setup, sizeof(setup), "control-setup", 0, extra);
}

void usb_packet_debug_note_hid_get_report(uint8_t instance, uint8_t report_id, uint8_t report_type,
                                          uint16_t request_len) {
    uint16_t value = ((uint16_t)report_type << 8) | report_id;
    usb_packet_debug_note_setup("hid-get-report", 0xA1, 0x01, value, instance, request_len);
}

void usb_packet_debug_note_hid_set_report(uint8_t instance, uint8_t report_id, uint8_t report_type,
                                          uint16_t payload_len) {
    uint16_t value = ((uint16_t)report_type << 8) | report_id;
    usb_packet_debug_note_setup("hid-set-report", 0x21, 0x09, value, instance, payload_len);
}

void usb_packet_debug_note_control_in(const char *source, uint8_t const *buffer, uint16_t len) {
    note_packet("control-in", source, buffer, len, "control-reply", 0);
}
