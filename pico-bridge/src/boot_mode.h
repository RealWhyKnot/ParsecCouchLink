#pragma once

#include <stdbool.h>

#include "boot_mode_policy.h"
#include "reset_reason.h"

typedef enum {
    BOOT_MODE_SETUP = 0,
    BOOT_MODE_RUN = 1,
} boot_mode_t;

boot_mode_t boot_mode_decide(const reset_reason_info_t *rr);

boot_mode_t boot_mode_current(void);

run_persona_t boot_mode_run_persona(void);

bool boot_mode_bootsel_at_boot(void);

void boot_mode_post_enum_bootsel_poll(void);
