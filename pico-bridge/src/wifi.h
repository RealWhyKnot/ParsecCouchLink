#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef enum {
    WIFI_STATE_IDLE,
    WIFI_STATE_JOINING,
    WIFI_STATE_JOINED,
    WIFI_STATE_FAILED,
} wifi_state_t;

bool wifi_init(void);

void wifi_start_join(const char *ssid, uint8_t ssid_len, const char *password, uint8_t pass_len);

void wifi_task(void);

wifi_state_t wifi_state(void);
int8_t wifi_rssi(void);
uint32_t wifi_ip(void);
uint8_t wifi_last_error_code(void);

int wifi_last_init_rc(void);

uint32_t wifi_radio_generation(void);
