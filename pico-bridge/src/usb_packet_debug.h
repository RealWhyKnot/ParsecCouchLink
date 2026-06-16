#pragma once

#include <stdbool.h>
#include <stdint.h>

// Log raw USB packets while the debug input persona is active. Normal
// personas never emit raw packet dumps.
void usb_packet_debug_note_out(const char *source, uint8_t const *buffer, uint16_t len);
void usb_packet_debug_note_in(const char *source, uint8_t const *buffer, uint16_t len,
                              bool changed);
void usb_packet_debug_note_setup(const char *source, uint8_t bm_request_type, uint8_t b_request,
                                 uint16_t w_value, uint16_t w_index, uint16_t w_length);
void usb_packet_debug_note_control_in(const char *source, uint8_t const *buffer, uint16_t len);
