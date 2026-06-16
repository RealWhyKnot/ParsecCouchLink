#include "usb_packet_debug.h"

#include <stddef.h>

#include "pico/stdlib.h"

#include "boot_mode.h"
#include "diag_log.h"

#define USB_PACKET_DEBUG_MAX_BYTES 64u

static uint32_t seq;
static uint32_t dropped_bytes;

static char hex_digit(uint8_t v) {
    v &= 0x0Fu;
    return (char)(v < 10u ? ('0' + v) : ('A' + (v - 10u)));
}

void usb_packet_debug_note_out(const char *source, uint8_t const *buffer, uint16_t len) {
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

    diag_log_printf("usb-packet seq=%u t=%u dir=out src=%s len=%u captured=%u dropped=%u data=%s",
                    (unsigned)seq++, (unsigned)to_ms_since_boot(get_absolute_time()),
                    source ? source : "unknown", (unsigned)len, (unsigned)capture_len,
                    (unsigned)dropped_bytes, hex);
}
