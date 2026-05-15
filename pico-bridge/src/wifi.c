#include "wifi.h"

#include <string.h>

#include "pico/cyw43_arch.h"
#include "pico/stdlib.h"

#include "diag_log.h"
#include "cdc_proto.h"  // for CDC_ERR_* codes (re-used for status reporting)

static wifi_state_t state = WIFI_STATE_IDLE;
static int8_t  rssi = 0;
static uint8_t last_error = 0;

static char saved_ssid[33];
static char saved_pass[64];
static uint8_t saved_ssid_len = 0;
static uint8_t saved_pass_len = 0;

static absolute_time_t join_started;
static absolute_time_t next_retry;
static uint32_t retry_count = 0;

bool wifi_init(void) {
    if (cyw43_arch_init() != 0) {
        diag_log_msg("wifi: cyw43_arch_init failed");
        return false;
    }
    cyw43_arch_enable_sta_mode();
    diag_log_msg("wifi: cyw43 initialised in station mode");
    return true;
}

void wifi_start_join(const char *ssid, uint8_t ssid_len,
                     const char *password, uint8_t pass_len) {
    if (ssid_len == 0 || ssid_len > 32) {
        diag_log_printf("wifi: rejecting ssid_len=%u", (unsigned)ssid_len);
        state = WIFI_STATE_FAILED;
        last_error = CDC_ERR_BAD_LENGTH;
        return;
    }
    memcpy(saved_ssid, ssid, ssid_len);
    saved_ssid[ssid_len] = 0;
    saved_ssid_len = ssid_len;
    if (pass_len > 63) pass_len = 63;
    memcpy(saved_pass, password, pass_len);
    saved_pass[pass_len] = 0;
    saved_pass_len = pass_len;

    diag_log_printf("wifi: starting join to %s (ssid_len=%u)",
                    saved_ssid, (unsigned)ssid_len);
    state = WIFI_STATE_JOINING;
    last_error = 0;
    join_started = get_absolute_time();
    retry_count = 0;

    int rc = cyw43_arch_wifi_connect_async(saved_ssid, saved_pass,
                                           CYW43_AUTH_WPA2_AES_PSK);
    if (rc != 0) {
        diag_log_printf("wifi: connect_async returned %d", rc);
        state = WIFI_STATE_FAILED;
        last_error = CDC_ERR_INTERNAL;
        // Without a retry timer, the FAILED branch in wifi_task would
        // busy-loop the connect call. Defer the next attempt.
        next_retry = make_timeout_time_ms(5000);
    }
}

void wifi_task(void) {
    if (state == WIFI_STATE_IDLE) return;

    int link = cyw43_tcpip_link_status(&cyw43_state, CYW43_ITF_STA);

    if (state == WIFI_STATE_JOINED) {
        // Detect AP-side disconnects so a router reboot doesn't wedge
        // the Pico in a stale JOINED state forever.
        if (link != CYW43_LINK_UP) {
            diag_log_printf("wifi: link dropped (status=%d); reconnecting", link);
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_INTERNAL;
            next_retry = make_timeout_time_ms(2000);
        }
        return;
    }

    if (state == WIFI_STATE_JOINING) {
        if (link == CYW43_LINK_UP) {
            state = WIFI_STATE_JOINED;
            diag_log_printf("wifi: joined; ip=%u.%u.%u.%u",
                            (cyw43_state.netif[0].ip_addr.addr >> 0) & 0xFF,
                            (cyw43_state.netif[0].ip_addr.addr >> 8) & 0xFF,
                            (cyw43_state.netif[0].ip_addr.addr >> 16) & 0xFF,
                            (cyw43_state.netif[0].ip_addr.addr >> 24) & 0xFF);
            return;
        }
        if (link == CYW43_LINK_NONET) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_NO_2G_NETWORK;
            next_retry = make_timeout_time_ms(15000);
            diag_log_msg("wifi: SSID not found");
            return;
        }
        if (link == CYW43_LINK_BADAUTH) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_AUTH_FAIL;
            next_retry = make_timeout_time_ms(60000);
            diag_log_msg("wifi: auth rejected; will retry in 60 s");
            return;
        }
        if (link == CYW43_LINK_FAIL) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_INTERNAL;
            next_retry = make_timeout_time_ms(15000);
            diag_log_msg("wifi: generic join fail");
            return;
        }
        // Otherwise still negotiating; check timeout.
        if (absolute_time_diff_us(join_started, get_absolute_time()) > 30 * 1000 * 1000) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_WIFI_JOIN_TIMEOUT;
            next_retry = make_timeout_time_ms(15000);
            diag_log_msg("wifi: join timed out after 30 s");
        }
    } else if (state == WIFI_STATE_FAILED) {
        if (saved_ssid_len > 0 && absolute_time_diff_us(get_absolute_time(), next_retry) <= 0) {
            retry_count++;
            diag_log_printf("wifi: retry #%u", (unsigned)retry_count);
            join_started = get_absolute_time();
            state = WIFI_STATE_JOINING;
            int rc = cyw43_arch_wifi_connect_async(saved_ssid, saved_pass,
                                                   CYW43_AUTH_WPA2_AES_PSK);
            if (rc != 0) {
                state = WIFI_STATE_FAILED;
                last_error = CDC_ERR_INTERNAL;
                next_retry = make_timeout_time_ms(15000);
            }
        }
    }
}

wifi_state_t wifi_state(void)         { return state; }
int8_t       wifi_rssi(void)          { return rssi; }
uint8_t      wifi_last_error_code(void) { return last_error; }
uint32_t     wifi_ip(void) {
    if (state != WIFI_STATE_JOINED) return 0;
    return cyw43_state.netif[0].ip_addr.addr;
}
