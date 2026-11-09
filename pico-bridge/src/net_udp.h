#pragma once

#include <stdbool.h>
#include <stdint.h>

bool net_udp_init(void);
void net_udp_set_recovery_boot(bool physical_boot);
void net_udp_task(void);

bool net_udp_has_peer(void);
uint32_t net_udp_tx_count(void);
uint32_t net_udp_rx_count(void);
