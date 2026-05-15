#include "watchdog.h"

#include <string.h>

#include "pico/stdlib.h"

#include "gamepad_state.h"
#include "diag_log.h"

static bool zeroed = false;

void watchdog_init(void) {
    zeroed = false;
    g_last_packet_ms = 0;
}

void watchdog_tick(void) {
    uint32_t now = to_ms_since_boot(get_absolute_time());
    uint32_t last = g_last_packet_ms; // atomic 32-bit load
    bool stale = (last == 0) || ((uint32_t)(now - last) > PICO_BRIDGE_WATCHDOG_TIMEOUT_MS);
    if (stale) {
        if (!zeroed) {
            // Edge-trigger: log once, neutralize state, then stay quiet
            // until the bridge starts talking again.
            zeroed = true;
            g_gamepad_state.buttons = 0;
            g_gamepad_state.left_trigger = 0;
            g_gamepad_state.right_trigger = 0;
            g_gamepad_state.left_x = 0;
            g_gamepad_state.left_y = 0;
            g_gamepad_state.right_x = 0;
            g_gamepad_state.right_y = 0;
            g_parsec_connected = 0;
            diag_log_msg("watchdog: no bridge packets for 100 ms, neutralized");
        }
    } else if (zeroed) {
        zeroed = false;
        diag_log_msg("watchdog: bridge back online");
    }
}
