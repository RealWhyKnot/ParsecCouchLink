#include "reset_reason.h"

#include <stddef.h>
#include <string.h>

#include "hardware/structs/watchdog.h"
#include "hardware/watchdog.h"
#include "pico/runtime.h"

#include "diag_log.h"

#if !PICO_RP2040
#include "pico/bootrom.h"
#endif

// Bumped from 0xC0DEBE12 -> 0xC0DEBE13 because the breadcrumb struct
// layout grew (force_setup_after_reboot field added). An old breadcrumb
// from a prior build will fail the magic check and be ignored.
#define BREADCRUMB_MAGIC 0xC0DEBE13u
#define USB_CAPTURE_MAGIC 0xC0A7CAFEu

// CRC-protected fault context persisted across resets via
// `__uninitialized_ram`. Survives watchdog reset on both chips, and
// RUN-pin reset on RP2040; does NOT survive RUN-pin reset on RP2350
// (pico-sdk issue #2203). Treated as best-effort context, not a
// guarantee.
//
// Layout intentionally puts magic + crc32 first so the CRC covers
// every other byte without having to skip a hole in the middle. The
// fault registers come before last_line so an alignment mishap on the
// char array does not push the regs across a cache line boundary.
typedef struct {
    uint32_t magic;
    uint32_t crc32;
    uint32_t r0, r1, r2, r3, r12, lr, pc, xpsr;
    uint32_t sp_at_fault;
    uint32_t cfsr; // valid only on Cortex-M33 / RP2350; zero on RP2040
    uint32_t hfsr;
    uint32_t mmfar;
    uint32_t bfar;
    uint8_t fault_status_valid; // 1 = the four regs above are RP2350-valid
    uint8_t force_setup;        // 1 = boot_mode_decide must force setup mode
    uint8_t reserved[2];
    char last_line[RESET_REASON_LAST_LINE_CAP];
} crash_breadcrumb_t;

static crash_breadcrumb_t __uninitialized_ram(g_crash_breadcrumb);

// Latched once by reset_reason_classify(); read by boot_mode_decide().
static bool g_force_setup_requested = false;

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

// CRC covers every byte after `crc32` itself. The two leading
// magic/CRC fields are the only thing the recovery path uses to
// validate the rest.
static uint32_t breadcrumb_crc(void) {
    const uint8_t *base = (const uint8_t *)&g_crash_breadcrumb;
    size_t off = offsetof(crash_breadcrumb_t, r0);
    size_t end = sizeof(crash_breadcrumb_t);
    return crc32_compute(base + off, end - off);
}

static bool breadcrumb_valid(void) {
    if (g_crash_breadcrumb.magic != BREADCRUMB_MAGIC)
        return false;
    return g_crash_breadcrumb.crc32 == breadcrumb_crc();
}

static bool breadcrumb_read_last_line(char *out, size_t cap) {
    if (!breadcrumb_valid())
        return false;
    size_t n = 0;
    while (n < sizeof(g_crash_breadcrumb.last_line) && g_crash_breadcrumb.last_line[n] != 0) {
        n++;
    }
    if (n > cap - 1)
        n = cap - 1;
    for (size_t i = 0; i < n; i++)
        out[i] = g_crash_breadcrumb.last_line[i];
    out[n] = 0;
    return n > 0;
}

static void breadcrumb_clear(void) {
    memset(&g_crash_breadcrumb, 0, sizeof(g_crash_breadcrumb));
}

// Read the full breadcrumb into `out` if valid, otherwise leave the
// new fields at zero/false. Always returns the last_line_valid status
// for the caller's convenience.
static void breadcrumb_fill_info(reset_reason_info_t *out) {
    if (!breadcrumb_valid()) {
        out->full_frame_valid = false;
        out->fault_status_valid = false;
        out->force_setup_after_reboot = false;
        return;
    }
    out->full_frame_valid = true;
    out->fault_r0 = g_crash_breadcrumb.r0;
    out->fault_r1 = g_crash_breadcrumb.r1;
    out->fault_r2 = g_crash_breadcrumb.r2;
    out->fault_r3 = g_crash_breadcrumb.r3;
    out->fault_r12 = g_crash_breadcrumb.r12;
    out->fault_sp = g_crash_breadcrumb.sp_at_fault;
    if (g_crash_breadcrumb.fault_status_valid) {
        out->fault_status_valid = true;
        out->fault_cfsr = g_crash_breadcrumb.cfsr;
        out->fault_hfsr = g_crash_breadcrumb.hfsr;
        out->fault_mmfar = g_crash_breadcrumb.mmfar;
        out->fault_bfar = g_crash_breadcrumb.bfar;
    }
    // Read and clear the one-shot flag so a second call after an
    // accidental breadcrumb_clear() doesn't re-trigger it.
    out->force_setup_after_reboot = (g_crash_breadcrumb.force_setup != 0);
    g_crash_breadcrumb.force_setup = 0;
}

reset_reason_info_t reset_reason_classify(void) {
    reset_reason_info_t out = (reset_reason_info_t){
        .reason = RESET_REASON_UNKNOWN,
        .fault_pc = 0,
        .fault_lr = 0,
        .fault_xpsr = 0,
        .full_frame_valid = false,
        .fault_status_valid = false,
        .last_line_valid = false,
        .force_setup_after_reboot = false,
    };
    out.last_line[0] = 0;

    uint32_t magic = watchdog_hw->scratch[0];
    uint32_t pc = watchdog_hw->scratch[1];
    uint32_t lr = watchdog_hw->scratch[2];
    uint32_t xpsr = watchdog_hw->scratch[3];

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
            out.reason = RESET_REASON_FAULT;
            out.fault_pc = pc;
            out.fault_lr = lr;
            out.fault_xpsr = xpsr;
            breadcrumb_fill_info(&out);
            out.last_line_valid = breadcrumb_read_last_line(out.last_line, sizeof(out.last_line));
        } else if (magic == RESET_REASON_MAGIC_NORMAL_EXIT) {
            out.reason = RESET_REASON_DELIBERATE;
            // A deliberate reboot may carry the force_setup flag set by
            // reset_reason_request_setup_after_reboot(). Read it now and
            // clear it (one-shot) before breadcrumb_clear() below.
            breadcrumb_fill_info(&out);
            out.last_line_valid = breadcrumb_read_last_line(out.last_line, sizeof(out.last_line));
        } else {
            out.reason = RESET_REASON_WATCHDOG_HANG;
            // Hang during boot may still have a breadcrumb from the
            // previous boot's last live line -- worth carrying.
            breadcrumb_fill_info(&out);
            out.last_line_valid = breadcrumb_read_last_line(out.last_line, sizeof(out.last_line));
        }
    } else {
        out.reason = RESET_REASON_COLD_OR_PIN;
    }

    // Latch before clearing -- breadcrumb_clear() zeros the raw field.
    g_force_setup_requested = out.force_setup_after_reboot;

    breadcrumb_clear();
    return out;
}

const char *reset_reason_name(reset_reason_t r) {
    switch (r) {
    case RESET_REASON_COLD_OR_PIN:
        return "cold-or-pin-reset";
    case RESET_REASON_FLASH_UPDATE:
        return "uf2-reflash";
    case RESET_REASON_DELIBERATE:
        return "deliberate-reboot";
    case RESET_REASON_WATCHDOG_HANG:
        return "watchdog-hang";
    case RESET_REASON_FAULT:
        return "fault";
    case RESET_REASON_UNKNOWN:
    default:
        return "unknown";
    }
}

void reset_reason_mark_main_loop_entered(void) {
    watchdog_hw->scratch[0] = RESET_REASON_MAGIC_NORMAL_EXIT;
}

bool reset_reason_force_setup_requested(void) {
    return g_force_setup_requested;
}

void reset_reason_request_setup_with_note(const char *note) {
    // Write a minimal breadcrumb containing the force_setup flag and,
    // optionally, a short human-readable reason in last_line. We
    // intentionally do not stomp any existing fault frame here -- this is
    // called from a healthy code path. Write MAGIC_NORMAL_EXIT to
    // scratch[0] so reset_reason_classify() takes the DELIBERATE branch
    // and reads the breadcrumb (the force_setup flag and last_line live
    // there, not in scratch). The note must be populated BEFORE the CRC is
    // computed, since the CRC covers last_line.
    memset(&g_crash_breadcrumb, 0, sizeof(g_crash_breadcrumb));
    g_crash_breadcrumb.force_setup = 1;
    if (note) {
        size_t i = 0;
        for (; i < sizeof(g_crash_breadcrumb.last_line) - 1 && note[i]; i++)
            g_crash_breadcrumb.last_line[i] = note[i];
        g_crash_breadcrumb.last_line[i] = 0;
    }
    g_crash_breadcrumb.crc32 = breadcrumb_crc();
    g_crash_breadcrumb.magic = BREADCRUMB_MAGIC;
    // Mark scratch[0] so classify() treats this as a deliberate reboot
    // and calls breadcrumb_fill_info() to pick up force_setup.
    watchdog_hw->scratch[0] = RESET_REASON_MAGIC_NORMAL_EXIT;
}

void reset_reason_request_setup_after_reboot(void) {
    reset_reason_request_setup_with_note(NULL);
}

void reset_reason_request_usb_capture_after_reboot(uint8_t persona) {
    watchdog_hw->scratch[5] = USB_CAPTURE_MAGIC;
    watchdog_hw->scratch[6] = (uint32_t)persona;
    watchdog_hw->scratch[0] = RESET_REASON_MAGIC_NORMAL_EXIT;
}

bool reset_reason_consume_usb_capture_request(uint8_t *persona) {
    if (watchdog_hw->scratch[5] != USB_CAPTURE_MAGIC) {
        return false;
    }
    if (persona) {
        *persona = (uint8_t)(watchdog_hw->scratch[6] & 0xFFu);
    }
    watchdog_hw->scratch[5] = 0;
    watchdog_hw->scratch[6] = 0;
    return true;
}

void reset_reason_record_fault(const uint32_t *frame, uint32_t sp_at_fault) {
    // Scratch first: smallest write set, most reliable under fault
    // context. Even if the breadcrumb storage fails or its CRC gets
    // torn, the reset-reason classifier can still report the fault
    // PC/LR/xPSR from scratch on the next boot.
    uint32_t pc = frame[6];
    uint32_t lr = frame[5];
    uint32_t xpsr = frame[7];
    watchdog_hw->scratch[1] = pc;
    watchdog_hw->scratch[2] = lr;
    watchdog_hw->scratch[3] = xpsr;
    watchdog_hw->scratch[0] = RESET_REASON_MAGIC_FAULT;

    // Fuller context in the CRC'd breadcrumb: full basic frame + SP +
    // (Cortex-M33 only) the SCB fault status registers. The CFSR/HFSR/
    // MMFAR/BFAR addresses are the same on all ARMv7-M / ARMv8-M cores;
    // on Cortex-M0+ those addresses are reserved memory and reads
    // would fault again. The PICO_RP2040 guard keeps the RP2040 path
    // entirely off them.
    memset(&g_crash_breadcrumb, 0, sizeof(g_crash_breadcrumb));
    g_crash_breadcrumb.r0 = frame[0];
    g_crash_breadcrumb.r1 = frame[1];
    g_crash_breadcrumb.r2 = frame[2];
    g_crash_breadcrumb.r3 = frame[3];
    g_crash_breadcrumb.r12 = frame[4];
    g_crash_breadcrumb.lr = frame[5];
    g_crash_breadcrumb.pc = frame[6];
    g_crash_breadcrumb.xpsr = frame[7];
    g_crash_breadcrumb.sp_at_fault = sp_at_fault;
#if !PICO_RP2040
    {
        // SCB->CFSR @ 0xE000ED28, HFSR @ 0xE000ED2C,
        // MMFAR @ 0xE000ED34, BFAR @ 0xE000ED38.
        volatile uint32_t *cfsr = (volatile uint32_t *)0xE000ED28u;
        volatile uint32_t *hfsr = (volatile uint32_t *)0xE000ED2Cu;
        volatile uint32_t *mmfar = (volatile uint32_t *)0xE000ED34u;
        volatile uint32_t *bfar = (volatile uint32_t *)0xE000ED38u;
        g_crash_breadcrumb.cfsr = *cfsr;
        g_crash_breadcrumb.hfsr = *hfsr;
        g_crash_breadcrumb.mmfar = *mmfar;
        g_crash_breadcrumb.bfar = *bfar;
        g_crash_breadcrumb.fault_status_valid = 1;
    }
#endif
    diag_log_copy_last_line_unsafe(g_crash_breadcrumb.last_line,
                                   sizeof(g_crash_breadcrumb.last_line));
    g_crash_breadcrumb.crc32 = breadcrumb_crc();
    g_crash_breadcrumb.magic = BREADCRUMB_MAGIC;
}
