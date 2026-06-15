#pragma once

// TinyUSB configuration. We host two different USB personas at runtime:
//   - setup mode:          CDC ACM under Raspberry Pi VID 0x2E8A / PID 0xCAF0
//   - run / controller:    wired Xbox 360 (XUSB, vendor class) VID 0x045E / PID 0x028E
//   - run / keyboard:      USB HID boot keyboard under VID 0x2E8A / PID 0xCAF1
// Only one persona is presented at a time, fixed at boot before
// tusb_init(). Every class must be enabled so the build includes the
// relevant TinyUSB code paths.

#ifdef __cplusplus
extern "C" {
#endif

// MCU select. pico-sdk defines CFG_TUSB_MCU for us; this guard is a
// belt-and-braces in case board headers don't.
#ifndef CFG_TUSB_MCU
#define CFG_TUSB_MCU OPT_MCU_RP2040
#endif

#define CFG_TUSB_OS OPT_OS_PICO
#define CFG_TUSB_DEBUG 0
#define CFG_TUSB_RHPORT0_MODE OPT_MODE_DEVICE

#define CFG_TUD_ENABLED 1
#define CFG_TUSB_MEM_SECTION
#define CFG_TUSB_MEM_ALIGN __attribute__((aligned(4)))

#define CFG_TUD_ENDPOINT0_SIZE 64

// Class drivers. CDC for setup-mode provisioning; Vendor for XUSB; HID
// for the keyboard persona.
#define CFG_TUD_CDC 1
#define CFG_TUD_VENDOR 1
#define CFG_TUD_HID 1
#define CFG_TUD_MSC 0
#define CFG_TUD_MIDI 0
#define CFG_TUD_DFU 0
#define CFG_TUD_DFU_RUNTIME 0
#define CFG_TUD_NCM 0

// CDC FIFO sizes. CDC frames are up to ~260 bytes; size FIFOs ~2x to
// avoid blocking when the host sends back-to-back commands.
#define CFG_TUD_CDC_RX_BUFSIZE 512
#define CFG_TUD_CDC_TX_BUFSIZE 512

// XInput uses 20-byte interrupt-IN and 8-byte interrupt-OUT.
#define CFG_TUD_VENDOR_RX_BUFSIZE 64
#define CFG_TUD_VENDOR_TX_BUFSIZE 64

// HID keyboard: canonical 8-byte boot report in, 1-byte LED report out.
// Keep the endpoint at the standard boot-keyboard size of 8 so strict
// HID hosts (and the Dreamcast adapter) see exactly what they expect.
#define CFG_TUD_HID_EP_BUFSIZE 8

#ifdef __cplusplus
}
#endif
