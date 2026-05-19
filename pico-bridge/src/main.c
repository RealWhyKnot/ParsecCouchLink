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
#include "pico/unique_id.h"
#include "hardware/watchdog.h"
#include "tusb.h"

#include "boot_mode.h"
#include "cdc_handlers.h"
#include "diag_log.h"
#include "flash_creds.h"
#include "gamepad_state.h"
#include "heartbeat.h"
#include "net_udp.h"
#include "reset_reason.h"
#include "version.h"
#include "watchdog.h"
#include "wifi.h"
#include "xinput.h"

volatile gamepad_state_t g_gamepad_state = {0};
volatile uint32_t        g_last_packet_ms = 0;
volatile uint8_t         g_parsec_connected = 0;

static void log_reset_reason(const reset_reason_info_t *info) {
    diag_log_printf("boot: reset-reason=%s", reset_reason_name(info->reason));
    if (info->reason == RESET_REASON_FAULT) {
        diag_log_printf("boot: prior fault pc=0x%08X lr=0x%08X xpsr=0x%08X",
                        (unsigned)info->fault_pc,
                        (unsigned)info->fault_lr,
                        (unsigned)info->fault_xpsr);
        if (info->full_frame_valid) {
            diag_log_printf("boot: prior fault r0=0x%08X r1=0x%08X r2=0x%08X r3=0x%08X",
                            (unsigned)info->fault_r0,
                            (unsigned)info->fault_r1,
                            (unsigned)info->fault_r2,
                            (unsigned)info->fault_r3);
            diag_log_printf("boot: prior fault r12=0x%08X sp_at_fault=0x%08X",
                            (unsigned)info->fault_r12,
                            (unsigned)info->fault_sp);
        }
        if (info->fault_status_valid) {
            // CFSR splits as MMFSR (byte 0), BFSR (byte 1), UFSR (bytes 2-3)
            // -- the breakdown is the single most useful classifier of
            // the fault cause on Cortex-M33. Dump the raw value and the
            // common-case bit decode.
            uint32_t cfsr = info->fault_cfsr;
            diag_log_printf("boot: prior fault cfsr=0x%08X hfsr=0x%08X mmfar=0x%08X bfar=0x%08X",
                            (unsigned)cfsr,
                            (unsigned)info->fault_hfsr,
                            (unsigned)info->fault_mmfar,
                            (unsigned)info->fault_bfar);
            // MMFSR bits in CFSR[7:0]:
            //   bit 0 IACCVIOL    instruction access violation
            //   bit 1 DACCVIOL    data access violation
            //   bit 3 MUNSTKERR   error returning from exception
            //   bit 4 MSTKERR     error stacking for exception entry
            //   bit 7 MMARVALID   MMFAR is valid
            // BFSR bits in CFSR[15:8]:
            //   bit 8 IBUSERR     bus error on instruction prefetch
            //   bit 9 PRECISERR   precise data bus error (BFAR valid)
            //   bit 10 IMPRECISERR imprecise data bus error
            //   bit 11 UNSTKERR   unstacking error
            //   bit 12 STKERR     stacking error
            //   bit 15 BFARVALID  BFAR is valid
            // UFSR bits in CFSR[31:16]:
            //   bit 16 UNDEFINSTR undefined instruction
            //   bit 17 INVSTATE   invalid EPSR state (e.g. bad branch)
            //   bit 18 INVPC      bad EXC_RETURN
            //   bit 19 NOCP       coprocessor access (FPU off, MVE off)
            //   bit 24 UNALIGNED  unaligned access
            //   bit 25 DIVBYZERO  integer divide by zero
            const char *cause = "unspecified";
            if (cfsr & (1u << 25)) cause = "divide-by-zero";
            else if (cfsr & (1u << 24)) cause = "unaligned-access";
            else if (cfsr & (1u << 19)) cause = "coprocessor-access";
            else if (cfsr & (1u << 18)) cause = "invalid-EXC_RETURN";
            else if (cfsr & (1u << 17)) cause = "invalid-EPSR-state";
            else if (cfsr & (1u << 16)) cause = "undefined-instruction";
            else if (cfsr & (1u << 12)) cause = "stacking-error";
            else if (cfsr & (1u << 11)) cause = "unstacking-error";
            else if (cfsr & (1u << 10)) cause = "imprecise-bus-error";
            else if (cfsr & (1u <<  9)) cause = "precise-bus-error";
            else if (cfsr & (1u <<  8)) cause = "instruction-prefetch-bus-error";
            else if (cfsr & (1u <<  4)) cause = "exception-entry-stack-error";
            else if (cfsr & (1u <<  3)) cause = "exception-return-unstack-error";
            else if (cfsr & (1u <<  1)) cause = "data-access-violation";
            else if (cfsr & (1u <<  0)) cause = "instruction-access-violation";
            diag_log_printf("boot: prior fault cause=%s", cause);
        }
    }
    if (info->last_line_valid && info->last_line[0] != 0) {
        diag_log_printf("boot: last live line before reset: %s",
                        info->last_line);
    }
}

static void log_board_identity(void) {
    char id[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES + 1];
    pico_get_unique_board_id_string(id, sizeof(id));
    diag_log_printf("boot: couchlink-pico fw=%d.%d.%d board=0x%02X unique-id=%s",
                    PICO_BRIDGE_FW_MAJOR, PICO_BRIDGE_FW_MINOR,
                    PICO_BRIDGE_FW_PATCH, PICO_BRIDGE_BOARD_TYPE, id);
}

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
    heartbeat_init();
    diag_log_msg("run: main loop");
    reset_reason_mark_main_loop_entered();

    for (;;) {
        tud_task();
        // Defense-in-depth: drain any CDC bytes the host may have sent
        // before we re-enumerated as XInput. With the boot-ordering fix
        // in main(), run mode never exposes CDC endpoints and this is a
        // no-op (early-exits on tud_cdc_available() == 0). It's here so
        // a future regression that reintroduces the persona race can't
        // silently fill the CDC RX FIFO again.
        cdc_handlers_poll();
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
        heartbeat_run_mode_task();

        // Briefly yield so cyw43_arch_poll doesn't get starved on a
        // tight loop. ~250 Hz is plenty for the application logic;
        // XInput's 1 ms USB cadence is driven by TinyUSB.
        sleep_us(500);
    }
}

static void setup_mode_main_loop(void) {
    cdc_handlers_init();
    heartbeat_init();
    diag_log_msg("setup: CDC ready, awaiting host");
    reset_reason_mark_main_loop_entered();
    for (;;) {
        tud_task();
        cdc_handlers_poll();
        heartbeat_setup_mode_task();
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

    // Classify the reset cause before anything else touches scratch
    // or runs code that might fault. The output goes into diag_log
    // first so the log buffer's very first line names the prior boot
    // outcome -- bundles after a hang will self-narrate.
    reset_reason_info_t rr = reset_reason_classify();
    log_reset_reason(&rr);
    log_board_identity();

    // tusb_init() runs first so TinyUSB's IRQ vectors, FIFOs, and
    // descriptor callbacks are alive before any blocking work. But we
    // immediately drop the D+ pull-up with tud_disconnect() so the
    // host never sees a connect event while boot_mode_decide() is
    // still running: until the mode flag is final, the descriptor
    // callbacks would return whichever persona defaults to value 0,
    // and the host would latch onto that. Holding D+ low across the
    // BOOTSEL recovery wait costs us nothing (the host has no device
    // to enumerate yet), and tud_connect() afterwards triggers a
    // single clean enumeration with the correct persona descriptors.
    tusb_init();
    tud_disconnect();
    diag_log_msg("boot: tusb_init done; D+ held low until mode decided");

    boot_mode_t mode = boot_mode_decide();
    tud_connect();
    diag_log_printf("boot: D+ raised for %s persona; entering %s main loop",
                    mode == BOOT_MODE_RUN ? "XInput" : "CDC+diag",
                    mode == BOOT_MODE_RUN ? "run" : "setup");

    if (mode == BOOT_MODE_RUN) {
        run_mode_main_loop();
    } else {
        setup_mode_main_loop();
    }
    return 0;
}
