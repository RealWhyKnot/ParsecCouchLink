#pragma once

#include <stdbool.h>

// lwIP UDP server for the runtime wire protocol (wiki/Protocol.md):
//   recv: DISCOVER, STATE, HEARTBEAT  (port 4242)
//   send: ACK (responding to DISCOVER) + 1 Hz keepalive HEARTBEAT once
//         bound to a peer.

bool net_udp_init(void);
void net_udp_task(void);
