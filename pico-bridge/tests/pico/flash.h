#pragma once

#include <stdint.h>

#define PICO_OK 0

int flash_safe_execute(void (*func)(void *), void *param, uint32_t enter_exit_timeout_ms);
