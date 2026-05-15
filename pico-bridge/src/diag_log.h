#pragma once

#include <stdint.h>
#include <stddef.h>

// Simple in-RAM ring buffer for diagnostic log lines. Read via CDC
// GET_LOG_BUFFER in setup mode, and dumped to a Pico-side support
// bundle in future versions. Keep messages short and stateful (boot,
// state transitions, errors) -- never log per-packet noise.

void diag_log_init(void);
void diag_log_msg(const char *msg);
void diag_log_printf(const char *fmt, ...);

// Copy up to `cap` bytes of the most recent log lines into `out`.
// Returns bytes written. Always NUL-free (no terminator added).
size_t diag_log_snapshot(uint8_t *out, size_t cap);
