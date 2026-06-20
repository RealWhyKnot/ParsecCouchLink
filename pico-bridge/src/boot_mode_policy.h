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
    RUN_PERSONA_XINPUT = 0,      // wired Xbox 360 / XInput (default)
    RUN_PERSONA_KEYBOARD = 1,    // USB HID boot keyboard
    RUN_PERSONA_MAPLE = 2,       // Xbox-compatible Dreamcast Maple adapter mode
    RUN_PERSONA_PS3 = 3,         // Sony DualShock 3 / PS3 HID gamepad
    RUN_PERSONA_PS4 = 4,         // Sony DualShock 4 / PS4 HID gamepad
    RUN_PERSONA_XBOXONE = 5,     // Xbox One-compatible XGIP gamepad
    RUN_PERSONA_DEBUG = 6,       // XInput shape with raw USB packet capture
    RUN_PERSONA_GENERIC_HID = 7, // Generic HID gamepad for unknown-HID adapters
} run_persona_t;

bootsel_setup_action_t boot_mode_bootsel_setup_action(bool still_pressed, int64_t elapsed_us);
boot_cold_action_t boot_mode_cold_boot_action(bool have_creds);
boot_cold_action_t boot_mode_flash_update_action(bool have_creds);

// Map a stored persona byte to the run persona. A missing record or any
// unrecognised value falls back to the XInput persona, so a blank or
// corrupt slot can never strand the device as a keyboard with no gamepad.
run_persona_t boot_mode_persona_from_flash(bool have_creds, uint8_t persona_byte);

// True for runtime personas that present the wired Xbox 360 USB device
// shape. Maple mode keeps a separate persisted label, but deliberately
// matches this USB shape for Dreamcast Maple adapters that already
// support Xbox 360 controllers. HID gamepad and Xbox One XGIP personas
// use distinct USB shapes and return false.
bool boot_mode_persona_uses_xinput_usb(run_persona_t persona);

// True for runtime personas that present a HID gamepad interface.
bool boot_mode_persona_uses_gamepad_hid(run_persona_t persona);
