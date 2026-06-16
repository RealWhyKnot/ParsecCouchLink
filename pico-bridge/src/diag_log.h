#pragma once

#include <stdint.h>
#include <stddef.h>

// Simple in-RAM ring buffer for diagnostic log lines. Read via CDC
// GET_LOG_BUFFER in setup mode, and dumped to a Pico-side support
// bundle in future versions. Keep messages short and stateful (boot,
// state transitions, errors) -- never log per-packet noise.

#define DIAG_LOG_RING_SIZE 16384u

void diag_log_init(void);
void diag_log_msg(const char *msg);
void diag_log_printf(const char *fmt, ...);

// Copy up to `cap` bytes of the most recent log lines into `out`.
// Returns bytes written. Always NUL-free (no terminator added). If
// `lost_out` is non-NULL, it receives the number of bytes that were
// dropped from the ring before the snapshot started -- a non-zero
// value means an earlier burst overflowed the ring and the host is
// seeing only the tail.
size_t diag_log_snapshot(uint8_t *out, size_t cap, uint32_t *lost_out);

// Copy the most-recent committed log line (without the trailing
// newline) into `out`, NUL-terminating if there is room. Returns the
// number of bytes written (excluding the terminator) or 0 if the
// ring is empty.
size_t diag_log_copy_last_line(char *out, size_t cap);

// Lock-free variant of diag_log_copy_last_line for use from a fault
// handler context, where entering the ring's critical section could
// deadlock against an interrupted writer. The cost is that a writer
// torn mid-byte may produce a single garbled character at the end of
// the copied line; for a post-mortem breadcrumb that is acceptable
// because the alternative is no last line at all.
size_t diag_log_copy_last_line_unsafe(char *out, size_t cap);
