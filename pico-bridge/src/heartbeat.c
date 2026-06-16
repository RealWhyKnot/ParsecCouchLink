#include "heartbeat.h"

#include <stdint.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "diag_log.h"
#include "gamepad_state.h"
#include "net_udp.h"
#include "wifi.h"

// 5 seconds keeps the ring populated without burning bytes on a quiet
// system. 16 KiB diag_log ring / ~96 bytes per heartbeat = ~170 entries
// or ~3.5 minutes of history even if nothing else ever logs.
#define HEARTBEAT_INTERVAL_MS 5000

static absolute_time_t next_beat;
static uint32_t beat_count = 0;

// One-shot assoc_result line. Emitted on the first heartbeat after the
// Wi-Fi state settles to a definitive outcome. Reset to "joining" when
// a new join begins so back-to-back attempts each get their own line.
static wifi_state_t assoc_last_observed = WIFI_STATE_IDLE;
static bool assoc_result_logged = false;

static const char *wifi_state_name(wifi_state_t s) {
    switch (s) {
    case WIFI_STATE_IDLE:
        return "idle";
    case WIFI_STATE_JOINING:
        return "joining";
    case WIFI_STATE_JOINED:
        return "joined";
    case WIFI_STATE_FAILED:
        return "failed";
    default:
        return "?";
    }
}

void heartbeat_init(void) {
    next_beat = make_timeout_time_ms(HEARTBEAT_INTERVAL_MS);
    beat_count = 0;
    assoc_last_observed = WIFI_STATE_IDLE;
    assoc_result_logged = false;
}

static bool due(void) {
    if (absolute_time_diff_us(get_absolute_time(), next_beat) > 0)
        return false;
    next_beat = make_timeout_time_ms(HEARTBEAT_INTERVAL_MS);
    beat_count++;
    return true;
}

void heartbeat_run_mode_task(void) {
    wifi_state_t ws = wifi_state();

    // One-shot assoc_result line on the first heartbeat after the join
    // attempt settles. Emitted independently of the normal heartbeat
    // cadence so a diag pull that only gets one or two heartbeats still
    // surfaces the association outcome.
    if (!assoc_result_logged) {
        // Transition from JOINING to a settled state.
        bool was_joining =
            (assoc_last_observed == WIFI_STATE_JOINING || assoc_last_observed == WIFI_STATE_IDLE);
        bool settled = (ws == WIFI_STATE_JOINED || ws == WIFI_STATE_FAILED);
        if (was_joining && settled) {
            diag_log_printf("hb#%u assoc_result=%s last_error=%u", (unsigned)beat_count,
                            wifi_state_name(ws), (unsigned)wifi_last_error_code());
            assoc_result_logged = true;
        }
        // Reset for the next join attempt when we go back to JOINING.
        if (ws == WIFI_STATE_JOINING && assoc_last_observed != WIFI_STATE_JOINING) {
            assoc_result_logged = false;
        }
    }
    assoc_last_observed = ws;

    if (!due())
        return;

    uint32_t uptime_s = (uint32_t)(to_ms_since_boot(get_absolute_time()) / 1000);
    bool usb_up = tud_mounted();
    bool usb_susp = tud_suspended();
    uint32_t ip = wifi_ip();
    int8_t rssi_val = wifi_rssi();
    bool have_peer = net_udp_has_peer();
    uint32_t now_ms = to_ms_since_boot(get_absolute_time());
    uint32_t since_peer = now_ms - g_last_packet_ms;

    diag_log_printf("hb#%u t=%us mode=run usb=%s%s wifi=%s ip=%u.%u.%u.%u rssi=%d peer=%s "
                    "since_peer_ms=%u pcon=%d tx_pkts=%u rx_pkts=%u",
                    (unsigned)beat_count, (unsigned)uptime_s, usb_up ? "mounted" : "unmounted",
                    usb_susp ? "/suspended" : "", wifi_state_name(ws), (unsigned)((ip >> 0) & 0xFF),
                    (unsigned)((ip >> 8) & 0xFF), (unsigned)((ip >> 16) & 0xFF),
                    (unsigned)((ip >> 24) & 0xFF), (int)rssi_val, have_peer ? "latched" : "none",
                    (unsigned)since_peer, (int)g_parsec_connected, (unsigned)net_udp_tx_count(),
                    (unsigned)net_udp_rx_count());
}

void heartbeat_setup_mode_task(void) {
    if (!due())
        return;

    uint32_t uptime_s = (uint32_t)(to_ms_since_boot(get_absolute_time()) / 1000);
    bool usb_up = tud_mounted();
    bool usb_susp = tud_suspended();
    bool dtr = tud_cdc_connected();

    diag_log_printf("hb#%u t=%us mode=setup usb=%s%s cdc_dtr=%d", (unsigned)beat_count,
                    (unsigned)uptime_s, usb_up ? "mounted" : "unmounted",
                    usb_susp ? "/suspended" : "", (int)dtr);
}
