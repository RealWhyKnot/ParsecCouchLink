#pragma once

#include <stdbool.h>

#include "boot_mode_policy.h"
#include "reset_reason.h"

typedef enum {
    BOOT_MODE_SETUP = 0, // CDC, no Wi-Fi
    BOOT_MODE_RUN = 1,   // XInput or keyboard, Wi-Fi
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

// Which USB device run mode presents, latched from the stored persona
// byte when boot_mode_decide() selects BOOT_MODE_RUN. Always
// RUN_PERSONA_CONTROLLER in setup mode. Stable for the rest of the boot,
// so the descriptor callbacks and the run loop can read it freely.
run_persona_t boot_mode_run_persona(void);

// True if BOOTSEL was pressed at the moment boot_mode_decide() ran.
// Used by boot_mode_post_enum_bootsel_poll() to finish the setup-mode
// recovery gesture after USB has enumerated.
bool boot_mode_bootsel_at_boot(void);

// Call from both main loops once per iteration. Tracks elapsed time
// since boot_mode_decide() returned. When BOOTSEL was held at t=0,
// setup mode remains active and saved Wi-Fi credentials are retained.
// Once BOOTSEL is released or the hold threshold passes, this becomes
// a no-op.
void boot_mode_post_enum_bootsel_poll(void);
