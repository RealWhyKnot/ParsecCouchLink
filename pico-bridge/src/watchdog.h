#pragma once

#include <stdint.h>

#define PICO_BRIDGE_WATCHDOG_TIMEOUT_MS 100

void watchdog_init(void);
void watchdog_tick(void);
