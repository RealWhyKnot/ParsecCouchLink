#pragma once

#include <stdint.h>
#include <stddef.h>

#define DIAG_LOG_RING_SIZE 16384u

void diag_log_init(void);
void diag_log_msg(const char *msg);
void diag_log_printf(const char *fmt, ...);

size_t diag_log_snapshot(uint8_t *out, size_t cap, uint32_t *lost_out);

size_t diag_log_copy_last_line(char *out, size_t cap);

size_t diag_log_copy_last_line_unsafe(char *out, size_t cap);
