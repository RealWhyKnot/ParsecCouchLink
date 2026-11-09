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

#define BCD_DEVICE_VERSION PICO_BRIDGE_USB_BCD_DEVICE

static const tusb_desc_device_t desc_device_cdc = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x2E8A,
    .idProduct = 0xCAF0,
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
    .bDeviceClass = 0xFF,
    .bDeviceSubClass = 0xFF,
    .bDeviceProtocol = 0xFF,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x045E,
    .idProduct = 0x028E,
    .bcdDevice = 0x0114,

    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_keyboard = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor = 0x2E8A,
    .idProduct = 0xCAF1,
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

    .idVendor = 0x2E8A,
    .idProduct = 0xCAF2,
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

    .idVendor = 0x054C,
    .idProduct = 0x0268,
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

    .idVendor = 0x054C,
    .idProduct = 0x09CC,
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

    .idVendor = 0x0E6F,
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
    if (boot_mode_current() != BOOT_MODE_RUN ||
        boot_mode_persona_uses_bluetooth(boot_mode_run_persona())) {
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

enum { CDC_ITF_NUM_NOTIF = 0, CDC_ITF_NUM_DATA, DIAG_ITF_NUM, CDC_ITF_COUNT };

#define DIAG_VENDOR_DESC_LEN 9
#define CDC_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_CDC_DESC_LEN + DIAG_VENDOR_DESC_LEN)

#define CDC_NOTIF_EP_ADDR 0x82
#define CDC_OUT_EP_ADDR 0x03
#define CDC_IN_EP_ADDR 0x83

static const uint8_t desc_configuration_cdc[] = {
    TUD_CONFIG_DESCRIPTOR(1, CDC_ITF_COUNT, 0, CDC_CONFIG_TOTAL_LEN, 0xA0, 100),
    TUD_CDC_DESCRIPTOR(CDC_ITF_NUM_NOTIF, 4, CDC_NOTIF_EP_ADDR, 8, CDC_OUT_EP_ADDR, CDC_IN_EP_ADDR,
                       64),

    9,
    TUSB_DESC_INTERFACE,
    DIAG_ITF_NUM,
    0,
    0,
    0xFF,
    0x00,
    0x00,
    5,
};

#define XINPUT_IN_EP_ADDR 0x81
#define XINPUT_OUT_EP_ADDR 0x02
#define XINPUT_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + 9 + 17 + 7 + 7)

static const uint8_t desc_configuration_xinput[] = {
    9,
    TUSB_DESC_CONFIGURATION,
    U16_TO_U8S_LE(XINPUT_CONFIG_TOTAL_LEN),
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
    0x5D,
    0x01,
    0,

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

    7,
    TUSB_DESC_ENDPOINT,
    XINPUT_IN_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(20),
    1,

    7,
    TUSB_DESC_ENDPOINT,
    XINPUT_OUT_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(32),
    8,
};

#define KBD_ITF_NUM 0
#define KBD_IN_EP_ADDR 0x81

static const uint8_t desc_hid_report_keyboard[] = {TUD_HID_REPORT_DESC_KEYBOARD()};

#define KBD_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN)

static const uint8_t desc_configuration_keyboard[] = {
    TUD_CONFIG_DESCRIPTOR(1, 1, 0, KBD_CONFIG_TOTAL_LEN, 0xA0, 100),
    TUD_HID_DESCRIPTOR(KBD_ITF_NUM, 0, HID_ITF_PROTOCOL_KEYBOARD, sizeof(desc_hid_report_keyboard),
                       KBD_IN_EP_ADDR, CFG_TUD_HID_EP_BUFSIZE, 10),
};

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

static const uint8_t desc_hid_report_generic_hid[] = {
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0C, 0x15, 0x00,
    0x25, 0x01, 0x75, 0x01, 0x95, 0x0C, 0x81, 0x02, 0x75, 0x01, 0x95, 0x04, 0x81, 0x03,
    0x05, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x06, 0x09, 0x30, 0x09,
    0x31, 0x09, 0x33, 0x09, 0x34, 0x09, 0x32, 0x09, 0x35, 0x81, 0x02, 0xC0,
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
    if (boot_mode_current() != BOOT_MODE_RUN ||
        boot_mode_persona_uses_bluetooth(boot_mode_run_persona())) {
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
    [STRID_LANGID] = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "Parsec CouchLink",
    [STRID_PRODUCT] = "CouchLink Pico",
    [STRID_SERIAL] = serial_str,
    [STRID_CDC_INTERFACE] = "Parsec CouchLink Setup",
    [STRID_DIAG_INTERFACE] = "Parsec CouchLink Diag",
};

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

static uint16_t string_buf[127];

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid) {
    (void)langid;

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

void tud_vendor_rx_cb(uint8_t itf, uint8_t const *buffer, uint16_t bufsize) {
    (void)itf;
    usb_diag_note_xinput_out(buffer, bufsize);
    usb_packet_debug_note_out("vendor", buffer, bufsize);
    if (boot_mode_run_persona() == RUN_PERSONA_XBOXONE)
        xbone_on_out(buffer, bufsize);
    tud_vendor_read_flush();
}

void tud_vendor_tx_cb(uint8_t itf, uint32_t sent_bytes) {
    (void)itf;
    usb_diag_note_xinput_in_sent(sent_bytes);
    usb_packet_debug_note_in_accepted("xinput", sent_bytes);
}

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
    return 0;
}

void tud_hid_set_report_cb(uint8_t instance, uint8_t report_id, hid_report_type_t report_type,
                           uint8_t const *buffer, uint16_t bufsize) {
    usb_packet_debug_note_hid_set_report(instance, report_id, (uint8_t)report_type, bufsize);
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

#define MS_OS_20_VENDOR_REQ_CODE 0x20
#define MS_OS_20_DESCRIPTOR_INDEX 0x0007
#define DIAG_GET_LOG_REQ 0x01
#define MS_OS_10_COMPAT_ID_INDEX 0x0004

#define MS_OS_20_DESC_SET_TOTAL_LEN 38

static const uint8_t desc_ms_os_20[MS_OS_20_DESC_SET_TOTAL_LEN] = {
    0x0A, 0x00, 0x00, 0x00, 0x00,         0x00, 0x03, 0x06, MS_OS_20_DESC_SET_TOTAL_LEN,
    0x00,

    0x08, 0x00, 0x02, 0x00, DIAG_ITF_NUM, 0x00, 0x14, 0x00,

    0x14, 0x00, 0x03, 0x00, 'W',          'I',  'N',  'U',  'S',
    'B',  0x00, 0x00, 0x00, 0x00,         0x00, 0x00, 0x00, 0x00,
    0x00, 0x00,
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
    0x05,
    0x0F,
    BOS_DESC_TOTAL_LEN,
    0x00,
    0x01,

    0x1C,
    0x10,
    0x05,
    0x00,
    0xDF,
    0x60,
    0xDD,
    0xD8,
    0x89,
    0x45,
    0xC7,
    0x4C,
    0x9C,
    0xD2,
    0x65,
    0x9D,
    0x9E,
    0x64,
    0x8A,
    0x9F,
    0x00,
    0x00,
    0x03,
    0x06,
    MS_OS_20_DESC_SET_TOTAL_LEN,
    0x00,
    MS_OS_20_VENDOR_REQ_CODE,
    0x00,
};

uint8_t const *tud_descriptor_bos_cb(void) {
    if (boot_mode_current() == BOOT_MODE_RUN &&
        !boot_mode_persona_uses_bluetooth(boot_mode_run_persona()))
        return NULL;
    return desc_bos;
}

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

    if (req->bmRequestType == 0xC0 && req->bRequest == MS_OS_20_VENDOR_REQ_CODE &&
        req->wIndex == MS_OS_20_DESCRIPTOR_INDEX) {
        uint16_t want = req->wLength;
        if (want > sizeof(desc_ms_os_20))
            want = sizeof(desc_ms_os_20);
        usb_packet_debug_note_control_in("ms-os-20", desc_ms_os_20, want);
        return tud_control_xfer(rhport, req, (void *)desc_ms_os_20, want);
    }

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

void tud_cdc_line_state_cb(uint8_t itf, bool dtr, bool rts) {
    diag_log_printf("cdc: line state itf=%u dtr=%d rts=%d", (unsigned)itf, (int)dtr, (int)rts);
}

void tud_cdc_line_coding_cb(uint8_t itf, cdc_line_coding_t const *coding) {
    static bool logged = false;
    if (!logged) {
        diag_log_printf("cdc: line coding itf=%u baud=%u (logged once per boot)", (unsigned)itf,
                        (unsigned)coding->bit_rate);
        logged = true;
    }
}
