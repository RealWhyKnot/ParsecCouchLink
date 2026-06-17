#pragma once

#include <stdint.h>

// Pump one gamepad HID report onto the USB endpoint when the host is ready.
// Only used when a gamepad HID persona is active.
void dinput_init(void);
void dinput_task(void);
void dinput_note_usb_reset(void);

uint16_t dinput_get_report_payload(uint8_t report_id, uint8_t report_type, uint8_t *buffer,
                                   uint16_t reqlen);
void dinput_set_report(uint8_t report_id, uint8_t report_type, uint8_t const *buffer,
                       uint16_t bufsize);
