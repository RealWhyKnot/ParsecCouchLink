// pico-bridge firmware entry point.
//
// One binary, setup mode plus runtime output personas, decided at boot from flash:
//   setup mode (no creds): CDC ACM, accepts SET_WIFI, persists to flash,
//     then REBOOT_TO_RUN.
//   run mode (creds present): Wi-Fi via cyw43, UDP listener on 4242,
//     forwards state from bridge.exe into the selected output persona.

#include <stdio.h>

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
#include "dinput.h"
#include "flash_creds.h"
#include "gamepad_state.h"
#include "heartbeat.h"
#include "hid_kbd.h"
#include "keyboard_state.h"
#include "n64_backend.h"
#include "net_udp.h"
#include "reset_reason.h"
#include "usb_diag.h"
#include "usb_packet_debug.h"
#include "version.h"
#include "watchdog.h"
#include "wifi.h"
#include "xbone.h"
#include "xinput.h"

volatile gamepad_state_t g_gamepad_state = {0};
volatile keyboard_state_t g_keyboard_state = {0};
volatile uint32_t g_last_packet_ms = 0;
volatile uint8_t g_parsec_connected = 0;

static void log_reset_reason(const reset_reason_info_t *info) {
    diag_log_printf("boot: reset-reason=%s", reset_reason_name(info->reason));
    if (info->reason == RESET_REASON_FAULT) {
        diag_log_printf("boot: prior fault pc=0x%08X lr=0x%08X xpsr=0x%08X",
                        (unsigned)info->fault_pc, (unsigned)info->fault_lr,
                        (unsigned)info->fault_xpsr);
        if (info->full_frame_valid) {
            diag_log_printf("boot: prior fault r0=0x%08X r1=0x%08X r2=0x%08X r3=0x%08X",
                            (unsigned)info->fault_r0, (unsigned)info->fault_r1,
                            (unsigned)info->fault_r2, (unsigned)info->fault_r3);
            diag_log_printf("boot: prior fault r12=0x%08X sp_at_fault=0x%08X",
                            (unsigned)info->fault_r12, (unsigned)info->fault_sp);
        }
        if (info->fault_status_valid) {
            // CFSR splits as MMFSR (byte 0), BFSR (byte 1), UFSR (bytes 2-3)
            // -- the breakdown is the single most useful classifier of
            // the fault cause on Cortex-M33. Dump the raw value and the
            // common-case bit decode.
            uint32_t cfsr = info->fault_cfsr;
            diag_log_printf("boot: prior fault cfsr=0x%08X hfsr=0x%08X mmfar=0x%08X bfar=0x%08X",
                            (unsigned)cfsr, (unsigned)info->fault_hfsr, (unsigned)info->fault_mmfar,
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
            if (cfsr & (1u << 25))
                cause = "divide-by-zero";
            else if (cfsr & (1u << 24))
                cause = "unaligned-access";
            else if (cfsr & (1u << 19))
                cause = "coprocessor-access";
            else if (cfsr & (1u << 18))
                cause = "invalid-EXC_RETURN";
            else if (cfsr & (1u << 17))
                cause = "invalid-EPSR-state";
            else if (cfsr & (1u << 16))
                cause = "undefined-instruction";
            else if (cfsr & (1u << 12))
                cause = "stacking-error";
            else if (cfsr & (1u << 11))
                cause = "unstacking-error";
            else if (cfsr & (1u << 10))
                cause = "imprecise-bus-error";
            else if (cfsr & (1u << 9))
                cause = "precise-bus-error";
            else if (cfsr & (1u << 8))
                cause = "instruction-prefetch-bus-error";
            else if (cfsr & (1u << 4))
                cause = "exception-entry-stack-error";
            else if (cfsr & (1u << 3))
                cause = "exception-return-unstack-error";
            else if (cfsr & (1u << 1))
                cause = "data-access-violation";
            else if (cfsr & (1u << 0))
                cause = "instruction-access-violation";
            diag_log_printf("boot: prior fault cause=%s", cause);
        }
    }
    if (info->last_line_valid && info->last_line[0] != 0) {
        diag_log_printf("boot: last live line before reset: %s", info->last_line);
    }
}

static void log_board_identity(void) {
    char id[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES + 1];
    pico_get_unique_board_id_string(id, sizeof(id));
    diag_log_printf("boot: couchlink-pico fw=%s board=0x%02X unique-id=%s",
                    PICO_BRIDGE_FW_VERSION_STRING, PICO_BRIDGE_BOARD_TYPE, id);
}

// Hand the credentials to cyw43 and start a non-blocking join, then wipe
// the local plaintext password copy (cyw43 has its own copy now).
static void run_issue_join(flash_creds_t *creds) {
    wifi_start_join((const char *)creds->ssid, creds->ssid_len, (const char *)creds->password,
                    creds->pass_len);
    for (size_t i = 0; i < sizeof(creds->password); i++)
        ((volatile uint8_t *)creds->password)[i] = 0;
}

static void run_mode_main_loop(void) {
    // Load creds (already verified by boot_mode_decide), bring up Wi-Fi,
    // bring up UDP, then poll forever.
    //
    // Wi-Fi is the payload, but the selected output persona is the
    // device's identity: once we hold valid credentials we never give up
    // either. A radio that will not initialise, or an AP that will not
    // associate, must not cost the user their controller -- we stay
    // enumerated and keep retrying. The only reason run mode ever leaves
    // is a *sustained* definitive auth rejection (the stored password is
    // wrong and only the user can fix it) or a radio that cannot init at
    // all for a long window (broken hardware/power). Both bounce to setup
    // mode with the reason recorded in the cross-reset breadcrumb, since
    // the run-mode diag ring is RAM-only and would be lost on reboot.
    flash_creds_t creds;
    if (!flash_creds_load(&creds)) {
        diag_log_msg("run: lost creds between boot and run -- rebooting to setup");
        watchdog_reboot(0, 0, 100);
        for (;;)
            tight_loop_contents();
    }

    // Issue the join once the radio is up, then wipe the local password.
    bool join_issued = false;
    bool radio_up = wifi_init();
    if (radio_up) {
        run_issue_join(&creds);
        join_issued = true;
    }
    uint32_t radio_init_fails = radio_up ? 0u : 1u;
    absolute_time_t radio_retry_at = make_timeout_time_ms(3000);
    absolute_time_t radio_failing_since = get_absolute_time();

    bool udp_inited = false;

    // Sustained-BADAUTH escape hatch. wifi.c already retries every failure
    // mode forever (NONET, timeout, generic, link-drop, DHCP regrab), so a
    // single bad result is never a reason to abandon run mode -- a fresh
    // radio's first scan after a replug routinely reports NONET, and
    // link_status can briefly mis-report BADAUTH (cyw43-driver #62). Only a
    // password that is *continuously* rejected means re-provisioning is
    // needed, so we require BADAUTH to persist before bouncing.
    absolute_time_t badauth_since = get_absolute_time();
    bool badauth_armed = false;

    run_persona_t persona = boot_mode_run_persona();
    if (persona == RUN_PERSONA_KEYBOARD) {
        hid_kbd_init();
        diag_log_msg("run: USB persona = HID keyboard");
    } else if (persona == RUN_PERSONA_N64) {
        n64_backend_init();
        diag_log_msg("run: persona = Nintendo 64 Joybus");
    } else if (boot_mode_persona_uses_gamepad_hid(persona)) {
        dinput_init();
        if (persona == RUN_PERSONA_PS4)
            diag_log_msg("run: USB persona = Sony DualShock 4 / PS4 HID");
        else if (persona == RUN_PERSONA_GENERIC_HID)
            diag_log_msg("run: USB persona = generic HID gamepad");
        else
            diag_log_msg("run: USB persona = Sony DualShock 3 / PS3 HID");
    } else if (persona == RUN_PERSONA_XBOXONE) {
        xbone_init();
        diag_log_msg("run: USB persona = Xbox One XGIP");
    } else if (persona == RUN_PERSONA_MAPLE) {
        xinput_init();
        diag_log_msg("run: USB persona = XInput controller for Maple adapter");
    } else if (persona == RUN_PERSONA_DEBUG) {
        xinput_init();
        diag_log_msg("run: USB persona = debug packet capture (XInput descriptor)");
    } else {
        xinput_init();
        diag_log_msg("run: USB persona = XInput controller");
    }
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

        if (!radio_up) {
            // Stay a live (idle) controller while the radio refuses to
            // come up; retry cyw43_arch_init from the loop instead of
            // halting, so a transient init failure self-heals.
            if (absolute_time_diff_us(get_absolute_time(), radio_retry_at) <= 0) {
                radio_up = wifi_init();
                if (radio_up) {
                    diag_log_printf("run: cyw43 init recovered after %u failure(s)",
                                    (unsigned)radio_init_fails);
                    run_issue_join(&creds);
                    join_issued = true;
                } else {
                    radio_init_fails++;
                    radio_retry_at = make_timeout_time_ms(3000);
                    // A radio that cannot init for a sustained window is
                    // broken hardware/power, not a hiccup. Bounce to setup
                    // with the rc recorded so a bundle explains why.
                    if (absolute_time_diff_us(radio_failing_since, get_absolute_time()) >=
                        60LL * 1000 * 1000) {
                        char note[RESET_REASON_LAST_LINE_CAP];
                        snprintf(note, sizeof(note), "wifi: cyw43 init rc=%d x%u",
                                 wifi_last_init_rc(), (unsigned)radio_init_fails);
                        diag_log_printf("run: %s -- bouncing to setup for diagnosis", note);
                        reset_reason_request_setup_with_note(note);
                        watchdog_reboot(0, 0, 100);
                        for (;;)
                            tight_loop_contents();
                    }
                }
            }
        } else {
            cyw43_arch_poll();
            wifi_task();

            if (wifi_state() == WIFI_STATE_JOINED && !udp_inited) {
                udp_inited = net_udp_init();
            }
            if (udp_inited) {
                net_udp_task();
            }
        }

        if (persona == RUN_PERSONA_KEYBOARD)
            hid_kbd_task();
        else if (persona == RUN_PERSONA_N64)
            n64_backend_task();
        else if (boot_mode_persona_uses_gamepad_hid(persona))
            dinput_task();
        else if (persona == RUN_PERSONA_XBOXONE)
            xbone_task();
        else
            xinput_task();
        watchdog_tick();
        heartbeat_run_mode_task();

        // Sustained-BADAUTH watchdog. Arm on the first auth rejection and
        // disarm the instant we join or see any other state, so only a
        // continuously-wrong password ever reaches the timeout and bounces.
        if (join_issued && wifi_state() != WIFI_STATE_JOINED &&
            wifi_last_error_code() == CDC_ERR_AUTH_FAIL) {
            if (!badauth_armed) {
                badauth_since = get_absolute_time();
                badauth_armed = true;
            } else if (absolute_time_diff_us(badauth_since, get_absolute_time()) >=
                       120LL * 1000 * 1000) {
                diag_log_msg("wifi: auth rejected for 120 s -- bouncing to setup so the password "
                             "can be re-entered");
                reset_reason_request_setup_with_note("wifi: BADAUTH sustained 120s");
                watchdog_reboot(0, 0, 100);
                for (;;)
                    tight_loop_contents();
            }
        } else {
            badauth_armed = false;
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
            for (;;)
                tight_loop_contents();
        }
        sleep_us(500);
    }
}

int main(void) {
    stdio_init_all();
    diag_log_init();
    usb_diag_init();

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
    const char *persona_name = "CDC+diag";
    if (mode == BOOT_MODE_RUN) {
        switch (boot_mode_run_persona()) {
        case RUN_PERSONA_XINPUT:
            persona_name = "XInput";
            break;
        case RUN_PERSONA_KEYBOARD:
            persona_name = "HID keyboard";
            break;
        case RUN_PERSONA_MAPLE:
            persona_name = "Maple";
            break;
        case RUN_PERSONA_PS3:
            persona_name = "PS3";
            break;
        case RUN_PERSONA_PS4:
            persona_name = "PS4";
            break;
        case RUN_PERSONA_XBOXONE:
            persona_name = "Xbox One";
            break;
        case RUN_PERSONA_DEBUG:
            persona_name = "debug packet capture";
            break;
        case RUN_PERSONA_GENERIC_HID:
            persona_name = "generic HID gamepad";
            break;
        case RUN_PERSONA_N64:
            persona_name = "Nintendo 64 Joybus";
            break;
        }
    }
    uint8_t usb_capture_persona = 0;
    if (reset_reason_consume_usb_capture_request(&usb_capture_persona)) {
        if (mode == BOOT_MODE_RUN && usb_capture_persona == (uint8_t)boot_mode_run_persona()) {
            usb_packet_debug_set_capture_enabled(true);
            diag_log_printf("usb_capture: enabled for persona=%u before tusb_init",
                            (unsigned)usb_capture_persona);
        } else {
            diag_log_printf("usb_capture: ignored marker persona=%u mode=%d active_persona=%u",
                            (unsigned)usb_capture_persona, (int)mode,
                            (unsigned)boot_mode_run_persona());
        }
    }
    diag_log_printf("boot: mode decided: %s persona will be advertised", persona_name);

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
        absolute_time_t deadline = make_timeout_time_ms(1500);
        bool mounted_in_pump = false;
        while (!time_reached(deadline)) {
            tud_task();
            if (tud_mounted()) {
                mounted_in_pump = true;
                break;
            }
            sleep_us(500);
        }
        uint32_t elapsed_ms =
            (uint32_t)(absolute_time_diff_us(pump_start, get_absolute_time()) / 1000);
        if (mounted_in_pump) {
            diag_log_printf("usb_init: enumeration completed during pump (%u ms)",
                            (unsigned)elapsed_ms);
        } else {
            diag_log_printf("usb_init: pump timeout after %u ms (mounted=%d) -- "
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
