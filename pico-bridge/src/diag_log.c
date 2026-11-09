#include "diag_log.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "pico/stdlib.h"
#include "pico/sync.h"

static uint8_t ring[DIAG_LOG_RING_SIZE];
static size_t head;
static size_t filled;
static uint32_t lost;
static critical_section_t cs;

void diag_log_init(void) {
    head = 0;
    filled = 0;
    lost = 0;
    critical_section_init(&cs);
}

static void write_bytes(const uint8_t *data, size_t n) {
    critical_section_enter_blocking(&cs);
    for (size_t i = 0; i < n; i++) {
        if (filled == DIAG_LOG_RING_SIZE) {
            lost++;
        }
        ring[head] = data[i];
        head = (head + 1) % DIAG_LOG_RING_SIZE;
        if (filled < DIAG_LOG_RING_SIZE) {
            filled++;
        }
    }
    critical_section_exit(&cs);
}

void diag_log_msg(const char *msg) {
    if (!msg)
        return;
    char line[224];
    uint32_t ms = to_ms_since_boot(get_absolute_time());
    int n = snprintf(line, sizeof(line), "[%10u] %s\n", ms, msg);
    if (n <= 0)
        return;
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
    if (n < 0)
        return;
    if ((size_t)n > sizeof(buf) - 1)
        n = sizeof(buf) - 1;
    buf[n] = 0;
    diag_log_msg(buf);
}

size_t diag_log_snapshot(uint8_t *out, size_t cap, uint32_t *lost_out) {
    if (!out || cap == 0) {
        if (lost_out) {
            critical_section_enter_blocking(&cs);
            *lost_out = lost;
            critical_section_exit(&cs);
        }
        return 0;
    }
    critical_section_enter_blocking(&cs);
    size_t avail = filled;
    size_t skip = (avail > cap) ? (avail - cap) : 0;
    size_t to_write = avail - skip;
    size_t start = (head + DIAG_LOG_RING_SIZE - avail + skip) % DIAG_LOG_RING_SIZE;
    for (size_t i = 0; i < to_write; i++) {
        out[i] = ring[(start + i) % DIAG_LOG_RING_SIZE];
    }
    if (lost_out) {
        *lost_out = lost + (uint32_t)skip;
    }
    critical_section_exit(&cs);
    return to_write;
}

static size_t copy_last_line_inner(char *out, size_t cap) {
    size_t avail = filled;
    if (avail == 0) {
        if (cap > 0)
            out[0] = 0;
        return 0;
    }
    size_t end = (head + DIAG_LOG_RING_SIZE - 1) % DIAG_LOG_RING_SIZE;
    size_t walk = 1;
    if (ring[end] == '\n') {
        if (walk >= avail) {
            if (cap > 0)
                out[0] = 0;
            return 0;
        }
        end = (end + DIAG_LOG_RING_SIZE - 1) % DIAG_LOG_RING_SIZE;
        walk++;
    }
    size_t line_end = end;
    while (walk < avail) {
        size_t prev = (end + DIAG_LOG_RING_SIZE - 1) % DIAG_LOG_RING_SIZE;
        if (ring[prev] == '\n')
            break;
        end = prev;
        walk++;
    }
    size_t line_start = end;
    size_t len = (line_end >= line_start) ? (line_end - line_start + 1)
                                          : (DIAG_LOG_RING_SIZE - line_start + line_end + 1);
    if (cap > 0 && len > cap - 1)
        len = cap - 1;
    for (size_t i = 0; i < len; i++) {
        out[i] = (char)ring[(line_start + i) % DIAG_LOG_RING_SIZE];
    }
    if (cap > 0)
        out[len < cap ? len : cap - 1] = 0;
    return len;
}

size_t diag_log_copy_last_line(char *out, size_t cap) {
    if (!out || cap == 0)
        return 0;
    critical_section_enter_blocking(&cs);
    size_t n = copy_last_line_inner(out, cap);
    critical_section_exit(&cs);
    return n;
}

size_t diag_log_copy_last_line_unsafe(char *out, size_t cap) {
    if (!out || cap == 0)
        return 0;
    return copy_last_line_inner(out, cap);
}
