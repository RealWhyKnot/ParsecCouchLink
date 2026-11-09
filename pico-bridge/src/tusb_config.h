#pragma once

#ifdef __cplusplus
extern "C" {
#endif

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

#define CFG_TUD_CDC 1
#define CFG_TUD_VENDOR 1
#define CFG_TUD_HID 1
#define CFG_TUD_MSC 0
#define CFG_TUD_MIDI 0
#define CFG_TUD_DFU 0
#define CFG_TUD_DFU_RUNTIME 0
#define CFG_TUD_NCM 0

#define CFG_TUD_CDC_RX_BUFSIZE 512
#define CFG_TUD_CDC_TX_BUFSIZE 512

#define CFG_TUD_VENDOR_RX_BUFSIZE 64
#define CFG_TUD_VENDOR_TX_BUFSIZE 64

#define CFG_TUD_HID_EP_BUFSIZE 64

#ifdef __cplusplus
}
#endif
