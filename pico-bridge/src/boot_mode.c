#include "boot_mode.h"

#include "pico/stdlib.h"
#include "hardware/structs/ioqspi.h"
#include "hardware/structs/sio.h"
#include "hardware/sync.h"
#include "hardware/gpio.h"
#include "tusb.h"

#include "flash_creds.h"
#include "diag_log.h"

static boot_mode_t current = BOOT_MODE_SETUP;

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

bool boot_mode_bootsel_held(void) {
    // Wait 3 seconds, then sample once. The recovery procedure is to
    // unplug, plug back in, and hold BOOTSEL for the first 3 seconds.
    // The delay avoids wiping creds on an accidental tap. tud_task()
    // is pumped during the wait so USB enumeration can run in parallel
    // -- caller must have already invoked tusb_init().
    const uint32_t wait_ms = 3000;
    absolute_time_t deadline = make_timeout_time_ms(wait_ms);
    while (!time_reached(deadline)) {
        tud_task();
        sleep_us(500);
    }
    bool held = read_bootsel_button();
    diag_log_printf("boot: BOOTSEL window done (held=%s, waited=%u ms)",
                    held ? "yes" : "no", (unsigned)wait_ms);
    return held;
}

boot_mode_t boot_mode_decide(void) {
    if (boot_mode_bootsel_held()) {
        diag_log_msg("boot: BOOTSEL held at startup; clearing creds and entering setup mode");
        flash_creds_clear();
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
