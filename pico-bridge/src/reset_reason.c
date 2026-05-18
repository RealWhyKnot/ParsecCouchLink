#include "reset_reason.h"

#include <string.h>

#include "hardware/structs/watchdog.h"
#include "hardware/watchdog.h"
#include "pico/runtime.h"

#include "diag_log.h"

#if !PICO_RP2040
#include "pico/bootrom.h"
#endif

#define BREADCRUMB_MAGIC 0xC0DEBE11u

// CRC-protected "last live diag_log line" persisted across resets via
// `__uninitialized_ram`. Survives watchdog reset on both chips, and
// RUN-pin reset on RP2040; does NOT survive RUN-pin reset on RP2350
// (pico-sdk issue #2203). Treated as best-effort context, not a
// guarantee.
typedef struct {
    uint32_t magic;
    uint32_t crc32;
    char     last_line[RESET_REASON_LAST_LINE_CAP];
} crash_breadcrumb_t;

static crash_breadcrumb_t __uninitialized_ram(g_crash_breadcrumb);

static uint32_t crc32_compute(const void *data, size_t n) {
    const uint8_t *p = (const uint8_t *)data;
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < n; i++) {
        crc ^= p[i];
        for (int b = 0; b < 8; b++) {
            uint32_t mask = -(crc & 1u);
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return ~crc;
}

static bool breadcrumb_read(char *out, size_t cap) {
    if (g_crash_breadcrumb.magic != BREADCRUMB_MAGIC) return false;
    uint32_t want = crc32_compute(g_crash_breadcrumb.last_line,
                                  sizeof(g_crash_breadcrumb.last_line));
    if (want != g_crash_breadcrumb.crc32) return false;
    // The last_line field is NUL-padded; strlen-equivalent is safe.
    size_t n = 0;
    while (n < sizeof(g_crash_breadcrumb.last_line)
           && g_crash_breadcrumb.last_line[n] != 0) {
        n++;
    }
    if (n > cap - 1) n = cap - 1;
    for (size_t i = 0; i < n; i++) out[i] = g_crash_breadcrumb.last_line[i];
    out[n] = 0;
    return n > 0;
}

static void breadcrumb_clear(void) {
    g_crash_breadcrumb.magic = 0;
    g_crash_breadcrumb.crc32 = 0;
    memset(g_crash_breadcrumb.last_line, 0, sizeof(g_crash_breadcrumb.last_line));
}

reset_reason_info_t reset_reason_classify(void) {
    reset_reason_info_t out = (reset_reason_info_t){
        .reason          = RESET_REASON_UNKNOWN,
        .fault_pc        = 0,
        .fault_lr        = 0,
        .fault_xpsr      = 0,
        .last_line_valid = false,
    };
    out.last_line[0] = 0;

    uint32_t magic = watchdog_hw->scratch[0];
    uint32_t pc    = watchdog_hw->scratch[1];
    uint32_t lr    = watchdog_hw->scratch[2];
    uint32_t xpsr  = watchdog_hw->scratch[3];

    watchdog_hw->scratch[0] = 0;
    watchdog_hw->scratch[1] = 0;
    watchdog_hw->scratch[2] = 0;
    watchdog_hw->scratch[3] = 0;

    bool wdt = watchdog_caused_reboot();

#if !PICO_RP2040
    uint32_t bt = rom_get_last_boot_type();
    if (bt == BOOT_TYPE_FLASH_UPDATE) {
        breadcrumb_clear();
        out.reason = RESET_REASON_FLASH_UPDATE;
        return out;
    }
#endif

    if (wdt) {
        if (magic == RESET_REASON_MAGIC_FAULT) {
            out.reason     = RESET_REASON_FAULT;
            out.fault_pc   = pc;
            out.fault_lr   = lr;
            out.fault_xpsr = xpsr;
            out.last_line_valid = breadcrumb_read(out.last_line,
                                                  sizeof(out.last_line));
        } else if (magic == RESET_REASON_MAGIC_NORMAL_EXIT) {
            out.reason = RESET_REASON_DELIBERATE;
        } else {
            out.reason = RESET_REASON_WATCHDOG_HANG;
            // Hang during boot may still have a breadcrumb from the
            // previous boot's last live line -- worth carrying.
            out.last_line_valid = breadcrumb_read(out.last_line,
                                                  sizeof(out.last_line));
        }
    } else {
        out.reason = RESET_REASON_COLD_OR_PIN;
    }

    breadcrumb_clear();
    return out;
}

const char *reset_reason_name(reset_reason_t r) {
    switch (r) {
        case RESET_REASON_COLD_OR_PIN:   return "cold-or-pin-reset";
        case RESET_REASON_FLASH_UPDATE:  return "uf2-reflash";
        case RESET_REASON_DELIBERATE:    return "deliberate-reboot";
        case RESET_REASON_WATCHDOG_HANG: return "watchdog-hang";
        case RESET_REASON_FAULT:         return "fault";
        case RESET_REASON_UNKNOWN:
        default:                         return "unknown";
    }
}

void reset_reason_mark_main_loop_entered(void) {
    watchdog_hw->scratch[0] = RESET_REASON_MAGIC_NORMAL_EXIT;
}

void reset_reason_record_fault(uint32_t pc, uint32_t lr, uint32_t xpsr) {
    // Scratch first: smallest write set, most reliable under fault
    // context. Even if the breadcrumb storage fails or its CRC gets
    // torn, the reset-reason classifier can still report the fault
    // PC/LR/xPSR from scratch on the next boot.
    watchdog_hw->scratch[1] = pc;
    watchdog_hw->scratch[2] = lr;
    watchdog_hw->scratch[3] = xpsr;
    watchdog_hw->scratch[0] = RESET_REASON_MAGIC_FAULT;

    // Best-effort breadcrumb: copy the last diag_log line so the bug
    // report has some textual context about what was happening
    // before the fault.
    memset(g_crash_breadcrumb.last_line, 0,
           sizeof(g_crash_breadcrumb.last_line));
    diag_log_copy_last_line_unsafe(g_crash_breadcrumb.last_line,
                                   sizeof(g_crash_breadcrumb.last_line));
    g_crash_breadcrumb.crc32 = crc32_compute(g_crash_breadcrumb.last_line,
                                             sizeof(g_crash_breadcrumb.last_line));
    g_crash_breadcrumb.magic = BREADCRUMB_MAGIC;
}
