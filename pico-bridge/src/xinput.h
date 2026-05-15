#pragma once

#include <stdbool.h>

// Pump one XInput IN report onto the USB endpoint if the host has
// claimed a buffer slot. Cheap to call every iteration of the main
// loop; cooperates with TinyUSB's internal scheduling.
void xinput_init(void);
void xinput_task(void);
