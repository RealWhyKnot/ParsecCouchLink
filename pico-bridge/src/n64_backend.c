#include "n64_backend.h"

#include <stdbool.h>
#include <stdint.h>

#include "hardware/pio.h"
#include "hardware/sync.h"
#include "pico/multicore.h"
#include "pico/stdlib.h"

#include "joybus/backend/rp2xxx.h"
#include "joybus/joybus.h"
#include "joybus/target/n64_controller.h"

#include "diag_log.h"
#include "gamepad_state.h"
#include "n64_report.h"

#ifndef PICO_BRIDGE_N64_GPIO
#define PICO_BRIDGE_N64_GPIO 0
#endif

static struct joybus_rp2xxx joybus_bus;
static struct joybus_target_n64_controller n64_controller;
static volatile uint32_t staged_report;
static volatile int core1_status;
static volatile bool core1_started;
static bool status_logged;

static void n64_core1_entry(void) {
    multicore_lockout_victim_init();

    struct joybus *bus = JOYBUS(&joybus_bus);
    int rc = joybus_rp2xxx_init(&joybus_bus, PICO_BRIDGE_N64_GPIO, pio0);
    if (rc != 0) {
        core1_status = rc;
        core1_started = true;
        for (;;)
            tight_loop_contents();
    }

    joybus_bus.data.target_freq = JOYBUS_FREQ_N64_CONTROLLER;
    joybus_target_n64_controller_init(&n64_controller);
    joybus_target_n64_controller_detach_pak(&n64_controller);

    rc = joybus_target_register(bus, JOYBUS_TARGET(&n64_controller));
    if (rc == 0)
        rc = joybus_enable(bus);
    core1_status = rc;
    core1_started = true;

    if (rc != 0) {
        for (;;)
            tight_loop_contents();
    }

    for (;;) {
        struct joybus_n64_controller_state state = n64_report_unpack(staged_report);
        uint32_t flags = save_and_disable_interrupts();
        n64_controller.input = state;
        restore_interrupts(flags);
        sleep_us(500);
    }
}

void n64_backend_init(void) {
    struct joybus_n64_controller_state neutral = {0};
    staged_report = n64_report_pack(&neutral);
    core1_status = -1;
    core1_started = false;
    status_logged = false;
    diag_log_printf("n64: starting Joybus target on GPIO %u", (unsigned)PICO_BRIDGE_N64_GPIO);
    multicore_launch_core1(n64_core1_entry);
}

void n64_backend_task(void) {
    gamepad_state_t state;
    state.buttons = g_gamepad_state.buttons;
    state.left_trigger = g_gamepad_state.left_trigger;
    state.right_trigger = g_gamepad_state.right_trigger;
    state.left_x = g_gamepad_state.left_x;
    state.left_y = g_gamepad_state.left_y;
    state.right_x = g_gamepad_state.right_x;
    state.right_y = g_gamepad_state.right_y;

    struct joybus_n64_controller_state n64_state = n64_report_from_gamepad(&state);
    staged_report = n64_report_pack(&n64_state);

    if (!status_logged && core1_started) {
        status_logged = true;
        if (core1_status == 0) {
            diag_log_msg("n64: Joybus target ready");
        } else {
            diag_log_printf("n64: Joybus target failed rc=%d", core1_status);
        }
    }
}
