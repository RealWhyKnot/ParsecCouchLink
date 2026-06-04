#pragma once

#include <stdbool.h>
#include <stdint.h>

typedef enum {
    WIFI_STATE_IDLE,
    WIFI_STATE_JOINING,
    WIFI_STATE_JOINED,
    WIFI_STATE_FAILED,
} wifi_state_t;

// Init cyw43_arch. Safe to call once.
bool wifi_init(void);

// Begin a non-blocking join attempt with the given credentials.
// `ssid` and `password` are not retained; copies are made.
void wifi_start_join(const char *ssid, uint8_t ssid_len, const char *password, uint8_t pass_len);

// Poll once. Drives the state machine and triggers retries on failure.
void wifi_task(void);

wifi_state_t wifi_state(void);
int8_t wifi_rssi(void);
uint32_t wifi_ip(void);
uint8_t wifi_last_error_code(void);

// rc returned by the most recent cyw43_arch_init_with_country() call
// (0 = success). Used by run mode to record *why* the radio never came
// up when it bounces to setup for diagnosis.
int wifi_last_init_rc(void);
