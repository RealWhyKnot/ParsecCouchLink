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

typedef enum {
    RUN_PERSONA_XINPUT = 0,
    RUN_PERSONA_KEYBOARD = 1,
    RUN_PERSONA_MAPLE = 2,
    RUN_PERSONA_PS3 = 3,
    RUN_PERSONA_PS4 = 4,
    RUN_PERSONA_XBOXONE = 5,
    RUN_PERSONA_DEBUG = 6,
    RUN_PERSONA_GENERIC_HID = 7,
    RUN_PERSONA_BT_HID = 8,
    RUN_PERSONA_BT_XBOX = 9,
    RUN_PERSONA_BT_PS = 10,
} run_persona_t;

bootsel_setup_action_t boot_mode_bootsel_setup_action(bool still_pressed, int64_t elapsed_us);
boot_cold_action_t boot_mode_cold_boot_action(bool have_creds);
boot_cold_action_t boot_mode_flash_update_action(bool have_creds);

run_persona_t boot_mode_persona_from_flash(bool have_creds, uint8_t persona_byte);

bool boot_mode_persona_uses_xinput_usb(run_persona_t persona);

bool boot_mode_persona_uses_gamepad_hid(run_persona_t persona);

bool boot_mode_persona_uses_usb_output(run_persona_t persona);

bool boot_mode_persona_uses_bluetooth(run_persona_t persona);
