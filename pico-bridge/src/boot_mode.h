#pragma once

#include <stdbool.h>

#include "reset_reason.h"

typedef enum {
    BOOT_MODE_SETUP = 0,   // CDC, no Wi-Fi
    BOOT_MODE_RUN   = 1,   // XInput, Wi-Fi
} boot_mode_t;

// Decide which mode to boot into. Reads reset context from `rr`
// (including force-setup bounces and RP2350 UF2 reflash detection),
// samples BOOTSEL once when reset context does not already force the
// answer, and checks stored credentials. Call exactly once, early in main, BEFORE
// tusb_init() -- the D+ pull-up must not be asserted until this
// returns so the host sees a single clean connect event with the
// correct descriptor persona.
boot_mode_t boot_mode_decide(const reset_reason_info_t *rr);

// What was decided. Stable for the rest of the boot.
boot_mode_t boot_mode_current(void);

// True if BOOTSEL was pressed at the moment boot_mode_decide() ran.
// Used by boot_mode_post_enum_bootsel_poll() to decide whether to
// start the wipe timer.
bool boot_mode_bootsel_at_boot(void);

// Call from both main loops once per iteration. Tracks elapsed time
// since boot_mode_decide() returned. When BOOTSEL was held at t=0 AND
// has been continuously held for >= 3 seconds, wipes credentials and
// reboots into setup mode (same net effect as the old blocking wait,
// but the USB enumeration is not disrupted). Once BOOTSEL is released
// or the wipe fires this becomes a no-op -- the wipe-or-not decision is settled.
void boot_mode_post_enum_bootsel_poll(void);
