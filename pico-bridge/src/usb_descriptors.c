// USB descriptors for Pico setup mode and runtime personas:
//
//   setup mode: CDC ACM + WinUSB diag (Raspberry Pi VID 0x2E8A, PID 0xCAF0)
//   run / xinput: wired Xbox 360 / XUSB (Microsoft VID 0x045E, PID 0x028E)
//   run / keyboard:   USB HID boot keyboard (Raspberry Pi VID 0x2E8A, PID 0xCAF1)
//   run / maple:      wired Xbox 360 / XUSB for Dreamcast Maple adapters
//   run / ps3:        Sony DualShock 3 HID gamepad (Sony VID 0x054C, PID 0x0268)
//   run / ps4:        Sony DualShock 4 HID gamepad (Sony VID 0x054C, PID 0x09CC)
//   run / xboxone:    Xbox One-compatible XGIP vendor-class gamepad
//   run / generic-hid: generic HID gamepad (Raspberry Pi VID 0x2E8A, PID 0xCAF2)
//
// Only one persona is presented at a time. main() calls boot_mode_decide()
// before tusb_init(), so D+ is raised exactly once with the final mode
// already committed. The descriptor callbacks below therefore always
// return the correct persona on first enumeration -- no re-enumeration
// race.
//
// Setup mode's CDC composite carries a third interface (interface 2,
// vendor-class, no endpoints) that Windows binds to WinUSB via the MS
// OS 2.0 descriptor set further down. The host reads the diag log via
// a vendor control transfer on EP0; this works regardless of the CDC
// bulk endpoint state, breaking the catch-22 where a wedged CDC FIFO
// blocked diag retrieval.
//
// The XInput descriptor + magic 17-byte unknown descriptor are lifted
// from Ryzee119/tusb_xinput (MIT). The 17-byte unknown descriptor is
// required for Windows xusb22.sys to accept the device as a wired Xbox
// 360 controller. The XInput persona deliberately does NOT carry the
// vendor diag interface -- xusb22.sys binding is fragile with respect
// to extra interfaces, and run mode has UDP TYPE_GET_LOG for diag.

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "tusb.h"
#include "pico/unique_id.h"

#include "boot_mode.h"
#include "diag_log.h"
#include "dinput.h"
#include "dinput_report.h"
#include "hid_kbd.h"
#include "usb_diag.h"
#include "usb_packet_debug.h"
#include "version.h"
#include "xbone.h"
#include "xgip_constants.h"
#include "xinput.h"

// bcdDevice is keyed by Windows usbflags / driver-binding cache. It is
// intentionally independent from product firmware version and protocol
// version so a USB binding cache refresh can be made explicit.
#define BCD_DEVICE_VERSION PICO_BRIDGE_USB_BCD_DEVICE

// -------- common: device descriptors -----------------------------------

static const tusb_desc_device_t desc_device_cdc = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x2E8A,  // Raspberry Pi
    .idProduct = 0xCAF0, // sub-licensed; see raspberrypi/usb-pid
    .bcdDevice = BCD_DEVICE_VERSION,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_xinput = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0xFF, // vendor-specific
    .bDeviceSubClass = 0xFF,
    .bDeviceProtocol = 0xFF,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x045E,  // Microsoft
    .idProduct = 0x028E, // wired Xbox 360 controller
    .bcdDevice = 0x0114,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

// Keyboard persona: a plain USB HID boot keyboard. Class is declared at
// the interface level, so the device descriptor is class 0x00. A
// HID-keyboard-aware console adapter (e.g. USB4MAPLE/Pico2Maple feeding a
// Dreamcast) binds on the HID boot interface, so the VID/PID is cosmetic;
// we keep the Raspberry Pi VID with a distinct product id.
static const tusb_desc_device_t desc_device_keyboard = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x2E8A,  // Raspberry Pi
    .idProduct = 0xCAF1, // CouchLink keyboard persona
    .bcdDevice = BCD_DEVICE_VERSION,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_generic_hid = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x2E8A,  // Raspberry Pi
    .idProduct = 0xCAF2, // CouchLink generic HID gamepad persona
    .bcdDevice = BCD_DEVICE_VERSION,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_ps3 = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x054C,  // Sony
    .idProduct = 0x0268, // PLAYSTATION(R)3 Controller
    .bcdDevice = 0x0100,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x00,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_ps4 = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x054C,  // Sony
    .idProduct = 0x09CC, // DualShock 4 / Wireless Controller
    .bcdDevice = 0x0100,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_xboxone = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = XGIP_DEVICE_CLASS,
    .bDeviceSubClass = XGIP_DEVICE_SUBCLASS,
    .bDeviceProtocol = XGIP_DEVICE_PROTOCOL,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x0E6F, // PDP Xbox One-compatible controller
    .idProduct = 0x02A4,
    .bcdDevice = BCD_DEVICE_VERSION,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

uint8_t const *tud_descriptor_device_cb(void) {
    static volatile bool logged = false;
    usb_diag_note_device_descriptor();
    if (!logged) {
        logged = true;
        diag_log_msg("usb_init: first GET_DESCRIPTOR(DEVICE) reply sent");
    }
    uint8_t const *desc = NULL;
    if (boot_mode_current() != BOOT_MODE_RUN) {
        desc = (uint8_t const *)&desc_device_cdc;
        usb_packet_debug_note_control_in("desc-device", desc, desc[0]);
        return desc;
    }
    switch (boot_mode_run_persona()) {
    case RUN_PERSONA_PS3:
        desc = (uint8_t const *)&desc_device_ps3;
        break;
    case RUN_PERSONA_PS4:
        desc = (uint8_t const *)&desc_device_ps4;
        break;
    case RUN_PERSONA_XBOXONE:
        desc = (uint8_t const *)&desc_device_xboxone;
        break;
    case RUN_PERSONA_GENERIC_HID:
        desc = (uint8_t const *)&desc_device_generic_hid;
        break;
    case RUN_PERSONA_KEYBOARD:
        desc = (uint8_t const *)&desc_device_keyboard;
        break;
    case RUN_PERSONA_XINPUT:
    case RUN_PERSONA_MAPLE:
    default:
        desc = (uint8_t const *)&desc_device_xinput;
        break;
    }
    usb_packet_debug_note_control_in("desc-device", desc, desc[0]);
    return desc;
}

// -------- setup mode: CDC + WinUSB diag configuration ------------------

enum { CDC_ITF_NUM_NOTIF = 0, CDC_ITF_NUM_DATA, DIAG_ITF_NUM, CDC_ITF_COUNT };

#define DIAG_VENDOR_DESC_LEN 9 // interface descriptor only, no endpoints
#define CDC_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_CDC_DESC_LEN + DIAG_VENDOR_DESC_LEN)

#define CDC_NOTIF_EP_ADDR 0x82
#define CDC_OUT_EP_ADDR 0x03
#define CDC_IN_EP_ADDR 0x83

static const uint8_t desc_configuration_cdc[] = {
    TUD_CONFIG_DESCRIPTOR(1, CDC_ITF_COUNT, 0, CDC_CONFIG_TOTAL_LEN, 0xA0, 100),
    TUD_CDC_DESCRIPTOR(CDC_ITF_NUM_NOTIF, 4, CDC_NOTIF_EP_ADDR, 8, CDC_OUT_EP_ADDR, CDC_IN_EP_ADDR,
                       64),

    // Interface 2: diag vendor interface. Class 0xFF, no endpoints --
    // the diag log is read via a vendor control transfer on EP0 (see
    // tud_vendor_control_xfer_cb below). Bound to WinUSB on Windows via
    // the MS OS 2.0 descriptor set (see further down).
    // clang-format off
    9, TUSB_DESC_INTERFACE, DIAG_ITF_NUM, 0, 0, 0xFF, 0x00, 0x00,
    // clang-format on
    5, // iInterface = STRID_DIAG_INTERFACE (see string enum below)
};

// -------- run mode: XInput configuration --------------------------------

#define XINPUT_IN_EP_ADDR 0x81
#define XINPUT_OUT_EP_ADDR 0x02
#define XINPUT_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + 9 /*interface*/ + 17 /*magic*/ + 7 + 7)

static const uint8_t desc_configuration_xinput[] = {
    // Configuration descriptor
    9,
    TUSB_DESC_CONFIGURATION,
    U16_TO_U8S_LE(XINPUT_CONFIG_TOTAL_LEN),
    1,
    1,
    0,
    0xA0,
    250, // bmAttributes=bus-powered, bMaxPower=500mA

    // Interface 0: vendor-specific, class 0xFF, subclass 0x5D, protocol 0x01
    9,
    TUSB_DESC_INTERFACE,
    0,
    0,
    2, // bInterfaceNumber=0, bAlternateSetting=0, bNumEndpoints=2
    0xFF,
    0x5D,
    0x01,
    0, // iInterface

    // Magic 17-byte unknown descriptor; required by xusb22.sys.
    0x11,
    0x21,
    0x10,
    0x01,
    0x01,
    0x24,
    0x81,
    0x14,
    0x03,
    0x00,
    0x03,
    0x13,
    0x02,
    0x00,
    0x03,
    0x00,
    0x00,

    // Endpoint 0x81 IN, interrupt, 20-byte report. At full speed bInterval is
    // counted in 1 ms frames, so bInterval=1 requests a 1 ms (1000 Hz) poll for
    // the lowest input latency. (The real wired Xbox 360 pad asks for 4 ms /
    // 250 Hz; advertising the faster rate only lets the host poll more often --
    // the report itself is still produced on change, so there is no extra load.)
    7,
    TUSB_DESC_ENDPOINT,
    XINPUT_IN_EP_ADDR,
    0x03, // bmAttributes = Interrupt
    U16_TO_U8S_LE(20),
    1, // bInterval = 1 ms (1000 Hz) at full speed

    // Endpoint 0x02 OUT, interrupt, 32 bytes, 8 ms (rumble + LED ring writes from the host)
    7,
    TUSB_DESC_ENDPOINT,
    XINPUT_OUT_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(32),
    8,
};

// -------- run mode: keyboard configuration ------------------------------

#define KBD_ITF_NUM 0
#define KBD_IN_EP_ADDR 0x81

// Standard 8-byte boot-keyboard report descriptor (report id 0).
static const uint8_t desc_hid_report_keyboard[] = {TUD_HID_REPORT_DESC_KEYBOARD()};

#define KBD_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN)

static const uint8_t desc_configuration_keyboard[] = {
    TUD_CONFIG_DESCRIPTOR(1, 1, 0, KBD_CONFIG_TOTAL_LEN, 0xA0, 100),
    // Boot keyboard: subclass BOOT, protocol KEYBOARD, one 10 ms
    // interrupt-IN endpoint carrying the 8-byte report.
    TUD_HID_DESCRIPTOR(KBD_ITF_NUM, 0, HID_ITF_PROTOCOL_KEYBOARD, sizeof(desc_hid_report_keyboard),
                       KBD_IN_EP_ADDR, CFG_TUD_HID_EP_BUFSIZE, 10),
};

// -------- run mode: HID gamepad configurations -------------------------

#define GAMEPAD_HID_ITF_NUM 0
#define GAMEPAD_HID_IN_EP_ADDR 0x81
#define GAMEPAD_HID_OUT_EP_ADDR 0x02

static const uint8_t desc_hid_report_ps3[] = {
    0x05, 0x01, 0x09, 0x04, 0xA1, 0x01, 0xA1, 0x02, 0x85, 0x01, 0x75, 0x08, 0x95, 0x01, 0x15,
    0x00, 0x26, 0xFF, 0x00, 0x81, 0x03, 0x75, 0x01, 0x95, 0x13, 0x15, 0x00, 0x25, 0x01, 0x35,
    0x00, 0x45, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x13, 0x81, 0x02, 0x75, 0x01, 0x95, 0x0D,
    0x06, 0x00, 0xFF, 0x81, 0x03, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x05, 0x01, 0x09, 0x01, 0xA1,
    0x00, 0x75, 0x08, 0x95, 0x04, 0x35, 0x00, 0x46, 0xFF, 0x00, 0x09, 0x30, 0x09, 0x31, 0x09,
    0x32, 0x09, 0x35, 0x81, 0x02, 0xC0, 0x05, 0x01, 0x75, 0x08, 0x95, 0x27, 0x09, 0x01, 0x81,
    0x02, 0x75, 0x08, 0x95, 0x30, 0x09, 0x01, 0x91, 0x02, 0x75, 0x08, 0x95, 0x30, 0x09, 0x01,
    0xB1, 0x02, 0xC0, 0xA1, 0x02, 0x85, 0x02, 0x75, 0x08, 0x95, 0x30, 0x09, 0x01, 0xB1, 0x02,
    0xC0, 0xA1, 0x02, 0x85, 0xEE, 0x75, 0x08, 0x95, 0x30, 0x09, 0x01, 0xB1, 0x02, 0xC0, 0xA1,
    0x02, 0x85, 0xEF, 0x75, 0x08, 0x95, 0x30, 0x09, 0x01, 0xB1, 0x02, 0xC0, 0xC0,
};

static const uint8_t desc_hid_report_ps4[] = {
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x09,
    0x35, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02, 0x09, 0x39, 0x15,
    0x00, 0x25, 0x07, 0x35, 0x00, 0x46, 0x3B, 0x01, 0x65, 0x14, 0x75, 0x04, 0x95, 0x01, 0x81,
    0x42, 0x65, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0E, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01,
    0x95, 0x0E, 0x81, 0x02, 0x06, 0x00, 0xFF, 0x09, 0x20, 0x75, 0x06, 0x95, 0x01, 0x81, 0x02,
    0x05, 0x01, 0x09, 0x33, 0x09, 0x34, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x02,
    0x81, 0x02, 0x06, 0x00, 0xFF, 0x09, 0x21, 0x95, 0x36, 0x81, 0x02, 0x85, 0x05, 0x09, 0x22,
    0x95, 0x1F, 0x91, 0x02, 0x85, 0x02, 0x09, 0x24, 0x95, 0x24, 0xB1, 0x02, 0x85, 0x03, 0x0A,
    0x21, 0x27, 0x95, 0x2F, 0xB1, 0x02, 0x85, 0x12, 0x06, 0x02, 0xFF, 0x09, 0x21, 0x95, 0x0F,
    0xB1, 0x02, 0x85, 0xA3, 0x06, 0x05, 0xFF, 0x09, 0x43, 0x95, 0x30, 0xB1, 0x02, 0x06, 0xF0,
    0xFF, 0x85, 0xF1, 0x09, 0x48, 0x95, 0x3F, 0xB1, 0x02, 0x85, 0xF2, 0x09, 0x49, 0x95, 0x0F,
    0xB1, 0x02, 0x85, 0xF3, 0x0A, 0x01, 0x47, 0x95, 0x07, 0xB1, 0x02, 0xC0,
};

// Generic HID gamepad report, no report ID:
//   byte 0..1: 12 buttons, b0..b11 = X,A,B,Y,LB,RB,LT,RT,SELECT,START,L3,R3
//   byte 2..7: X,Y,Rx,Ry,Z,Rz axes (Z/Rz carry analog LT/RT)
static const uint8_t desc_hid_report_generic_hid[] = {
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x05,       // Usage (Game Pad)
    0xA1, 0x01,       // Collection (Application)
    0x05, 0x09,       //   Usage Page (Button)
    0x19, 0x01,       //   Usage Minimum (Button 1)
    0x29, 0x0C,       //   Usage Maximum (Button 12)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x0C,       //   Report Count (12)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x04,       //   Report Count (4)
    0x81, 0x03,       //   Input (Const,Var,Abs)
    0x05, 0x01,       //   Usage Page (Generic Desktop)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x75, 0x08,       //   Report Size (8)
    0x95, 0x06,       //   Report Count (6)
    0x09, 0x30,       //   Usage (X)
    0x09, 0x31,       //   Usage (Y)
    0x09, 0x33,       //   Usage (Rx)
    0x09, 0x34,       //   Usage (Ry)
    0x09, 0x32,       //   Usage (Z)
    0x09, 0x35,       //   Usage (Rz)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0xC0,             // End Collection
};

#define HID_GAMEPAD_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN + 7)
#define GENERIC_HID_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN)

static const uint8_t desc_configuration_ps3[] = {
    TUD_CONFIG_DESCRIPTOR(1, 1, 0, HID_GAMEPAD_CONFIG_TOTAL_LEN, 0x80, 500),
    9,
    TUSB_DESC_INTERFACE,
    GAMEPAD_HID_ITF_NUM,
    0,
    2,
    0x03,
    0,
    HID_ITF_PROTOCOL_NONE,
    0,
    9,
    0x21,
    U16_TO_U8S_LE(0x0111),
    0,
    1,
    0x22,
    U16_TO_U8S_LE(sizeof(desc_hid_report_ps3)),
    7,
    TUSB_DESC_ENDPOINT,
    GAMEPAD_HID_OUT_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(CFG_TUD_HID_EP_BUFSIZE),
    1,
    7,
    TUSB_DESC_ENDPOINT,
    GAMEPAD_HID_IN_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(CFG_TUD_HID_EP_BUFSIZE),
    1,
};

static const uint8_t desc_configuration_ps4[] = {
    TUD_CONFIG_DESCRIPTOR(1, 1, 0, HID_GAMEPAD_CONFIG_TOTAL_LEN, 0x80, 50),
    9,
    TUSB_DESC_INTERFACE,
    GAMEPAD_HID_ITF_NUM,
    0,
    2,
    0x03,
    0,
    HID_ITF_PROTOCOL_NONE,
    0,
    9,
    0x21,
    U16_TO_U8S_LE(0x0111),
    0,
    1,
    0x22,
    U16_TO_U8S_LE(sizeof(desc_hid_report_ps4)),
    7,
    TUSB_DESC_ENDPOINT,
    GAMEPAD_HID_OUT_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(CFG_TUD_HID_EP_BUFSIZE),
    1,
    7,
    TUSB_DESC_ENDPOINT,
    GAMEPAD_HID_IN_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(CFG_TUD_HID_EP_BUFSIZE),
    1,
};

static const uint8_t desc_configuration_generic_hid[] = {
    TUD_CONFIG_DESCRIPTOR(1, 1, 0, GENERIC_HID_CONFIG_TOTAL_LEN, 0x80, 100),
    TUD_HID_DESCRIPTOR(GAMEPAD_HID_ITF_NUM, 0, HID_ITF_PROTOCOL_NONE,
                       sizeof(desc_hid_report_generic_hid), GAMEPAD_HID_IN_EP_ADDR,
                       CFG_TUD_HID_EP_BUFSIZE, 1),
};

// -------- run mode: Xbox One-compatible XGIP configuration -------------

#define XBONE_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + 9 + 7 + 7)

static const uint8_t desc_configuration_xboxone[] = {
    9,
    TUSB_DESC_CONFIGURATION,
    U16_TO_U8S_LE(XBONE_CONFIG_TOTAL_LEN),
    1,
    1,
    0,
    0xA0,
    250,
    9,
    TUSB_DESC_INTERFACE,
    0,
    0,
    2,
    0xFF,
    0x47,
    0xD0,
    0,
    7,
    TUSB_DESC_ENDPOINT,
    0x81,
    0x03,
    U16_TO_U8S_LE(64),
    1,
    7,
    TUSB_DESC_ENDPOINT,
    0x02,
    0x03,
    U16_TO_U8S_LE(64),
    1,
};

// The config descriptor's wTotalLength (KBD_CONFIG_TOTAL_LEN) must equal
// the bytes actually emitted, or the host stops parsing mid-descriptor.
_Static_assert(sizeof(desc_configuration_keyboard) == KBD_CONFIG_TOTAL_LEN,
               "keyboard configuration descriptor length mismatch");
_Static_assert(sizeof(desc_configuration_ps3) == HID_GAMEPAD_CONFIG_TOTAL_LEN,
               "PS3 configuration descriptor length mismatch");
_Static_assert(sizeof(desc_configuration_ps4) == HID_GAMEPAD_CONFIG_TOTAL_LEN,
               "PS4 configuration descriptor length mismatch");
_Static_assert(sizeof(desc_configuration_generic_hid) == GENERIC_HID_CONFIG_TOTAL_LEN,
               "generic HID configuration descriptor length mismatch");
_Static_assert(sizeof(desc_configuration_xboxone) == XBONE_CONFIG_TOTAL_LEN,
               "Xbox One configuration descriptor length mismatch");
// The boot keyboard IN report is 8 bytes; the endpoint buffer must hold it.
_Static_assert(CFG_TUD_HID_EP_BUFSIZE >= 8,
               "CFG_TUD_HID_EP_BUFSIZE too small for the 8-byte boot keyboard report");
_Static_assert(CFG_TUD_HID_EP_BUFSIZE >= DINPUT_PS4_WIRE_REPORT_LEN,
               "CFG_TUD_HID_EP_BUFSIZE too small for the HID gamepad report");

uint8_t const *tud_descriptor_configuration_cb(uint8_t index) {
    (void)index;
    static bool logged = false;
    usb_diag_note_configuration_descriptor();
    if (!logged) {
        diag_log_msg("usb: host requested configuration descriptor (enum step 2)");
        logged = true;
    }
    uint8_t const *desc = NULL;
    if (boot_mode_current() != BOOT_MODE_RUN) {
        desc = desc_configuration_cdc;
        usb_packet_debug_note_control_in("desc-config", desc,
                                         (uint16_t)(desc[2] | ((uint16_t)desc[3] << 8)));
        return desc;
    }
    switch (boot_mode_run_persona()) {
    case RUN_PERSONA_PS3:
        desc = desc_configuration_ps3;
        break;
    case RUN_PERSONA_PS4:
        desc = desc_configuration_ps4;
        break;
    case RUN_PERSONA_XBOXONE:
        desc = desc_configuration_xboxone;
        break;
    case RUN_PERSONA_GENERIC_HID:
        desc = desc_configuration_generic_hid;
        break;
    case RUN_PERSONA_KEYBOARD:
        desc = desc_configuration_keyboard;
        break;
    case RUN_PERSONA_XINPUT:
    case RUN_PERSONA_MAPLE:
    default:
        desc = desc_configuration_xinput;
        break;
    }
    usb_packet_debug_note_control_in("desc-config", desc,
                                     (uint16_t)(desc[2] | ((uint16_t)desc[3] << 8)));
    return desc;
}

// -------- string descriptors --------------------------------------------

enum {
    STRID_LANGID = 0,
    STRID_MANUFACTURER,
    STRID_PRODUCT,
    STRID_SERIAL,
    STRID_CDC_INTERFACE,
    STRID_DIAG_INTERFACE,
};

static char serial_str[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES + 1];

static const char *const string_desc_arr[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04}, // English (US)
    [STRID_MANUFACTURER] = "Parsec CouchLink",
    [STRID_PRODUCT] = "CouchLink Pico",
    [STRID_SERIAL] = serial_str,
    [STRID_CDC_INTERFACE] = "Parsec CouchLink Setup",
    [STRID_DIAG_INTERFACE] = "Parsec CouchLink Diag",
};

// XInput wants Microsoft-y strings so xusb22.sys binds without
// complaining. Real wired 360 pads report these (or close enough).
static const char *const string_desc_arr_xinput[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "(c) Microsoft Corporation",
    [STRID_PRODUCT] = "Controller",
    [STRID_SERIAL] = serial_str,
};

static const char *const string_desc_arr_ps3[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "Sony",
    [STRID_PRODUCT] = "PLAYSTATION(R)3 Controller",
    [STRID_SERIAL] = serial_str,
};

static const char *const string_desc_arr_ps4[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "Sony Interactive Entertainment",
    [STRID_PRODUCT] = "Wireless Controller",
    [STRID_SERIAL] = serial_str,
};

static const char *const string_desc_arr_generic_hid[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "Parsec CouchLink",
    [STRID_PRODUCT] = "Generic HID Gamepad",
    [STRID_SERIAL] = serial_str,
};

static const char *const string_desc_arr_xboxone[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "Performance Designed Products",
    [STRID_PRODUCT] = "Controller",
    [STRID_SERIAL] = serial_str,
};

static uint16_t string_buf[127]; // descriptor type + length prefix + up to 126 chars

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid) {
    (void)langid;

    // Lazily fill the serial string from the SoC unique ID.
    if (serial_str[0] == 0) {
        pico_unique_board_id_t id;
        pico_get_unique_board_id(&id);
        for (int i = 0; i < PICO_UNIQUE_BOARD_ID_SIZE_BYTES; i++) {
            static const char hex[] = "0123456789ABCDEF";
            serial_str[i * 2 + 0] = hex[(id.id[i] >> 4) & 0xF];
            serial_str[i * 2 + 1] = hex[id.id[i] & 0xF];
        }
        serial_str[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES] = 0;
    }

    if (boot_mode_current() == BOOT_MODE_RUN && boot_mode_run_persona() == RUN_PERSONA_XBOXONE &&
        index == XGIP_MS_OS_STRING_INDEX) {
        string_buf[0] = (TUSB_DESC_STRING << 8) | 18;
        string_buf[1] = 'M';
        string_buf[2] = 'S';
        string_buf[3] = 'F';
        string_buf[4] = 'T';
        string_buf[5] = '1';
        string_buf[6] = '0';
        string_buf[7] = '0';
        string_buf[8] = XGIP_MS_VENDOR_REQ_CODE;
        usb_packet_debug_note_control_in("desc-string-msos10", (uint8_t const *)string_buf,
                                         (uint16_t)(string_buf[0] & 0xFFu));
        return string_buf;
    }

    // Run personas use strings that match the USB identity they expose.
    // The keyboard persona uses the default CouchLink strings, same as
    // setup mode.
    const char *const *arr = string_desc_arr;
    size_t arr_count = sizeof(string_desc_arr) / sizeof(string_desc_arr[0]);
    if (boot_mode_current() == BOOT_MODE_RUN) {
        if (boot_mode_persona_uses_xinput_usb(boot_mode_run_persona())) {
            arr = string_desc_arr_xinput;
            arr_count = sizeof(string_desc_arr_xinput) / sizeof(string_desc_arr_xinput[0]);
        } else if (boot_mode_run_persona() == RUN_PERSONA_PS3) {
            arr = string_desc_arr_ps3;
            arr_count = sizeof(string_desc_arr_ps3) / sizeof(string_desc_arr_ps3[0]);
        } else if (boot_mode_run_persona() == RUN_PERSONA_PS4) {
            arr = string_desc_arr_ps4;
            arr_count = sizeof(string_desc_arr_ps4) / sizeof(string_desc_arr_ps4[0]);
        } else if (boot_mode_run_persona() == RUN_PERSONA_GENERIC_HID) {
            arr = string_desc_arr_generic_hid;
            arr_count =
                sizeof(string_desc_arr_generic_hid) / sizeof(string_desc_arr_generic_hid[0]);
        } else if (boot_mode_run_persona() == RUN_PERSONA_XBOXONE) {
            arr = string_desc_arr_xboxone;
            arr_count = sizeof(string_desc_arr_xboxone) / sizeof(string_desc_arr_xboxone[0]);
        }
    }

    if (index >= arr_count)
        return NULL;

    if (index == STRID_LANGID) {
        // LANGID descriptor is exactly 4 bytes: length(1)+type(1)+langid(2).
        string_buf[0] = (TUSB_DESC_STRING << 8) | 4;
        memcpy(&string_buf[1], arr[0], 2);
        usb_packet_debug_note_control_in("desc-string", (uint8_t const *)string_buf,
                                         (uint16_t)(string_buf[0] & 0xFFu));
        return string_buf;
    }

    const char *str = arr[index];
    if (!str)
        return NULL;
    size_t len = strlen(str);
    if (len > 126)
        len = 126;
    for (size_t i = 0; i < len; i++)
        string_buf[1 + i] = (uint16_t)str[i];
    string_buf[0] = (uint16_t)((TUSB_DESC_STRING << 8) | (2 + len * 2));
    usb_packet_debug_note_control_in("desc-string", (uint8_t const *)string_buf,
                                     (uint16_t)(string_buf[0] & 0xFFu));
    return string_buf;
}

// -------- TinyUSB vendor-class glue (run mode only) --------------------

// XInput IN endpoint is interrupt-IN at 1 ms. The xinput module owns
// the per-tick send via tud_vendor_n_write / tud_vendor_n_write_flush.
//
// The OUT endpoint receives Microsoft rumble + LED-ring messages we
// don't currently act on. Drain them to keep TinyUSB happy.
void tud_vendor_rx_cb(uint8_t itf, uint8_t const *buffer, uint16_t bufsize) {
    (void)itf;
    usb_diag_note_xinput_out(buffer, bufsize);
    usb_packet_debug_note_out("vendor", buffer, bufsize);
    if (boot_mode_run_persona() == RUN_PERSONA_XBOXONE)
        xbone_on_out(buffer, bufsize);
    // Discarded: rumble (msg 0x00, len 8) and LED (msg 0x01, len 3).
    // Acked via the read.
    tud_vendor_read_flush();
}

void tud_vendor_tx_cb(uint8_t itf, uint32_t sent_bytes) {
    (void)itf;
    usb_diag_note_xinput_in_sent(sent_bytes);
    usb_packet_debug_note_in_accepted("xinput", sent_bytes);
}

// -------- HID glue (keyboard and HID gamepad personas) -----------------
//
// These callbacks are part of the HID class driver and are linked in for
// every persona (CFG_TUD_HID is always 1), but only fire while a HID
// configuration is active. IN report paths live in hid_kbd.c and
// dinput.c; here we satisfy the descriptor request and the host's
// control transfers, and fold report activity into the shared usb_diag
// counters so `couchlink test usb` reports HID traffic too.

uint8_t const *tud_hid_descriptor_report_cb(uint8_t instance) {
    (void)instance;
    if (boot_mode_run_persona() == RUN_PERSONA_PS3)
        return desc_hid_report_ps3;
    if (boot_mode_run_persona() == RUN_PERSONA_PS4)
        return desc_hid_report_ps4;
    if (boot_mode_run_persona() == RUN_PERSONA_GENERIC_HID)
        return desc_hid_report_generic_hid;
    return desc_hid_report_keyboard;
}

uint16_t tud_hid_get_report_cb(uint8_t instance, uint8_t report_id, hid_report_type_t report_type,
                               uint8_t *buffer, uint16_t reqlen) {
    usb_packet_debug_note_hid_get_report(instance, report_id, (uint8_t)report_type, reqlen);
    if (boot_mode_persona_uses_gamepad_hid(boot_mode_run_persona())) {
        return dinput_get_report_payload(report_id, report_type, buffer, reqlen);
    }
    // Keyboard input reports are delivered on the interrupt endpoint
    // from hid_kbd.c; unsupported control-pipe reads are stalled.
    return 0;
}

void tud_hid_set_report_cb(uint8_t instance, uint8_t report_id, hid_report_type_t report_type,
                           uint8_t const *buffer, uint16_t bufsize) {
    usb_packet_debug_note_hid_set_report(instance, report_id, (uint8_t)report_type, bufsize);
    // Keyboard LEDs and gamepad output reports are ignored, but noted
    // so OUT-activity diagnostics still light up.
    if (report_type == HID_REPORT_TYPE_OUTPUT && bufsize >= 1) {
        usb_diag_note_xinput_out(buffer, bufsize);
        usb_packet_debug_note_out_report("hid-output", report_id, (uint8_t)report_type, buffer,
                                         bufsize);
    }
    if (report_type == HID_REPORT_TYPE_FEATURE && bufsize >= 1) {
        usb_packet_debug_note_out_report("hid-feature", report_id, (uint8_t)report_type, buffer,
                                         bufsize);
    }
    if (boot_mode_persona_uses_gamepad_hid(boot_mode_run_persona())) {
        dinput_set_report(report_id, report_type, buffer, bufsize);
    }
}

void tud_hid_report_complete_cb(uint8_t instance, uint8_t const *report, uint16_t len) {
    (void)instance;
    (void)report;
    usb_diag_note_xinput_in_sent(len);
    usb_packet_debug_note_in_accepted("hid", len);
}

// -------- diag vendor-class glue (setup mode only) ---------------------
//
// In setup mode, interface 2 (DIAG_ITF_NUM) is a vendor-class interface
// with no bulk endpoints. Windows binds WinUSB to it automatically via
// the MS OS 2.0 descriptor set below. The host reads the firmware diag
// ring via a vendor IN control transfer on EP0 -- independent of the
// CDC bulk endpoint state, so it survives any CDC FIFO wedge. Wire
// format of the diag transfer matches the CDC CMD_GET_LOG_BUFFER
// response payload: [lost_bytes_le32][raw_log_bytes].

#define MS_OS_20_VENDOR_REQ_CODE 0x20
#define MS_OS_20_DESCRIPTOR_INDEX 0x0007
#define DIAG_GET_LOG_REQ 0x01
#define MS_OS_10_COMPAT_ID_INDEX 0x0004

#define MS_OS_20_DESC_SET_TOTAL_LEN 38

static const uint8_t desc_ms_os_20[MS_OS_20_DESC_SET_TOTAL_LEN] = {
    // Set header (10 bytes).
    0x0A,
    0x00, // wLength = 10
    0x00,
    0x00, // wDescriptorType = SET_HEADER
    0x00,
    0x00,
    0x03,
    0x06, // dwWindowsVersion = Windows 8.1+
    MS_OS_20_DESC_SET_TOTAL_LEN,
    0x00, // wTotalLength

    // Function subset header (8 bytes): scopes the rest of the set to
    // interface DIAG_ITF_NUM only, so usbser.sys's binding to the CDC
    // interfaces is unaffected.
    0x08,
    0x00, // wLength = 8
    0x02,
    0x00,         // wDescriptorType = SUBSET_HEADER_FUNCTION
    DIAG_ITF_NUM, // bFirstInterface
    0x00,         // bReserved
    0x14,
    0x00, // wSubsetLength = 8 + 20 = 28

    // Compatible ID feature descriptor (20 bytes): tells Windows to
    // bind WinUSB.
    0x14,
    0x00, // wLength = 20
    0x03,
    0x00, // wDescriptorType = FEATURE_COMPATIBLE_ID
    'W',
    'I',
    'N',
    'U',
    'S',
    'B',
    0x00,
    0x00,
    0x00,
    0x00,
    0x00,
    0x00,
    0x00,
    0x00,
    0x00,
    0x00,
};

typedef struct __attribute__((packed)) {
    uint32_t total_length;
    uint16_t version;
    uint16_t index;
    uint8_t total_sections;
    uint8_t reserved[7];
    uint8_t first_interface_number;
    uint8_t reserved2;
    uint8_t compatible_id[8];
    uint8_t sub_compatible_id[8];
    uint8_t reserved3[6];
} ms_os_10_compatible_id_single_t;

static const ms_os_10_compatible_id_single_t desc_xgip_compatible_id = {
    .total_length = sizeof(ms_os_10_compatible_id_single_t),
    .version = 0x0100,
    .index = MS_OS_10_COMPAT_ID_INDEX,
    .total_sections = 1,
    .reserved = {0},
    .first_interface_number = 0,
    .reserved2 = XGIP_NUM_INTERFACES_WITHOUT_AUDIO,
    .compatible_id = XGIP_COMPATIBLE_ID_BYTES,
    .sub_compatible_id = {0},
    .reserved3 = {0},
};

#define BOS_DESC_TOTAL_LEN (5 + 28)

static const uint8_t desc_bos[BOS_DESC_TOTAL_LEN] = {
    // BOS header (5 bytes).
    0x05, // bLength
    0x0F, // bDescriptorType = BOS
    BOS_DESC_TOTAL_LEN,
    0x00, // wTotalLength
    0x01, // bNumDeviceCaps

    // Platform device capability (28 bytes) advertising MS OS 2.0.
    0x1C, // bLength
    0x10, // bDescriptorType = DEVICE_CAPABILITY
    0x05, // bDevCapType = PLATFORM
    0x00, // bReserved
    // MS OS 2.0 platform-capability UUID: {D8DD60DF-4589-4CC7-9CD2-659D9E648A9F}
    // clang-format off
    0xDF, 0x60, 0xDD, 0xD8, 0x89, 0x45, 0xC7, 0x4C, 0x9C, 0xD2, 0x65, 0x9D, 0x9E, 0x64, 0x8A, 0x9F,
    0x00, 0x00, 0x03,
    // clang-format on
    0x06, // dwWindowsVersion = Windows 8.1+
    MS_OS_20_DESC_SET_TOTAL_LEN,
    0x00,                     // wMSOSDescriptorSetTotalLength
    MS_OS_20_VENDOR_REQ_CODE, // bMS_VendorCode
    0x00,                     // bAltEnumCode
};

uint8_t const *tud_descriptor_bos_cb(void) {
    // Only setup mode advertises WinUSB binding. XInput's binding to
    // xusb22.sys is sensitive to extra descriptors and capability
    // declarations; run mode skips BOS entirely.
    if (boot_mode_current() == BOOT_MODE_RUN)
        return NULL;
    return desc_bos;
}

// Static response buffer for GET_DIAG_LOG. Filled in the SETUP stage of
// the control transfer; TinyUSB chunks it into 64-byte EP0 DATA packets
// automatically up to req->wLength. Sized to hold [lost_le32] plus the
// full diag ring (16 KiB).
static uint8_t diag_xfer_buf[4 + DIAG_LOG_RING_SIZE];

bool tud_vendor_control_xfer_cb(uint8_t rhport, uint8_t stage, tusb_control_request_t const *req) {
    if (stage != CONTROL_STAGE_SETUP)
        return true;

    usb_packet_debug_note_setup("vendor-control", req->bmRequestType, req->bRequest, req->wValue,
                                req->wIndex, req->wLength);

    if (boot_mode_current() == BOOT_MODE_RUN && boot_mode_run_persona() == RUN_PERSONA_XBOXONE &&
        req->bmRequestType == 0xC0 && req->bRequest == XGIP_MS_VENDOR_REQ_CODE &&
        req->wIndex == MS_OS_10_COMPAT_ID_INDEX) {
        uint16_t want = req->wLength;
        if (want > sizeof(desc_xgip_compatible_id))
            want = sizeof(desc_xgip_compatible_id);
        usb_packet_debug_note_control_in("xgip-compat-id",
                                         (uint8_t const *)&desc_xgip_compatible_id, want);
        return tud_control_xfer(rhport, req, (void *)&desc_xgip_compatible_id, want);
    }

    // GET_MS_OS_20_DESCRIPTOR: bmRequestType=0xC0 (vendor IN, device),
    // bRequest=MS_OS_20_VENDOR_REQ_CODE, wIndex=7.
    if (req->bmRequestType == 0xC0 && req->bRequest == MS_OS_20_VENDOR_REQ_CODE &&
        req->wIndex == MS_OS_20_DESCRIPTOR_INDEX) {
        uint16_t want = req->wLength;
        if (want > sizeof(desc_ms_os_20))
            want = sizeof(desc_ms_os_20);
        usb_packet_debug_note_control_in("ms-os-20", desc_ms_os_20, want);
        return tud_control_xfer(rhport, req, (void *)desc_ms_os_20, want);
    }

    // GET_DIAG_LOG: bmRequestType=0xC1 (vendor IN, interface),
    // bRequest=DIAG_GET_LOG_REQ, wIndex.low == DIAG_ITF_NUM. The host
    // gets back [lost_le32][snapshot_of_diag_ring_tail].
    if (req->bmRequestType == 0xC1 && req->bRequest == DIAG_GET_LOG_REQ &&
        (req->wIndex & 0xFFu) == DIAG_ITF_NUM) {
        uint32_t lost = 0;
        size_t n = diag_log_snapshot(diag_xfer_buf + 4, sizeof(diag_xfer_buf) - 4, &lost);
        diag_xfer_buf[0] = (uint8_t)(lost & 0xFFu);
        diag_xfer_buf[1] = (uint8_t)((lost >> 8) & 0xFFu);
        diag_xfer_buf[2] = (uint8_t)((lost >> 16) & 0xFFu);
        diag_xfer_buf[3] = (uint8_t)((lost >> 24) & 0xFFu);
        uint16_t avail = (uint16_t)(4 + n);
        uint16_t want = req->wLength;
        if (want > avail)
            want = avail;
        usb_packet_debug_note_control_in("setup-diag-log", diag_xfer_buf, want);
        return tud_control_xfer(rhport, req, diag_xfer_buf, want);
    }

    return false;
}

// -------- USB lifecycle diagnostics ------------------------------------
//
// These weak callbacks fire when the host transitions us through the
// USB device states. Together with the descriptor-request logs above
// they let an operator triage enumeration failures by reading the
// diag log: "device descriptor" without "mounted" means the host gave
// up between SET_ADDRESS and SET_CONFIGURATION; no "device descriptor"
// at all means the host never started talking to us (cable, port, or
// firmware-didn't-boot).

void tud_mount_cb(void) {
    usb_diag_note_mount();
    usb_packet_debug_note_event("mount", "");
    xinput_note_usb_reset();
    hid_kbd_note_usb_reset();
    dinput_note_usb_reset();
    xbone_note_usb_reset();
    diag_log_msg("usb_init: tud_mount_cb -- enumeration complete");
}

void tud_umount_cb(void) {
    usb_diag_note_umount();
    usb_packet_debug_note_event("unmount", "");
    xinput_note_usb_reset();
    hid_kbd_note_usb_reset();
    dinput_note_usb_reset();
    xbone_note_usb_reset();
    diag_log_msg("usb: unmounted (host disconnected or bus reset)");
}

void tud_suspend_cb(bool remote_wakeup_en) {
    usb_diag_note_suspend();
    char fields[24];
    snprintf(fields, sizeof(fields), "remote_wakeup=%u", remote_wakeup_en ? 1u : 0u);
    usb_packet_debug_note_event("suspend", fields);
    diag_log_printf("usb: suspended (remote_wakeup=%d)", (int)remote_wakeup_en);
}

void tud_resume_cb(void) {
    usb_diag_note_resume();
    usb_packet_debug_note_event("resume", "");
    diag_log_msg("usb: resumed");
}

// Fires whenever the host changes DTR or RTS via SET_CONTROL_LINE_STATE.
// In our setup-mode handshake, the bridge explicitly asserts DTR+RTS
// right after opening the COM port, so a healthy bundle will show this
// callback firing within milliseconds of "usb: mounted". If a bundle
// shows mount-without-line-state, the host opened the port but never
// drove DTR -- check the bridge logs for an "asserted DTR" line.
void tud_cdc_line_state_cb(uint8_t itf, bool dtr, bool rts) {
    diag_log_printf("cdc: line state itf=%u dtr=%d rts=%d", (unsigned)itf, (int)dtr, (int)rts);
}

// Fires whenever the host changes line coding (baud, parity, etc.).
// Logged once so we can see whether the host did the full open sequence.
void tud_cdc_line_coding_cb(uint8_t itf, cdc_line_coding_t const *coding) {
    static bool logged = false;
    if (!logged) {
        diag_log_printf("cdc: line coding itf=%u baud=%u (logged once per boot)", (unsigned)itf,
                        (unsigned)coding->bit_rate);
        logged = true;
    }
}
