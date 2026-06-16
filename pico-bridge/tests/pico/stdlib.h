#pragma once

#include <stdint.h>

typedef uint32_t absolute_time_t;

absolute_time_t get_absolute_time(void);
uint32_t to_ms_since_boot(absolute_time_t t);
