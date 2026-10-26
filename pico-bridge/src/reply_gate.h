#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef struct {
    bool armed;
    uint32_t next_ms;
} reply_gate_t;

bool reply_gate_allow(reply_gate_t *gate, uint32_t now_ms, uint32_t interval_ms, uint32_t burst);
