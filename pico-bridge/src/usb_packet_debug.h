#pragma once

#include <stdint.h>

// Log host-to-device USB packets while the debug input persona is active.
// Normal personas never emit raw packet dumps.
void usb_packet_debug_note_out(const char *source, uint8_t const *buffer, uint16_t len);
