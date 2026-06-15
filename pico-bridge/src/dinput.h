#pragma once

#include <stdint.h>

// Pump one 8BitDo Pro 2 D-Input HID report onto the USB endpoint when
// the host is ready. Only used when RUN_PERSONA_DINPUT is active.
void dinput_init(void);
void dinput_task(void);
void dinput_note_usb_reset(void);

// Fill the GET_REPORT control-transfer buffer with report 0x03's payload
// bytes, excluding the report ID byte that TinyUSB carries separately.
uint16_t dinput_get_report_payload(uint8_t *buffer, uint16_t reqlen);
