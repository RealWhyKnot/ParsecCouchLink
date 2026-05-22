#pragma once

#include <stdbool.h>
#include <stdint.h>

// lwIP UDP server for the runtime wire protocol (wiki/Protocol.md):
//   recv: DISCOVER, STATE, HEARTBEAT  (port 4242)
//   send: ACK (responding to DISCOVER) + 1 Hz keepalive HEARTBEAT once
//         bound to a peer.

bool net_udp_init(void);
void net_udp_task(void);

// Read-only accessors used by the firmware heartbeat. Counters wrap on
// uint32_t overflow (~49 days at 250 pkt/s); the heartbeat reports the
// instantaneous value, so wrap is harmless for diagnostic purposes.
bool     net_udp_has_peer(void);
uint32_t net_udp_tx_count(void);
uint32_t net_udp_rx_count(void);
