#pragma once

#include <stdbool.h>
#include <stdint.h>

#define BOOT_MODE_BOOTSEL_HOLD_US 3000000LL

typedef enum {
    BOOTSEL_SETUP_WAIT = 0,
    BOOTSEL_SETUP_RETAIN_CREDS = 1,
} bootsel_setup_action_t;

typedef enum {
    BOOT_COLD_SETUP = 0,
    BOOT_COLD_RUN = 1,
} boot_cold_action_t;

// Which USB device the run-mode firmware presents. Latched at boot from
// the stored persona byte; see boot_mode_run_persona().
typedef enum {
    RUN_PERSONA_CONTROLLER = 0, // wired Xbox 360 / XInput (default)
    RUN_PERSONA_KEYBOARD = 1,   // USB HID boot keyboard
    RUN_PERSONA_MAPLE = 2,      // Dreamcast Maple controller
} run_persona_t;

bootsel_setup_action_t boot_mode_bootsel_setup_action(bool still_pressed, int64_t elapsed_us);
boot_cold_action_t boot_mode_cold_boot_action(bool have_creds);

// Map a stored persona byte to the run persona. A missing record or any
// unrecognised value falls back to the controller persona, so a blank or
// corrupt slot can never strand the device as a keyboard with no pad.
run_persona_t boot_mode_persona_from_flash(bool have_creds, uint8_t persona_byte);
