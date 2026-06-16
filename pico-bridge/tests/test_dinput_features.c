#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#include "pico/stdlib.h"

#include "boot_mode.h"
#include "dinput.h"
#include "gamepad_state.h"
#include "tusb.h"
#include "usb_diag.h"

static int failures = 0;
static run_persona_t current_persona = RUN_PERSONA_PS3;

#define CHECK(cond)                                                                                \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);                                 \
            failures++;                                                                            \
        }                                                                                          \
    } while (0)

volatile gamepad_state_t g_gamepad_state;
volatile uint32_t g_last_packet_ms;
volatile uint8_t g_parsec_connected;

absolute_time_t get_absolute_time(void) {
    return 0;
}

uint32_t to_ms_since_boot(absolute_time_t t) {
    return t;
}

run_persona_t boot_mode_run_persona(void) {
    return current_persona;
}

bool tud_mounted(void) {
    return false;
}

bool tud_suspended(void) {
    return false;
}

bool tud_hid_ready(void) {
    return false;
}

bool tud_hid_report(uint8_t report_id, void const *report, uint8_t len) {
    (void)report_id;
    (void)report;
    (void)len;
    return false;
}

void usb_diag_note_xinput_in_queued(uint32_t bytes) {
    (void)bytes;
}

void usb_diag_note_xinput_in_sent(uint32_t bytes) {
    (void)bytes;
}

void usb_diag_note_xinput_in_blocked(uint8_t reason, uint16_t want, uint16_t got) {
    (void)reason;
    (void)want;
    (void)got;
}

void usb_diag_note_xinput_in_idle_suppressed(void) {}

static uint16_t get_feature_report_wire(uint8_t report_id, uint16_t host_len, uint8_t *wire,
                                        uint16_t wire_len) {
    memset(wire, 0xA5, wire_len);
    if (host_len == 0 || wire_len < host_len)
        return 0;

    uint8_t *payload = wire;
    uint16_t payload_len = host_len;
    uint16_t xfer_len = 0;

    if (report_id != HID_REPORT_TYPE_INVALID && payload_len > 1) {
        *payload++ = report_id;
        payload_len--;
        xfer_len++;
    }

    xfer_len += dinput_get_report_payload(report_id, HID_REPORT_TYPE_FEATURE, payload, payload_len);
    return xfer_len;
}

static void test_ps3_operational_report_f2_matches_linux_request_size(void) {
    uint8_t wire[32];
    current_persona = RUN_PERSONA_PS3;
    dinput_init();

    uint16_t len = get_feature_report_wire(0xF2, 17, wire, sizeof(wire));

    CHECK(len == 17);
    CHECK(wire[0] == 0xF2);
    CHECK(wire[1] == 0xFF);
    CHECK(wire[2] == 0xFF);
    CHECK(wire[3] == 0x00);
    CHECK(wire[4] == 0x20);
    CHECK(wire[5] == 0x40);
    CHECK(wire[6] == 0xCE);
    CHECK(wire[7] == 0x21);
    CHECK(wire[8] == 0x43);
    CHECK(wire[9] == 0x65);
}

static void test_ps3_operational_report_f2_accepts_longer_request(void) {
    uint8_t wire[32];
    current_persona = RUN_PERSONA_PS3;
    dinput_init();

    uint16_t len = get_feature_report_wire(0xF2, 18, wire, sizeof(wire));

    CHECK(len == 18);
    CHECK(wire[0] == 0xF2);
    CHECK(wire[17] == 0x00);
}

static void test_ps4_feature_reports_still_copy_at_full_size(void) {
    uint8_t payload[64];
    current_persona = RUN_PERSONA_PS4;
    dinput_init();

    uint16_t len = dinput_get_report_payload(0xF2, HID_REPORT_TYPE_FEATURE, payload, 15);

    CHECK(len == 15);
    CHECK(payload[0] == 0x01);
    CHECK(payload[1] == 0x10);
    CHECK(payload[14] == 0x2A);
}

int main(void) {
    test_ps3_operational_report_f2_matches_linux_request_size();
    test_ps3_operational_report_f2_accepts_longer_request();
    test_ps4_feature_reports_still_copy_at_full_size();
    if (failures != 0)
        return 1;
    puts("dinput_feature tests passed");
    return 0;
}
