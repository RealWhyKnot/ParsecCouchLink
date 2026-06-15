#pragma once

#include <stddef.h>
#include <stdint.h>

#include "maple_proto.h"

void maple_output_init(maple_map_mode_t mode);
void maple_output_task(void);
maple_controller_condition_t maple_output_current_condition(void);
size_t maple_output_build_response(const maple_request_t *request, uint8_t *out, size_t out_cap);
