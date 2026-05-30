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
#include "pico/bootrom.h"
#include "hardware/watchdog.h"
#include "tusb.h"

#include "boot_mode.h"
#include "cdc_handlers.h"
#include "cdc_proto.h"
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
    diag_log_printf("boot: couchlink-pico fw=%s board=0x%02X unique-id=%s",
                    PICO_BRIDGE_FW_VERSION_STRING, PICO_BRIDGE_BOARD_TYPE, id);
}

// Inline state-name helper for the assoc watchdog log line. The
// canonical wifi_state_name() is static in heartbeat.c; duplicating
// the four-case switch here avoids an API surface change for a single
// call site.
static const char *wifi_state_str(wifi_state_t s) {
    switch (s) {
        case WIFI_STATE_IDLE:    return "idle";
        case WIFI_STATE_JOINING: return "joining";
        case WIFI_STATE_JOINED:  return "joined";
        case WIFI_STATE_FAILED:  return "failed";
        default:                 return "?";
    }
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

    // Capture entry time for the association watchdog.
    absolute_time_t run_entry = get_absolute_time();
    bool assoc_watchdog_armed = true;

    xinput_init();
    watchdog_init();
    heartbeat_init();
    diag_log_msg("run: main loop");
    reset_reason_mark_main_loop_entered();

    for (;;) {
        tud_task();
        boot_mode_post_enum_bootsel_poll();
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

        // Wi-Fi association watchdog. Fires once and triggers a bounce
        // to setup mode so the user can re-provision if the credentials
        // are wrong or the AP is unreachable at this location.
        if (assoc_watchdog_armed && wifi_state() != WIFI_STATE_JOINED) {
            wifi_state_t ws = wifi_state();
            uint8_t err = wifi_last_error_code();

            // Immediate bounce on definitive auth / SSID failures --
            // no point waiting 30 s when the answer is already known.
            if (ws == WIFI_STATE_FAILED
                && (err == CDC_ERR_AUTH_FAIL || err == CDC_ERR_NO_2G_NETWORK)) {
                const char *ename = (err == CDC_ERR_AUTH_FAIL) ? "BADAUTH" : "NONET";
                diag_log_printf("wifi: assoc_result=%s -- bouncing immediately to setup mode",
                                ename);
                assoc_watchdog_armed = false;
                reset_reason_request_setup_after_reboot();
                watchdog_reboot(0, 0, 100);
                for (;;) tight_loop_contents();
            }

            // 30-second timeout watchdog for all other failure modes
            // (JOINING timeout, generic FAILED, idle).
            int64_t elapsed_us = absolute_time_diff_us(run_entry,
                                                       get_absolute_time());
            if (elapsed_us >= 30000000) {
                uint32_t uptime_s = (uint32_t)(
                    to_ms_since_boot(get_absolute_time()) / 1000);
                diag_log_printf(
                    "wifi: assoc watchdog firing -- mode=run uptime=%us "
                    "state=%s last_error=%u -- bouncing to setup mode, creds retained",
                    (unsigned)uptime_s, wifi_state_str(ws), (unsigned)err);
                assoc_watchdog_armed = false;
                reset_reason_request_setup_after_reboot();
                watchdog_reboot(0, 0, 100);
                for (;;) tight_loop_contents();
            }
        }

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
        boot_mode_post_enum_bootsel_poll();
        cdc_handlers_poll();
        heartbeat_setup_mode_task();
        if (cdc_handlers_bootsel_pending()) {
            diag_log_msg("setup: REBOOT_TO_BOOTSEL acknowledged, resetting to BOOTSEL");
            sleep_ms(50);
            reset_usb_boot(0, 0);
        }
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

    // Decide mode BEFORE tusb_init() so D+ is never asserted while the
    // mode is still undecided. boot_mode_decide() is non-blocking: it
    // samples BOOTSEL once at t=0 and reads flash credentials. The
    // post-enum poll (called from each main loop) handles the 3-second
    // wipe escalation after USB is up.
    boot_mode_t mode = boot_mode_decide(&rr);
    diag_log_printf("boot: mode decided: %s persona will be advertised",
                    mode == BOOT_MODE_RUN ? "XInput" : "CDC+diag");

    // Now that the correct persona is fixed, raise D+ once and keep it
    // there. The host sees a single clean connect event.
    tusb_init();
    diag_log_msg("usb_init: tusb_init complete");

    // Pump TinyUSB until the host completes enumeration or 1500 ms,
    // whichever comes first. Run mode's first blocking step is
    // wifi_init()/cyw43_arch_init_with_country(), which can hold the
    // CPU for hundreds of ms to a couple of seconds while it streams
    // the CYW43 firmware blob to the radio. Without this pump, Windows
    // sends GET_DESCRIPTOR(DEVICE) into a stack with no task ticking,
    // times out, and abandons the device as VID_0000:PID_0002. Setup
    // mode mounts in well under 100 ms so the pump exits early and
    // costs us nothing on the setup path.
    {
        absolute_time_t pump_start = get_absolute_time();
        absolute_time_t deadline   = make_timeout_time_ms(1500);
        bool mounted_in_pump = false;
        while (!time_reached(deadline)) {
            tud_task();
            if (tud_mounted()) { mounted_in_pump = true; break; }
            sleep_us(500);
        }
        uint32_t elapsed_ms = (uint32_t)(
            absolute_time_diff_us(pump_start, get_absolute_time()) / 1000);
        if (mounted_in_pump) {
            diag_log_printf(
                "usb_init: enumeration completed during pump (%u ms)",
                (unsigned)elapsed_ms);
        } else {
            diag_log_printf(
                "usb_init: pump timeout after %u ms (mounted=%d) -- "
                "continuing with mode init",
                (unsigned)elapsed_ms, (int)tud_mounted());
        }
    }

    if (mode == BOOT_MODE_RUN) {
        run_mode_main_loop();
    } else {
        setup_mode_main_loop();
    }
    return 0;
}
