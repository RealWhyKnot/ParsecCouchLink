#pragma once

#include <stdbool.h>

typedef enum {
    BOOT_MODE_SETUP = 0,   // CDC, no Wi-Fi
    BOOT_MODE_RUN   = 1,   // XInput, Wi-Fi
} boot_mode_t;

// Decide which mode to boot into. Reads stored credentials and applies
// the BOOTSEL-recovery check. Call exactly once, early in main.
boot_mode_t boot_mode_decide(void);

// What was decided. Stable for the rest of the boot.
boot_mode_t boot_mode_current(void);

// Sample BOOTSEL once after a 3-second delay. Returns true if pressed.
bool boot_mode_bootsel_held(void);
