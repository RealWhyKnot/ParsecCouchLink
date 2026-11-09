#pragma once

#include "cdc_proto.h"

size_t cdc_dispatch(const cdc_frame_view_t *req, uint8_t *reply, size_t reply_cap);

void cdc_handlers_poll(void);

void cdc_handlers_init(void);

bool cdc_handlers_reboot_pending(void);

bool cdc_handlers_bootsel_pending(void);
