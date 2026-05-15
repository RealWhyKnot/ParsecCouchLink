#include "diag_log.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "pico/stdlib.h"
#include "pico/sync.h"

#define LOG_RING_SIZE 2048

static uint8_t  ring[LOG_RING_SIZE];
static size_t   head;      // next write position
static size_t   filled;    // number of valid bytes in the ring
static critical_section_t cs;

void diag_log_init(void) {
    head = 0;
    filled = 0;
    critical_section_init(&cs);
}

static void write_bytes(const uint8_t *data, size_t n) {
    critical_section_enter_blocking(&cs);
    for (size_t i = 0; i < n; i++) {
        ring[head] = data[i];
        head = (head + 1) % LOG_RING_SIZE;
        if (filled < LOG_RING_SIZE) {
            filled++;
        }
    }
    critical_section_exit(&cs);
}

void diag_log_msg(const char *msg) {
    if (!msg) return;
    // Build the full timestamped line into a single stack buffer before
    // entering the ring's critical section, so an interrupting context
    // that also logs can't interleave its bytes into the middle of ours.
    char line[224];
    uint32_t ms = to_ms_since_boot(get_absolute_time());
    int n = snprintf(line, sizeof(line), "[%10u] %s\n", ms, msg);
    if (n <= 0) return;
    size_t len = (size_t)n;
    if (len >= sizeof(line)) {
        len = sizeof(line) - 1;
        line[len - 1] = '\n';
    }
    write_bytes((const uint8_t *)line, len);
}

void diag_log_printf(const char *fmt, ...) {
    char buf[192];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    if (n < 0) return;
    if ((size_t)n > sizeof(buf) - 1) n = sizeof(buf) - 1;
    buf[n] = 0;
    diag_log_msg(buf);
}

size_t diag_log_snapshot(uint8_t *out, size_t cap) {
    if (!out || cap == 0) return 0;
    critical_section_enter_blocking(&cs);
    // Walk from the oldest valid byte to head, copying up to `cap`.
    size_t avail = filled;
    size_t skip = (avail > cap) ? (avail - cap) : 0;
    size_t to_write = avail - skip;
    size_t start = (head + LOG_RING_SIZE - avail + skip) % LOG_RING_SIZE;
    for (size_t i = 0; i < to_write; i++) {
        out[i] = ring[(start + i) % LOG_RING_SIZE];
    }
    critical_section_exit(&cs);
    return to_write;
}
