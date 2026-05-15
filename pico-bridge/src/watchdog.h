#pragma once

#include <stdint.h>

// 100 ms timeout. If no valid UDP datagram arrives within that window,
// the gamepad state is forced to neutral so the console sees no held
// buttons.
#define PICO_BRIDGE_WATCHDOG_TIMEOUT_MS 100

void watchdog_init(void);
// Call from main loop. Cheap if last update is still fresh.
void watchdog_tick(void);
