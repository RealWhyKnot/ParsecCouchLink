#pragma once

#include <stdbool.h>
#include <stdint.h>

// Log raw USB packets while the debug input persona is active. Normal
// personas always retain a bounded boot-time setup/control snapshot and emit
// continuous raw packet dumps only during a bundle-requested one-shot capture
// boot.
void usb_packet_debug_set_capture_enabled(bool enabled);
bool usb_packet_debug_capture_enabled(void);
void usb_packet_debug_note_out(const char *source, uint8_t const *buffer, uint16_t len);
void usb_packet_debug_note_out_report(const char *source, uint8_t report_id, uint8_t report_type,
                                      uint8_t const *buffer, uint16_t len);
void usb_packet_debug_note_in(const char *source, uint8_t const *buffer, uint16_t len,
                              bool changed);
void usb_packet_debug_note_in_accepted(const char *source, uint32_t bytes);
void usb_packet_debug_note_setup(const char *source, uint8_t bm_request_type, uint8_t b_request,
                                 uint16_t w_value, uint16_t w_index, uint16_t w_length);
void usb_packet_debug_note_hid_get_report(uint8_t instance, uint8_t report_id, uint8_t report_type,
                                          uint16_t request_len);
void usb_packet_debug_note_hid_set_report(uint8_t instance, uint8_t report_id, uint8_t report_type,
                                          uint16_t payload_len);
void usb_packet_debug_note_control_in(const char *source, uint8_t const *buffer, uint16_t len);
void usb_packet_debug_note_event(const char *event, const char *fields);
