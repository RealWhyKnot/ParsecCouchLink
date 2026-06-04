#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef struct {
    bool mounted;
    bool suspended;
    uint8_t last_out_len;
    uint8_t last_out_byte0;
    uint8_t last_out_byte1;
    uint32_t now_ms;
    uint32_t mount_count;
    uint32_t umount_count;
    uint32_t suspend_count;
    uint32_t resume_count;
    uint32_t device_desc_count;
    uint32_t config_desc_count;
    uint32_t xinput_in_queued_count;
    uint32_t xinput_in_sent_count;
    uint32_t xinput_out_count;
    uint32_t last_mount_ms;
    uint32_t last_umount_ms;
    uint32_t last_in_queued_ms;
    uint32_t last_in_sent_ms;
    uint32_t last_out_ms;
} usb_diag_snapshot_t;

void usb_diag_init(void);
void usb_diag_note_device_descriptor(void);
void usb_diag_note_configuration_descriptor(void);
void usb_diag_note_mount(void);
void usb_diag_note_umount(void);
void usb_diag_note_suspend(void);
void usb_diag_note_resume(void);
void usb_diag_note_xinput_in_queued(uint32_t bytes);
void usb_diag_note_xinput_in_sent(uint32_t bytes);
void usb_diag_note_xinput_out(uint8_t const *buffer, uint16_t len);
void usb_diag_snapshot(usb_diag_snapshot_t *out);
