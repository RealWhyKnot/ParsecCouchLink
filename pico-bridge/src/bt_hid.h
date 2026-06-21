#pragma once

#include <stdbool.h>

#include "boot_mode_policy.h"
#include "bt_hid_report.h"

bool bt_hid_target_from_persona(run_persona_t persona, bt_hid_target_t *out);
bool bt_hid_init(bt_hid_target_t target);
void bt_hid_reset_stack_state(void);
void bt_hid_task(void);
