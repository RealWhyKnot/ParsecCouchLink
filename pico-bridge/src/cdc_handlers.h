#pragma once

#include "cdc_proto.h"

// Dispatch a fully-decoded request frame. Writes a response frame into
// `reply` (one of CDC_RSP_*, or CDC_RSP_NACK on error). Returns the
// number of bytes written to `reply`.
size_t cdc_dispatch(const cdc_frame_view_t *req, uint8_t *reply, size_t reply_cap);

// Pump CDC reads/writes once. Call from the main loop. Buffers incoming
// bytes, dispatches whenever a full frame arrives.
void cdc_handlers_poll(void);

// Initialise state. Call once after TinyUSB is up.
void cdc_handlers_init(void);

// Returns true if a REBOOT_TO_RUN was queued; once the TX queue drains,
// the caller is expected to perform the reboot.
bool cdc_handlers_reboot_pending(void);

// Returns true if a REBOOT_TO_BOOTSEL was queued; once the TX queue
// drains, the caller is expected to enter the ROM bootloader.
bool cdc_handlers_bootsel_pending(void);
