#include "boot_mode_policy.h"

bootsel_setup_action_t boot_mode_bootsel_setup_action(bool still_pressed, int64_t elapsed_us) {
    if (!still_pressed)
        return BOOTSEL_SETUP_RETAIN_CREDS;
    if (elapsed_us >= BOOT_MODE_BOOTSEL_HOLD_US)
        return BOOTSEL_SETUP_RETAIN_CREDS;
    return BOOTSEL_SETUP_WAIT;
}

boot_cold_action_t boot_mode_cold_boot_action(bool have_creds) {
    return have_creds ? BOOT_COLD_RUN : BOOT_COLD_SETUP;
}

run_persona_t boot_mode_persona_from_flash(bool have_creds, uint8_t persona_byte) {
    // The stored flash byte is the run_persona_t value itself
    // (FLASH_PERSONA_* == RUN_PERSONA_*, asserted in boot_mode.c), so no
    // flash_creds.h dependency is needed here -- which keeps this module
    // host-compilable for the unit tests.
    if (!have_creds)
        return RUN_PERSONA_CONTROLLER;
    if (persona_byte == (uint8_t)RUN_PERSONA_KEYBOARD)
        return RUN_PERSONA_KEYBOARD;
    return RUN_PERSONA_CONTROLLER;
}

bool boot_mode_persona_uses_xinput_usb(run_persona_t persona) {
    return persona == RUN_PERSONA_CONTROLLER;
}
