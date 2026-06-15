#include <assert.h>
#include <stdio.h>

#include "boot_mode_policy.h"

static void released_bootsel_finishes_setup_without_wipe(void) {
    assert(boot_mode_bootsel_setup_action(false, 0) == BOOTSEL_SETUP_RETAIN_CREDS);
    assert(boot_mode_bootsel_setup_action(false, BOOT_MODE_BOOTSEL_HOLD_US - 1) ==
           BOOTSEL_SETUP_RETAIN_CREDS);
}

static void held_bootsel_waits_until_threshold(void) {
    assert(boot_mode_bootsel_setup_action(true, 0) == BOOTSEL_SETUP_WAIT);
    assert(boot_mode_bootsel_setup_action(true, BOOT_MODE_BOOTSEL_HOLD_US - 1) ==
           BOOTSEL_SETUP_WAIT);
}

static void held_bootsel_after_threshold_keeps_saved_credentials(void) {
    assert(boot_mode_bootsel_setup_action(true, BOOT_MODE_BOOTSEL_HOLD_US) ==
           BOOTSEL_SETUP_RETAIN_CREDS);
    assert(boot_mode_bootsel_setup_action(true, BOOT_MODE_BOOTSEL_HOLD_US + 1) ==
           BOOTSEL_SETUP_RETAIN_CREDS);
}

static void cold_boot_with_credentials_prefers_run_mode(void) {
    assert(boot_mode_cold_boot_action(true) == BOOT_COLD_RUN);
}

static void cold_boot_without_credentials_enters_setup_mode(void) {
    assert(boot_mode_cold_boot_action(false) == BOOT_COLD_SETUP);
}

static void persona_defaults_to_controller_without_credentials(void) {
    // No credentials -> always controller, whatever byte happens to be there.
    // The stored byte equals the run_persona_t value (asserted in boot_mode.c).
    assert(boot_mode_persona_from_flash(false, RUN_PERSONA_KEYBOARD) == RUN_PERSONA_CONTROLLER);
    assert(boot_mode_persona_from_flash(false, 0) == RUN_PERSONA_CONTROLLER);
}

static void persona_reads_stored_byte_with_credentials(void) {
    // Byte 0 is the controller default a pre-persona record reads back as.
    assert(boot_mode_persona_from_flash(true, RUN_PERSONA_CONTROLLER) == RUN_PERSONA_CONTROLLER);
    assert(boot_mode_persona_from_flash(true, RUN_PERSONA_KEYBOARD) == RUN_PERSONA_KEYBOARD);
    assert(boot_mode_persona_from_flash(true, RUN_PERSONA_MAPLE) == RUN_PERSONA_MAPLE);
}

static void persona_unknown_byte_falls_back_to_controller(void) {
    // A corrupt/garbage byte must never strand the device in a non-controller mode.
    assert(boot_mode_persona_from_flash(true, 0x5A) == RUN_PERSONA_CONTROLLER);
    assert(boot_mode_persona_from_flash(true, 0xFF) == RUN_PERSONA_CONTROLLER);
}

int main(void) {
    released_bootsel_finishes_setup_without_wipe();
    held_bootsel_waits_until_threshold();
    held_bootsel_after_threshold_keeps_saved_credentials();
    cold_boot_with_credentials_prefers_run_mode();
    cold_boot_without_credentials_enters_setup_mode();
    persona_defaults_to_controller_without_credentials();
    persona_reads_stored_byte_with_credentials();
    persona_unknown_byte_falls_back_to_controller();
    puts("boot_mode_policy tests passed");
    return 0;
}
