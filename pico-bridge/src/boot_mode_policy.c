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
