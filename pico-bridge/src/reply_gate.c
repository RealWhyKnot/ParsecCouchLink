#include "reply_gate.h"

bool reply_gate_allow(reply_gate_t *gate, uint32_t now_ms, uint32_t interval_ms, uint32_t burst) {
    if (interval_ms == 0 || burst == 0)
        return false;
    uint32_t window = interval_ms * burst;
    uint32_t ahead = gate->next_ms - now_ms;
    if (!gate->armed || ahead > window) {
        gate->armed = true;
        gate->next_ms = now_ms;
        ahead = 0;
    }
    if (ahead > window - interval_ms)
        return false;
    gate->next_ms += interval_ms;
    return true;
}
