// USB descriptors for both Pico personas:
//
//   setup mode: CDC ACM (Raspberry Pi VID 0x2E8A, PID 0xCAF0)
//   run mode:   wired Xbox 360 / XUSB (Microsoft VID 0x045E, PID 0x028E)
//
// Only one persona is presented at a time. `g_boot_mode` is set BEFORE
// TinyUSB initialises, so by the time the host enumerates and the
// callbacks below fire, the choice is already locked.
//
// The XInput descriptor + magic 17-byte unknown descriptor are lifted
// from Ryzee119/tusb_xinput (MIT). The 17-byte unknown descriptor is
// required for Windows xusb22.sys to accept the device as a wired Xbox
// 360 controller.

#include <stdint.h>
#include <string.h>

#include "tusb.h"
#include "pico/unique_id.h"

#include "boot_mode.h"
#include "diag_log.h"
#include "version.h"

// bcdDevice is keyed by Windows usbflags / driver-binding cache. Bumping
// it on a CDC-protocol break forces Windows to re-bind usbser.sys after
// a re-flash, sidestepping cached bindings from an older firmware that
// exposed a different interface layout. The firmware-side semver macros
// only bump on protocol breaks by definition, so deriving bcdDevice
// from them gets the right invalidation for free.
#define BCD8(n)  ((((n) / 10) << 4) | ((n) % 10))
#define BCD_DEVICE_VERSION \
    (((uint16_t)BCD8(PICO_BRIDGE_FW_MAJOR) << 8) | (uint16_t)BCD8(PICO_BRIDGE_FW_MINOR))

// -------- common: device descriptors -----------------------------------

static const tusb_desc_device_t desc_device_cdc = {
    .bLength            = sizeof(tusb_desc_device_t),
    .bDescriptorType    = TUSB_DESC_DEVICE,
    .bcdUSB             = 0x0200,
    .bDeviceClass       = TUSB_CLASS_MISC,
    .bDeviceSubClass    = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol    = MISC_PROTOCOL_IAD,
    .bMaxPacketSize0    = CFG_TUD_ENDPOINT0_SIZE,

    .idVendor           = 0x2E8A,  // Raspberry Pi
    .idProduct          = 0xCAF0,  // sub-licensed; see raspberrypi/usb-pid
    .bcdDevice          = BCD_DEVICE_VERSION,

    .iManufacturer      = 0x01,
    .iProduct           = 0x02,
    .iSerialNumber      = 0x03,
    .bNumConfigurations = 0x01,
};

static const tusb_desc_device_t desc_device_xinput = {
    .bLength            = sizeof(tusb_desc_device_t),
    .bDescriptorType    = TUSB_DESC_DEVICE,
    .bcdUSB             = 0x0200,
    .bDeviceClass       = 0xFF,    // vendor-specific
    .bDeviceSubClass    = 0xFF,
    .bDeviceProtocol    = 0xFF,
    .bMaxPacketSize0    = 8,       // wired Xbox 360 reports 8, not 64

    .idVendor           = 0x045E,  // Microsoft
    .idProduct          = 0x028E,  // wired Xbox 360 controller
    .bcdDevice          = 0x0114,

    .iManufacturer      = 0x01,
    .iProduct           = 0x02,
    .iSerialNumber      = 0x03,
    .bNumConfigurations = 0x01,
};

uint8_t const *tud_descriptor_device_cb(void) {
    static bool logged = false;
    if (!logged) {
        diag_log_msg("usb: host requested device descriptor (enum step 1)");
        logged = true;
    }
    return (uint8_t const *)(boot_mode_current() == BOOT_MODE_RUN
                             ? &desc_device_xinput
                             : &desc_device_cdc);
}

// -------- setup mode: CDC configuration --------------------------------

enum { CDC_ITF_NUM_NOTIF = 0, CDC_ITF_NUM_DATA, CDC_ITF_COUNT };

#define CDC_CONFIG_TOTAL_LEN  (TUD_CONFIG_DESC_LEN + TUD_CDC_DESC_LEN)

#define CDC_NOTIF_EP_ADDR  0x82
#define CDC_OUT_EP_ADDR    0x03
#define CDC_IN_EP_ADDR     0x83

static const uint8_t desc_configuration_cdc[] = {
    TUD_CONFIG_DESCRIPTOR(1, CDC_ITF_COUNT, 0, CDC_CONFIG_TOTAL_LEN, 0xA0, 100),
    TUD_CDC_DESCRIPTOR(CDC_ITF_NUM_NOTIF, 4,
                       CDC_NOTIF_EP_ADDR, 8,
                       CDC_OUT_EP_ADDR, CDC_IN_EP_ADDR, 64),
};

// -------- run mode: XInput configuration --------------------------------

#define XINPUT_IN_EP_ADDR   0x81
#define XINPUT_OUT_EP_ADDR  0x02
#define XINPUT_CONFIG_TOTAL_LEN  (TUD_CONFIG_DESC_LEN + 9 /*interface*/ + 17 /*magic*/ + 7 + 7)

static const uint8_t desc_configuration_xinput[] = {
    // Configuration descriptor
    9, TUSB_DESC_CONFIGURATION,
    U16_TO_U8S_LE(XINPUT_CONFIG_TOTAL_LEN),
    1, 1, 0, 0xA0, 250,   // bmAttributes=bus-powered, bMaxPower=500mA

    // Interface 0: vendor-specific, class 0xFF, subclass 0x5D, protocol 0x01
    9, TUSB_DESC_INTERFACE,
    0, 0, 2,              // bInterfaceNumber=0, bAlternateSetting=0, bNumEndpoints=2
    0xFF, 0x5D, 0x01,
    0,                    // iInterface

    // Magic 17-byte unknown descriptor; required by xusb22.sys.
    0x11, 0x21, 0x10, 0x01, 0x01, 0x24, 0x81, 0x14,
    0x03, 0x00, 0x03, 0x13, 0x02, 0x00, 0x03, 0x00,
    0x00,

    // Endpoint 0x81 IN, interrupt, 20 bytes, 1 ms (bInterval=4 at FS = 2^(4-1)? no -- FS uses ms directly)
    7, TUSB_DESC_ENDPOINT, XINPUT_IN_EP_ADDR,
    0x03,                 // bmAttributes = Interrupt
    U16_TO_U8S_LE(20),
    4,                    // bInterval (1 ms at full-speed via Microsoft's choice; matches the real wired 360 pad)

    // Endpoint 0x02 OUT, interrupt, 32 bytes, 8 ms (rumble + LED ring writes from the host)
    7, TUSB_DESC_ENDPOINT, XINPUT_OUT_EP_ADDR,
    0x03,
    U16_TO_U8S_LE(32),
    8,
};

uint8_t const *tud_descriptor_configuration_cb(uint8_t index) {
    (void)index;
    static bool logged = false;
    if (!logged) {
        diag_log_msg("usb: host requested configuration descriptor (enum step 2)");
        logged = true;
    }
    if (boot_mode_current() == BOOT_MODE_RUN) {
        return desc_configuration_xinput;
    }
    return desc_configuration_cdc;
}

// -------- string descriptors --------------------------------------------

enum {
    STRID_LANGID = 0,
    STRID_MANUFACTURER,
    STRID_PRODUCT,
    STRID_SERIAL,
    STRID_CDC_INTERFACE,
};

static char serial_str[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES + 1];

static const char *const string_desc_arr[] = {
    [STRID_LANGID]        = (const char[]){0x09, 0x04}, // English (US)
    [STRID_MANUFACTURER]  = "Parsec CouchLink",
    [STRID_PRODUCT]       = "CouchLink Pico",
    [STRID_SERIAL]        = serial_str,
    [STRID_CDC_INTERFACE] = "Parsec CouchLink Setup",
};

// XInput wants Microsoft-y strings so xusb22.sys binds without
// complaining. Real wired 360 pads report these (or close enough).
static const char *const string_desc_arr_xinput[] = {
    [STRID_LANGID]       = (const char[]){0x09, 0x04},
    [STRID_MANUFACTURER] = "(c) Microsoft Corporation",
    [STRID_PRODUCT]      = "Controller",
    [STRID_SERIAL]       = serial_str,
};

static uint16_t string_buf[33]; // descriptor type + length prefix + up to 31 chars

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid) {
    (void)langid;

    // Lazily fill the serial string from the SoC unique ID.
    if (serial_str[0] == 0) {
        pico_unique_board_id_t id;
        pico_get_unique_board_id(&id);
        for (int i = 0; i < PICO_UNIQUE_BOARD_ID_SIZE_BYTES; i++) {
            static const char hex[] = "0123456789ABCDEF";
            serial_str[i*2 + 0] = hex[(id.id[i] >> 4) & 0xF];
            serial_str[i*2 + 1] = hex[id.id[i] & 0xF];
        }
        serial_str[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES] = 0;
    }

    bool xinput = (boot_mode_current() == BOOT_MODE_RUN);
    const char *const *arr = xinput ? string_desc_arr_xinput : string_desc_arr;
    size_t arr_count = xinput
        ? (sizeof(string_desc_arr_xinput) / sizeof(string_desc_arr_xinput[0]))
        : (sizeof(string_desc_arr) / sizeof(string_desc_arr[0]));

    if (index >= arr_count) return NULL;

    if (index == STRID_LANGID) {
        // LANGID descriptor is exactly 4 bytes: length(1)+type(1)+langid(2).
        string_buf[0] = (TUSB_DESC_STRING << 8) | 4;
        memcpy(&string_buf[1], arr[0], 2);
        return string_buf;
    }

    const char *str = arr[index];
    if (!str) return NULL;
    size_t len = strlen(str);
    if (len > 31) len = 31;
    for (size_t i = 0; i < len; i++) string_buf[1 + i] = (uint16_t)str[i];
    string_buf[0] = (uint16_t)((TUSB_DESC_STRING << 8) | (2 + len * 2));
    return string_buf;
}

// -------- TinyUSB vendor-class glue (run mode only) --------------------

// XInput IN endpoint is interrupt-IN at 1 ms. The xinput module owns
// the per-tick send via tud_vendor_n_write / tud_vendor_n_write_flush.
//
// The OUT endpoint receives Microsoft rumble + LED-ring messages we
// don't currently act on. Drain them to keep TinyUSB happy.
void tud_vendor_rx_cb(uint8_t itf, uint8_t const *buffer, uint16_t bufsize) {
    (void)itf; (void)buffer; (void)bufsize;
    // Discarded: rumble (msg 0x00, len 8) and LED (msg 0x01, len 3).
    // Acked via the read.
    tud_vendor_read_flush();
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
    diag_log_msg("usb: mounted (enumeration complete)");
}

void tud_umount_cb(void) {
    diag_log_msg("usb: unmounted (host disconnected or bus reset)");
}

void tud_suspend_cb(bool remote_wakeup_en) {
    diag_log_printf("usb: suspended (remote_wakeup=%d)", (int)remote_wakeup_en);
}

void tud_resume_cb(void) {
    diag_log_msg("usb: resumed");
}

// Fires whenever the host changes DTR or RTS via SET_CONTROL_LINE_STATE.
// In our setup-mode handshake, the bridge explicitly asserts DTR+RTS
// right after opening the COM port, so a healthy bundle will show this
// callback firing within milliseconds of "usb: mounted". If a bundle
// shows mount-without-line-state, the host opened the port but never
// drove DTR -- check the bridge logs for an "asserted DTR" line.
void tud_cdc_line_state_cb(uint8_t itf, bool dtr, bool rts) {
    diag_log_printf("cdc: line state itf=%u dtr=%d rts=%d",
                    (unsigned)itf, (int)dtr, (int)rts);
}

// Fires whenever the host changes line coding (baud, parity, etc.).
// Logged once so we can see whether the host did the full open sequence.
void tud_cdc_line_coding_cb(uint8_t itf, cdc_line_coding_t const *coding) {
    static bool logged = false;
    if (!logged) {
        diag_log_printf("cdc: line coding itf=%u baud=%u (logged once per boot)",
                        (unsigned)itf, (unsigned)coding->bit_rate);
        logged = true;
    }
}
