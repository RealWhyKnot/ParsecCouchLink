#include "boot_mode.h"

#include "pico/stdlib.h"
#include "hardware/structs/ioqspi.h"
#include "hardware/structs/sio.h"
#include "hardware/sync.h"
#include "hardware/gpio.h"
#include "tusb.h"

#include "boot_mode_policy.h"
#include "flash_creds.h"
#include "diag_log.h"
#include "reset_reason.h"

// The persona byte stored in flash is the run_persona_t value directly;
// boot_mode_persona_from_flash() relies on this so it can stay free of a
// flash_creds.h dependency (and thus host-compilable for unit tests).
_Static_assert((int)FLASH_PERSONA_XINPUT == (int)RUN_PERSONA_XINPUT &&
                   (int)FLASH_PERSONA_KEYBOARD == (int)RUN_PERSONA_KEYBOARD &&
                   (int)FLASH_PERSONA_MAPLE == (int)RUN_PERSONA_MAPLE &&
                   (int)FLASH_PERSONA_PS3 == (int)RUN_PERSONA_PS3 &&
                   (int)FLASH_PERSONA_PS4 == (int)RUN_PERSONA_PS4 &&
                   (int)FLASH_PERSONA_XBOXONE == (int)RUN_PERSONA_XBOXONE &&
                   (int)FLASH_PERSONA_DEBUG == (int)RUN_PERSONA_DEBUG &&
                   (int)FLASH_PERSONA_GENERIC_HID == (int)RUN_PERSONA_GENERIC_HID &&
                   (int)FLASH_PERSONA_N64 == (int)RUN_PERSONA_N64,
               "flash persona byte values must match run_persona_t");

static boot_mode_t current = BOOT_MODE_SETUP;

// Output persona for run mode, latched alongside `current` when a RUN
// decision is made. Setup mode leaves it at the XInput default.
static run_persona_t run_persona = RUN_PERSONA_XINPUT;

// t=0 BOOTSEL state, latched by boot_mode_decide().
static bool bootsel_at_boot = false;

// Absolute time when boot_mode_decide() returned. Used by the post-
// enum poll to settle a BOOTSEL recovery hold after USB is visible.
static absolute_time_t decide_time;

// Read the BOOTSEL button at runtime by temporarily flipping QSPI CS to
// Hi-Z and reading its pin state. Adapted from the pico-examples
// `picoboard/button` sample. Disables interrupts and must NOT call
// flash-resident code while CS is muxed.
static bool __no_inline_not_in_flash_func(read_bootsel_button)(void) {
    const uint CS_PIN_INDEX = 1;
    uint32_t flags = save_and_disable_interrupts();
    hw_write_masked(&ioqspi_hw->io[CS_PIN_INDEX].ctrl,
                    GPIO_OVERRIDE_LOW << IO_QSPI_GPIO_QSPI_SS_CTRL_OEOVER_LSB,
                    IO_QSPI_GPIO_QSPI_SS_CTRL_OEOVER_BITS);
    for (volatile int i = 0; i < 1000; i++)
        ;
    bool pressed = !(sio_hw->gpio_hi_in & (1u << CS_PIN_INDEX));
    hw_write_masked(&ioqspi_hw->io[CS_PIN_INDEX].ctrl,
                    GPIO_OVERRIDE_NORMAL << IO_QSPI_GPIO_QSPI_SS_CTRL_OEOVER_LSB,
                    IO_QSPI_GPIO_QSPI_SS_CTRL_OEOVER_BITS);
    restore_interrupts(flags);
    return pressed;
}

boot_mode_t boot_mode_decide(const reset_reason_info_t *rr) {
    bootsel_at_boot = false;
    decide_time = get_absolute_time();

    // Previous boot explicitly requested a setup-mode bounce (e.g. the
    // Wi-Fi association watchdog fired). Honor it regardless of creds.
    if (rr->force_setup_after_reboot) {
        diag_log_msg(
            "boot: previous boot requested setup-mode bounce; honoring with creds retained");
        current = BOOT_MODE_SETUP;
        return current;
    }

    if (rr->reason == RESET_REASON_FLASH_UPDATE) {
        flash_creds_t rec;
        bool have_creds = flash_creds_load(&rec);
        if (boot_mode_flash_update_action(have_creds) == BOOT_COLD_RUN) {
            diag_log_printf("boot: UF2 reflash detected with saved credentials (ssid_len=%u, "
                            "gen=%u); entering run mode",
                            (unsigned)rec.ssid_len, (unsigned)rec.generation);
            current = BOOT_MODE_RUN;
            run_persona = boot_mode_persona_from_flash(true, rec.usb_persona);
        } else {
            diag_log_msg("boot: UF2 reflash detected with no credentials; entering setup mode");
            current = BOOT_MODE_SETUP;
        }
        return current;
    }

    if (rr->reason == RESET_REASON_DELIBERATE) {
        diag_log_msg("boot: deliberate firmware reboot -- ignoring BOOTSEL sample");
        flash_creds_t rec;
        if (flash_creds_load(&rec)) {
            diag_log_printf("boot: credentials found (ssid_len=%u, gen=%u); entering run mode",
                            (unsigned)rec.ssid_len, (unsigned)rec.generation);
            current = BOOT_MODE_RUN;
            run_persona = boot_mode_persona_from_flash(true, rec.usb_persona);
        } else {
            diag_log_msg("boot: no valid credentials; entering setup mode");
            current = BOOT_MODE_SETUP;
        }
        return current;
    }

    flash_creds_t rec;
    bool have_creds = flash_creds_load(&rec);

    // Single instantaneous BOOTSEL sample -- no blocking delay. This
    // intentionally runs after reset-reason handling so a
    // firmware-driven reboot cannot be mistaken for a
    // physical BOOTSEL hold. A provisioned cold boot still enters run
    // mode; saved credentials are the stable XInput path.
    bootsel_at_boot = read_bootsel_button();

    if (bootsel_at_boot) {
        if (boot_mode_cold_boot_action(have_creds) == BOOT_COLD_RUN) {
            diag_log_printf("boot: BOOTSEL at boot with saved credentials (ssid_len=%u, gen=%u); "
                            "entering run mode",
                            (unsigned)rec.ssid_len, (unsigned)rec.generation);
            bootsel_at_boot = false;
            current = BOOT_MODE_RUN;
            run_persona = boot_mode_persona_from_flash(true, rec.usb_persona);
        } else {
            diag_log_msg("boot: BOOTSEL at boot with no credentials -- entering setup mode");
            current = BOOT_MODE_SETUP;
        }
        return current;
    }

    if (boot_mode_cold_boot_action(have_creds) == BOOT_COLD_RUN) {
        diag_log_printf("boot: credentials found (ssid_len=%u, gen=%u); entering run mode",
                        (unsigned)rec.ssid_len, (unsigned)rec.generation);
        current = BOOT_MODE_RUN;
        run_persona = boot_mode_persona_from_flash(true, rec.usb_persona);
    } else {
        diag_log_msg("boot: no valid credentials; entering setup mode");
        current = BOOT_MODE_SETUP;
    }
    return current;
}

boot_mode_t boot_mode_current(void) {
    return current;
}

run_persona_t boot_mode_run_persona(void) {
    return run_persona;
}

bool boot_mode_bootsel_at_boot(void) {
    return bootsel_at_boot;
}

void boot_mode_post_enum_bootsel_poll(void) {
    // Fast-exit: if BOOTSEL was not pressed at boot, there is no
    // post-enumeration recovery gesture to settle.
    if (!bootsel_at_boot)
        return;

    int64_t elapsed_us = absolute_time_diff_us(decide_time, get_absolute_time());
    bool still_pressed = read_bootsel_button();
    bootsel_setup_action_t action = boot_mode_bootsel_setup_action(still_pressed, elapsed_us);

    if (action == BOOTSEL_SETUP_WAIT)
        return;

    if (still_pressed) {
        diag_log_msg("boot: BOOTSEL held >= 3s -- setup mode, creds retained");
    } else {
        diag_log_msg("boot: BOOTSEL released before wipe threshold -- setup mode, creds retained");
    }

    bootsel_at_boot = false;
}
