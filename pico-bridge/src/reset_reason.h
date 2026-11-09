#pragma once

#include <stdbool.h>
#include <stdint.h>

#define RESET_REASON_MAGIC_NORMAL_EXIT 0xB007C1EAu
#define RESET_REASON_MAGIC_FAULT 0xFA0FAEDDu

#define RESET_REASON_LAST_LINE_CAP 56

typedef enum {
    RESET_REASON_UNKNOWN = 0,
    RESET_REASON_COLD_OR_PIN,
    RESET_REASON_FLASH_UPDATE,
    RESET_REASON_DELIBERATE,
    RESET_REASON_WATCHDOG_HANG,
    RESET_REASON_FAULT,
} reset_reason_t;

typedef struct {
    reset_reason_t reason;
    uint32_t fault_pc;
    uint32_t fault_lr;
    uint32_t fault_xpsr;
    bool full_frame_valid;
    uint32_t fault_r0, fault_r1, fault_r2, fault_r3, fault_r12;
    uint32_t fault_sp;
    bool fault_status_valid;
    uint32_t fault_cfsr;
    uint32_t fault_hfsr;
    uint32_t fault_mmfar;
    uint32_t fault_bfar;
    bool last_line_valid;
    char last_line[RESET_REASON_LAST_LINE_CAP];
    bool force_setup_after_reboot;
} reset_reason_info_t;

reset_reason_info_t reset_reason_classify(void);

const char *reset_reason_name(reset_reason_t r);

void reset_reason_mark_main_loop_entered(void);

void reset_reason_request_setup_after_reboot(void);

void reset_reason_request_setup_with_note(const char *note);

bool reset_reason_force_setup_requested(void);

void reset_reason_request_usb_capture_after_reboot(uint8_t persona);

bool reset_reason_consume_usb_capture_request(uint8_t *persona);

void reset_reason_record_fault(const uint32_t *frame, uint32_t sp_at_fault);
