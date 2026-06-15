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

int main(void) {
    released_bootsel_finishes_setup_without_wipe();
    held_bootsel_waits_until_threshold();
    held_bootsel_after_threshold_keeps_saved_credentials();
    cold_boot_with_credentials_prefers_run_mode();
    cold_boot_without_credentials_enters_setup_mode();
    puts("boot_mode_policy tests passed");
    return 0;
}
