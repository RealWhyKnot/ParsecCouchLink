#pragma once

#include <stdbool.h>
#include <stdint.h>

// Cross-reset reset-cause classifier. Reads SDK + chip state at boot
// and labels why the firmware is starting now. The point is that a
// hardware watchdog reset, a HardFault, a deliberate reboot, and a
// fresh cable plug-in all look identical to a single-bit
// `watchdog_caused_reboot()` check; this module layers enough state
// to tell them apart in `couchlink bundle`.
//
// Convention: watchdog scratch[0..3] are reserved by this module.
//   scratch[0]: state-magic
//     0                 = uninitialized (cold start)
//     MAGIC_NORMAL_EXIT = main() entered its main loop cleanly
//     MAGIC_FAULT       = fault handler captured a frame before reset
//   scratch[1] = captured PC (valid only when state == MAGIC_FAULT)
//   scratch[2] = captured LR (valid only when state == MAGIC_FAULT)
//   scratch[3] = captured xPSR (valid only when state == MAGIC_FAULT)
//
// In addition, a CRC-protected `last_line` mini-buffer lives in
// `__uninitialized_ram` so the most recent diag_log line is
// recoverable on the next boot. Watchdog scratch survives both
// watchdog and (on RP2040) RUN-pin reset; `__uninitialized_ram`
// survives watchdog reset on both chips but not RUN-pin reset on
// RP2350 (pico-sdk issue #2203). The two together give the best
// available post-mortem.
//
// scratch[4..7] are intentionally not used by this module:
//   - scratch[4] is owned by the SDK's watchdog_enable_caused_reboot().
//   - scratch[5..7] are kept available for SDK bootrom helpers.

#define RESET_REASON_MAGIC_NORMAL_EXIT 0xB007C1EAu
#define RESET_REASON_MAGIC_FAULT       0xFA0FAEDDu

#define RESET_REASON_LAST_LINE_CAP 56

typedef enum {
    RESET_REASON_UNKNOWN = 0,
    RESET_REASON_COLD_OR_PIN,    // cold power-on, RUN-pin reset, or debugger reset
    RESET_REASON_FLASH_UPDATE,   // UF2 was just dropped (RP2350 only; RP2040 cannot distinguish)
    RESET_REASON_DELIBERATE,     // firmware called watchdog_reboot() in a healthy state
    RESET_REASON_WATCHDOG_HANG,  // hardware watchdog tripped because firmware hung
    RESET_REASON_FAULT,          // HardFault / BusFault / etc. captured a frame and reset
} reset_reason_t;

typedef struct {
    reset_reason_t reason;
    uint32_t fault_pc;                                  // valid only when reason == RESET_REASON_FAULT
    uint32_t fault_lr;
    uint32_t fault_xpsr;
    bool     last_line_valid;
    char     last_line[RESET_REASON_LAST_LINE_CAP];     // most recent diag_log line before the fault
} reset_reason_info_t;

// Read scratch + breadcrumb + SDK state, classify, and clear the
// magic so the next boot does not see stale data. Must be called
// exactly once at boot, after diag_log_init and before any code that
// touches scratch[0..3].
reset_reason_info_t reset_reason_classify(void);

// Returns a short, human-readable name for the classified reason.
const char *reset_reason_name(reset_reason_t r);

// Called from main() once the firmware has reached its main loop
// without crashing during init. Writes MAGIC_NORMAL_EXIT to scratch[0]
// so a subsequent watchdog reset can be distinguished from a hang
// during boot.
void reset_reason_mark_main_loop_entered(void);

// Called from the fault handler context (interrupts effectively
// disabled). Writes scratch[0..3] + the breadcrumb so the next boot
// can report what happened. Returns control; the caller is
// responsible for triggering the reset afterward.
void reset_reason_record_fault(uint32_t pc, uint32_t lr, uint32_t xpsr);
