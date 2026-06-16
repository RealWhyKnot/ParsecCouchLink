#pragma once

#include <stdint.h>

void xbone_init(void);
void xbone_task(void);
void xbone_note_usb_reset(void);
void xbone_on_out(uint8_t const *buffer, uint16_t len);
