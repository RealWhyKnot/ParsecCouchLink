#include "wifi.h"

#include <string.h>

#include "pico/cyw43_arch.h"
#include "pico/stdlib.h"
#include "lwip/dhcp.h"
#include "lwip/netif.h"

#include "diag_log.h"
#include "cdc_proto.h"  // for CDC_ERR_* codes (re-used for status reporting)

// Country code default. Override at build time with
// -DPICO_BRIDGE_WIFI_COUNTRY=CYW43_COUNTRY_<XX>. WORLDWIDE is the
// safest default for a globally-distributed pre-built UF2; EU/UK
// builders who need channel 12 or 13 should override.
#ifndef PICO_BRIDGE_WIFI_COUNTRY
#define PICO_BRIDGE_WIFI_COUNTRY CYW43_COUNTRY_WORLDWIDE
#endif

#define WIFI_COUNTRY_NAME_(c) #c
#define WIFI_COUNTRY_NAME(c)  WIFI_COUNTRY_NAME_(c)

// After this many consecutive join failures, do a full cyw43_arch
// deinit / init cycle before retrying. Community reports show the
// CYW43439 can wedge in a state that only a re-init clears (see e.g.
// pico-sdk issue #2153 for adjacent symptoms). At the current
// backoff cadence 10 retries is ~12 minutes -- enough to rule out
// transient AP outages without prolonging a real wedge.
#define WIFI_DEINIT_AFTER_FAILS 10

// DHCP renewal guard. Once the link is up and an IP is assigned, the
// lwIP DHCP client can silently lose the lease (3 failed renewals
// makes lwIP drop the netif). If `netif_default` reports no IP for
// this long, force a release+start cycle.
#define WIFI_DHCP_REGRAB_MS 30000u

static wifi_state_t state = WIFI_STATE_IDLE;
static int8_t  rssi = 0;
static uint8_t last_error = 0;

static char saved_ssid[33];
static char saved_pass[64];
static uint8_t saved_ssid_len = 0;
static uint8_t saved_pass_len = 0;

static absolute_time_t join_started;
static absolute_time_t next_retry;
static absolute_time_t no_ip_since;
static bool            no_ip_armed = false;
static uint32_t        retry_count = 0;
static uint32_t        consecutive_failures = 0;
static bool            link_logged_after_join = false;

static void apply_power_save(void) {
    // Default `CYW43_DEFAULT_PM = 0xA11142` (PM2 with 200 ms
    // sleep-return) has documented disassociation problems under
    // UniFi and enterprise APs at group-rekey time (pico-sdk issue
    // #2153). PERFORMANCE_PM keeps the same PM2 mode but with a
    // 20 ms sleep-return, which the community reports as the most
    // reliable. Cost is a few mA of idle current -- irrelevant on a
    // USB-powered bridge.
    int rc = cyw43_wifi_pm(&cyw43_state, CYW43_PERFORMANCE_PM);
    if (rc != 0) {
        diag_log_printf("wifi: cyw43_wifi_pm(PERFORMANCE) rc=%d", rc);
    }
}

bool wifi_init(void) {
    int rc = cyw43_arch_init_with_country(PICO_BRIDGE_WIFI_COUNTRY);
    if (rc != 0) {
        diag_log_printf("wifi: cyw43_arch_init_with_country(%s) rc=%d",
                        WIFI_COUNTRY_NAME(PICO_BRIDGE_WIFI_COUNTRY), rc);
        return false;
    }
    cyw43_arch_enable_sta_mode();
    apply_power_save();
    diag_log_printf("wifi: initialised (country=%s, pm=PERFORMANCE)",
                    WIFI_COUNTRY_NAME(PICO_BRIDGE_WIFI_COUNTRY));
    return true;
}

static bool wifi_reinit_after_wedge(void) {
    diag_log_printf("wifi: %u consecutive failures -- deinit/init cycle",
                    (unsigned)consecutive_failures);
    cyw43_arch_deinit();
    int rc = cyw43_arch_init_with_country(PICO_BRIDGE_WIFI_COUNTRY);
    if (rc != 0) {
        diag_log_printf("wifi: re-init returned rc=%d -- giving up for now", rc);
        return false;
    }
    cyw43_arch_enable_sta_mode();
    apply_power_save();
    diag_log_msg("wifi: re-init complete");
    consecutive_failures = 0;
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

    diag_log_printf("wifi: starting join to %s (ssid_len=%u pass_len=%u)",
                    saved_ssid, (unsigned)ssid_len, (unsigned)pass_len);
    state = WIFI_STATE_JOINING;
    last_error = 0;
    join_started = get_absolute_time();
    retry_count = 0;
    consecutive_failures = 0;
    link_logged_after_join = false;
    no_ip_armed = false;

    // Treat the wrapper return value as authoritative for hard
    // up-front failure (cyw43-driver issue #62: poll-after
    // link_status can lie about BADAUTH vs JOIN). We still use
    // cyw43_tcpip_link_status below for the eventual UP/NONET/
    // BADAUTH classification once join is in flight.
    int rc = cyw43_arch_wifi_connect_async(saved_ssid, saved_pass,
                                           CYW43_AUTH_WPA2_AES_PSK);
    if (rc != 0) {
        diag_log_printf("wifi: initial connect_async returned rc=%d", rc);
        state = WIFI_STATE_FAILED;
        last_error = CDC_ERR_INTERNAL;
        consecutive_failures++;
        next_retry = make_timeout_time_ms(5000);
    }
}

void wifi_task(void) {
    if (state == WIFI_STATE_IDLE) return;

    int link = cyw43_tcpip_link_status(&cyw43_state, CYW43_ITF_STA);

    // Log the first link sample after a join is in flight so a
    // "the radio came up at all" question is answerable from logs.
    if (!link_logged_after_join && state == WIFI_STATE_JOINING) {
        diag_log_printf("wifi: first link_status sample = %d", link);
        link_logged_after_join = true;
    }

    if (state == WIFI_STATE_JOINED) {
        // Detect AP-side disconnects so a router reboot doesn't wedge
        // the Pico in a stale JOINED state forever.
        if (link != CYW43_LINK_UP) {
            diag_log_printf("wifi: link dropped (status=%d); reconnecting", link);
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_INTERNAL;
            next_retry = make_timeout_time_ms(2000);
            no_ip_armed = false;
            return;
        }
        // DHCP-renewal guard: lwIP can silently drop the netif after
        // three failed renewals, which surfaces as udp_sendto rc=ERR_RTE.
        struct netif *nif = netif_default;
        bool ip_present = nif && !ip_addr_isany(&nif->ip_addr);
        if (!ip_present) {
            if (!no_ip_armed) {
                no_ip_since = get_absolute_time();
                no_ip_armed = true;
                diag_log_msg("wifi: link up but no IP -- watching for DHCP regrab");
            } else if (absolute_time_diff_us(no_ip_since, get_absolute_time())
                       > (int64_t)WIFI_DHCP_REGRAB_MS * 1000) {
                diag_log_msg("wifi: no IP for 30 s -- restarting DHCP");
                if (nif) {
                    dhcp_release_and_stop(nif);
                    dhcp_start(nif);
                }
                no_ip_armed = false;
            }
        } else {
            no_ip_armed = false;
        }
        return;
    }

    if (state == WIFI_STATE_JOINING) {
        if (link == CYW43_LINK_UP) {
            state = WIFI_STATE_JOINED;
            consecutive_failures = 0;
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
            consecutive_failures++;
            diag_log_printf("wifi: SSID not found (fail #%u)",
                            (unsigned)consecutive_failures);
            return;
        }
        if (link == CYW43_LINK_BADAUTH) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_AUTH_FAIL;
            next_retry = make_timeout_time_ms(60000);
            consecutive_failures++;
            diag_log_printf("wifi: auth rejected (fail #%u); will retry in 60 s",
                            (unsigned)consecutive_failures);
            return;
        }
        if (link == CYW43_LINK_FAIL) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_INTERNAL;
            next_retry = make_timeout_time_ms(15000);
            consecutive_failures++;
            diag_log_printf("wifi: generic join fail (fail #%u)",
                            (unsigned)consecutive_failures);
            return;
        }
        // Otherwise still negotiating; check timeout.
        if (absolute_time_diff_us(join_started, get_absolute_time()) > 30 * 1000 * 1000) {
            state = WIFI_STATE_FAILED;
            last_error = CDC_ERR_WIFI_JOIN_TIMEOUT;
            next_retry = make_timeout_time_ms(15000);
            consecutive_failures++;
            diag_log_printf("wifi: join timed out after 30 s (fail #%u)",
                            (unsigned)consecutive_failures);
        }
    } else if (state == WIFI_STATE_FAILED) {
        if (saved_ssid_len > 0 && absolute_time_diff_us(get_absolute_time(), next_retry) <= 0) {
            // If we have been failing for a long time, force a full
            // re-init before retrying. Note: cyw43_arch_wifi_connect_async
            // is documented (pico-sdk issue #1054) as being invoked
            // exactly once internally per call -- the retry loop here
            // is the workaround, not a defensive belt-and-braces.
            if (consecutive_failures >= WIFI_DEINIT_AFTER_FAILS) {
                if (!wifi_reinit_after_wedge()) {
                    next_retry = make_timeout_time_ms(60000);
                    return;
                }
            }

            retry_count++;
            diag_log_printf("wifi: retry #%u (consecutive_failures=%u)",
                            (unsigned)retry_count,
                            (unsigned)consecutive_failures);
            join_started = get_absolute_time();
            link_logged_after_join = false;
            state = WIFI_STATE_JOINING;
            int rc = cyw43_arch_wifi_connect_async(saved_ssid, saved_pass,
                                                   CYW43_AUTH_WPA2_AES_PSK);
            if (rc != 0) {
                state = WIFI_STATE_FAILED;
                last_error = CDC_ERR_INTERNAL;
                next_retry = make_timeout_time_ms(15000);
                consecutive_failures++;
                diag_log_printf("wifi: retry #%u connect_async rc=%d (fail #%u)",
                                (unsigned)retry_count, rc,
                                (unsigned)consecutive_failures);
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
