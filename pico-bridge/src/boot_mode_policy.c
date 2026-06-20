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

boot_cold_action_t boot_mode_flash_update_action(bool have_creds) {
    return boot_mode_cold_boot_action(have_creds);
}

run_persona_t boot_mode_persona_from_flash(bool have_creds, uint8_t persona_byte) {
    // The stored flash byte is the run_persona_t value itself
    // (FLASH_PERSONA_* == RUN_PERSONA_*, asserted in boot_mode.c), so no
    // flash_creds.h dependency is needed here -- which keeps this module
    // host-compilable for the unit tests.
    if (!have_creds)
        return RUN_PERSONA_XINPUT;
    if (persona_byte == (uint8_t)RUN_PERSONA_KEYBOARD)
        return RUN_PERSONA_KEYBOARD;
    if (persona_byte == (uint8_t)RUN_PERSONA_MAPLE)
        return RUN_PERSONA_MAPLE;
    if (persona_byte == (uint8_t)RUN_PERSONA_PS3)
        return RUN_PERSONA_PS3;
    if (persona_byte == (uint8_t)RUN_PERSONA_PS4)
        return RUN_PERSONA_PS4;
    if (persona_byte == (uint8_t)RUN_PERSONA_XBOXONE)
        return RUN_PERSONA_XBOXONE;
    if (persona_byte == (uint8_t)RUN_PERSONA_DEBUG)
        return RUN_PERSONA_DEBUG;
    if (persona_byte == (uint8_t)RUN_PERSONA_GENERIC_HID)
        return RUN_PERSONA_GENERIC_HID;
    if (persona_byte == (uint8_t)RUN_PERSONA_N64)
        return RUN_PERSONA_N64;
    return RUN_PERSONA_XINPUT;
}

bool boot_mode_persona_uses_xinput_usb(run_persona_t persona) {
    return persona == RUN_PERSONA_XINPUT || persona == RUN_PERSONA_MAPLE ||
           persona == RUN_PERSONA_DEBUG;
}

bool boot_mode_persona_uses_gamepad_hid(run_persona_t persona) {
    return persona == RUN_PERSONA_PS3 || persona == RUN_PERSONA_PS4 ||
           persona == RUN_PERSONA_GENERIC_HID;
}
