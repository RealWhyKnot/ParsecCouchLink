// pico-bridge firmware entry point.
//
// One binary, two USB personas, mode decided at boot from flash:
//   setup mode (no creds): CDC ACM, accepts SET_WIFI, persists to flash,
//     then REBOOT_TO_RUN.
//   run mode (creds present): wired Xbox 360 (XUSB) over USB, Wi-Fi via
//     cyw43, UDP listener on 4242, forwards state from bridge.exe into
//     a shared gamepad struct that the XInput report builder consumes.

#include "pico/stdlib.h"
#include "pico/cyw43_arch.h"
#include "hardware/watchdog.h"
#include "tusb.h"

#include "boot_mode.h"
#include "cdc_handlers.h"
#include "diag_log.h"
#include "flash_creds.h"
#include "gamepad_state.h"
#include "net_udp.h"
#include "watchdog.h"
#include "wifi.h"
#include "xinput.h"

volatile gamepad_state_t g_gamepad_state = {0};
volatile uint32_t        g_last_packet_ms = 0;
volatile uint8_t         g_parsec_connected = 0;

static void run_mode_main_loop(void) {
    // Load creds (already verified by boot_mode_decide), bring up Wi-Fi,
    // bring up UDP, then poll forever.
    flash_creds_t creds;
    if (!flash_creds_load(&creds)) {
        diag_log_msg("run: lost creds between boot and run -- rebooting to setup");
        watchdog_reboot(0, 0, 100);
        for (;;) tight_loop_contents();
    }

    if (!wifi_init()) {
        diag_log_msg("run: cyw43 init failed; halting");
        for (;;) {
            tud_task();
            sleep_ms(100);
        }
    }
    wifi_start_join((const char*)creds.ssid, creds.ssid_len,
                    (const char*)creds.password, creds.pass_len);
    // Zero the local copy of the password now that cyw43 has it.
    for (size_t i = 0; i < sizeof(creds.password); i++)
        ((volatile uint8_t *)creds.password)[i] = 0;

    bool udp_inited = false;

    xinput_init();
    watchdog_init();
    diag_log_msg("run: main loop");

    for (;;) {
        tud_task();
        cyw43_arch_poll();
        wifi_task();

        if (wifi_state() == WIFI_STATE_JOINED && !udp_inited) {
            udp_inited = net_udp_init();
        }
        if (udp_inited) {
            net_udp_task();
        }

        xinput_task();
        watchdog_tick();

        // Briefly yield so cyw43_arch_poll doesn't get starved on a
        // tight loop. ~250 Hz is plenty for the application logic;
        // XInput's 1 ms USB cadence is driven by TinyUSB.
        sleep_us(500);
    }
}

static void setup_mode_main_loop(void) {
    cdc_handlers_init();
    diag_log_msg("setup: CDC ready, awaiting host");
    for (;;) {
        tud_task();
        cdc_handlers_poll();
        if (cdc_handlers_reboot_pending()) {
            diag_log_msg("setup: REBOOT_TO_RUN acknowledged, resetting");
            sleep_ms(50);
            watchdog_reboot(0, 0, 100);
            for (;;) tight_loop_contents();
        }
        sleep_us(500);
    }
}

int main(void) {
    stdio_init_all();
    diag_log_init();
    diag_log_msg("couchlink-pico boot");

    boot_mode_t mode = boot_mode_decide();
    tusb_init();
    if (mode == BOOT_MODE_RUN) {
        run_mode_main_loop();
    } else {
        setup_mode_main_loop();
    }
    return 0;
}
