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

bootsel_setup_action_t boot_mode_bootsel_setup_action(bool still_pressed, int64_t elapsed_us);
boot_cold_action_t boot_mode_cold_boot_action(bool have_creds);
