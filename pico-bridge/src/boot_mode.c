#include "boot_mode.h"

#include "pico/stdlib.h"
#include "hardware/structs/ioqspi.h"
#include "hardware/structs/sio.h"
#include "hardware/sync.h"
#include "hardware/gpio.h"
#include "hardware/watchdog.h"
#include "tusb.h"

#include "flash_creds.h"
#include "diag_log.h"
#include "reset_reason.h"

static boot_mode_t current = BOOT_MODE_SETUP;

// t=0 BOOTSEL state, latched by boot_mode_decide().
static bool bootsel_at_boot = false;

// Absolute time when boot_mode_decide() returned. Used by the post-
// enum poll to bound the 3-second wipe window.
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
    for (volatile int i = 0; i < 1000; i++);
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
        diag_log_msg("boot: previous boot requested setup-mode bounce; honoring with creds retained");
        current = BOOT_MODE_SETUP;
        return current;
    }

    if (rr->reason == RESET_REASON_FLASH_UPDATE) {
        diag_log_msg("boot: UF2 reflash detected -- forcing setup mode with creds retained");
        current = BOOT_MODE_SETUP;
        return current;
    }

    if (rr->reason == RESET_REASON_DELIBERATE) {
        diag_log_msg("boot: deliberate firmware reboot -- ignoring BOOTSEL sample");
        flash_creds_t rec;
        if (flash_creds_load(&rec)) {
            diag_log_printf("boot: credentials found (ssid_len=%u, gen=%u); entering run mode",
                            (unsigned)rec.ssid_len, (unsigned)rec.generation);
            current = BOOT_MODE_RUN;
        } else {
            diag_log_msg("boot: no valid credentials; entering setup mode");
            current = BOOT_MODE_SETUP;
        }
        return current;
    }

    // Single instantaneous BOOTSEL sample -- no blocking delay.
    // The post-enum poll tracks a 3-second hold after enumeration to
    // decide whether to wipe credentials. This intentionally runs after
    // reset-reason handling so an RP2350 UF2 reflash or firmware-driven
    // reboot cannot be mistaken for a physical BOOTSEL hold.
    bootsel_at_boot = read_bootsel_button();

    if (bootsel_at_boot) {
        // BOOTSEL is pressed right now. Force setup mode immediately.
        // Whether creds get wiped depends on how long BOOTSEL stays held:
        //   < 3 s: setup mode, creds retained (brief-tap recovery tier).
        //   >= 3 s: setup mode, creds wiped (post-enum poll fires).
        diag_log_msg("boot: BOOTSEL at boot -- forcing setup mode (creds retained for now)");
        current = BOOT_MODE_SETUP;
        return current;
    }

    flash_creds_t rec;
    if (flash_creds_load(&rec)) {
        diag_log_printf("boot: credentials found (ssid_len=%u, gen=%u); entering run mode",
                        (unsigned)rec.ssid_len, (unsigned)rec.generation);
        current = BOOT_MODE_RUN;
    } else {
        diag_log_msg("boot: no valid credentials; entering setup mode");
        current = BOOT_MODE_SETUP;
    }
    return current;
}

boot_mode_t boot_mode_current(void) {
    return current;
}

bool boot_mode_bootsel_at_boot(void) {
    return bootsel_at_boot;
}

void boot_mode_post_enum_bootsel_poll(void) {
    // Fast-exit: if BOOTSEL was not pressed at boot, the 3-second wipe
    // window is irrelevant. Also exit after 3 seconds -- decision settled.
    if (!bootsel_at_boot) return;

    int64_t elapsed_us = absolute_time_diff_us(decide_time, get_absolute_time());

    if (!read_bootsel_button()) {
        // User released BOOTSEL before the wipe threshold. Become a
        // permanent no-op; setup mode remains active with creds retained.
        diag_log_msg("boot: BOOTSEL released before wipe threshold -- setup mode, creds retained");
        bootsel_at_boot = false;
        return;
    }

    if (elapsed_us < 3000000) return;

    // BOOTSEL has been held continuously since boot for >= 3 seconds.
    // This matches the old blocking-wait behavior: wipe creds and reboot.
    diag_log_msg("boot: BOOTSEL held >= 3s -- wiping creds and rebooting to setup mode");
    flash_creds_clear();
    watchdog_reboot(0, 0, 100);
    for (;;) tight_loop_contents();
}
