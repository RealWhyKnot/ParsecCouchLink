#include <assert.h>
#include <stdint.h>
#include <stdio.h>

#include "reply_gate.h"

#define INTERVAL_MS 400u
#define BURST 3u

static void burst_is_allowed_then_denied(uint32_t start) {
    reply_gate_t gate = {0};
    for (uint32_t i = 0; i < BURST; i++)
        assert(reply_gate_allow(&gate, start, INTERVAL_MS, BURST));
    assert(!reply_gate_allow(&gate, start, INTERVAL_MS, BURST));
    assert(!reply_gate_allow(&gate, start + INTERVAL_MS - 1u, INTERVAL_MS, BURST));
    assert(reply_gate_allow(&gate, start + INTERVAL_MS, INTERVAL_MS, BURST));
    assert(!reply_gate_allow(&gate, start + INTERVAL_MS, INTERVAL_MS, BURST));
}

static void host_harvest_cadence_with_bundle_is_never_denied(void) {
    reply_gate_t gate = {0};
    uint32_t now = 1000u;
    for (int i = 0; i < 2000; i++) {
        assert(reply_gate_allow(&gate, now, INTERVAL_MS, BURST));
        if (i % 100 == 50)
            assert(reply_gate_allow(&gate, now + 10u, INTERVAL_MS, BURST));
        now += 500u;
    }
}

static void flood_is_capped_to_burst_plus_rate(void) {
    reply_gate_t gate = {0};
    uint32_t allowed = 0;
    for (uint32_t ms = 0; ms < 10000u; ms++) {
        if (reply_gate_allow(&gate, 5000u + ms, INTERVAL_MS, BURST))
            allowed++;
    }
    assert(allowed <= BURST + 10000u / INTERVAL_MS);
    assert(allowed >= 10000u / INTERVAL_MS);
}

static void idle_gate_refills_to_full_burst(void) {
    reply_gate_t gate = {0};
    for (uint32_t i = 0; i < BURST; i++)
        assert(reply_gate_allow(&gate, 100u, INTERVAL_MS, BURST));
    assert(!reply_gate_allow(&gate, 100u, INTERVAL_MS, BURST));
    uint32_t later = 100u + 60000u;
    for (uint32_t i = 0; i < BURST; i++)
        assert(reply_gate_allow(&gate, later, INTERVAL_MS, BURST));
    assert(!reply_gate_allow(&gate, later, INTERVAL_MS, BURST));
}

static void zero_limits_deny_everything(void) {
    reply_gate_t gate = {0};
    assert(!reply_gate_allow(&gate, 0u, 0u, BURST));
    assert(!reply_gate_allow(&gate, 0u, INTERVAL_MS, 0u));
}

int main(void) {
    burst_is_allowed_then_denied(0u);
    burst_is_allowed_then_denied(UINT32_MAX - 100u);
    host_harvest_cadence_with_bundle_is_never_denied();
    flood_is_capped_to_burst_plus_rate();
    idle_gate_refills_to_full_burst();
    zero_limits_deny_everything();
    printf("reply_gate tests passed\n");
    return 0;
}
