// USB descriptors for both Pico personas:
//
//   setup mode: CDC ACM + WinUSB diag (Raspberry Pi VID 0x2E8A, PID 0xCAF0)
//   run mode:   wired Xbox 360 / XUSB (Microsoft VID 0x045E, PID 0x028E)
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
    static volatile bool logged = false;
    if (!logged) {
        logged = true;
        diag_log_msg("usb_init: first GET_DESCRIPTOR(DEVICE) reply sent");
    }
    return (uint8_t const *)(boot_mode_current() == BOOT_MODE_RUN
                             ? &desc_device_xinput
                             : &desc_device_cdc);
}

// -------- setup mode: CDC + WinUSB diag configuration ------------------

enum { CDC_ITF_NUM_NOTIF = 0, CDC_ITF_NUM_DATA, DIAG_ITF_NUM, CDC_ITF_COUNT };

#define DIAG_VENDOR_DESC_LEN  9  // interface descriptor only, no endpoints
#define CDC_CONFIG_TOTAL_LEN  (TUD_CONFIG_DESC_LEN + TUD_CDC_DESC_LEN + DIAG_VENDOR_DESC_LEN)

#define CDC_NOTIF_EP_ADDR  0x82
#define CDC_OUT_EP_ADDR    0x03
#define CDC_IN_EP_ADDR     0x83

static const uint8_t desc_configuration_cdc[] = {
    TUD_CONFIG_DESCRIPTOR(1, CDC_ITF_COUNT, 0, CDC_CONFIG_TOTAL_LEN, 0xA0, 100),
    TUD_CDC_DESCRIPTOR(CDC_ITF_NUM_NOTIF, 4,
                       CDC_NOTIF_EP_ADDR, 8,
                       CDC_OUT_EP_ADDR, CDC_IN_EP_ADDR, 64),

    // Interface 2: diag vendor interface. Class 0xFF, no endpoints --
    // the diag log is read via a vendor control transfer on EP0 (see
    // tud_vendor_control_xfer_cb below). Bound to WinUSB on Windows via
    // the MS OS 2.0 descriptor set (see further down).
    9, TUSB_DESC_INTERFACE,
    DIAG_ITF_NUM, 0, 0,
    0xFF, 0x00, 0x00,
    5,  // iInterface = STRID_DIAG_INTERFACE (see string enum below)
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
    STRID_DIAG_INTERFACE,
};

static char serial_str[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES + 1];

static const char *const string_desc_arr[] = {
    [STRID_LANGID]         = (const char[]){0x09, 0x04}, // English (US)
    [STRID_MANUFACTURER]   = "Parsec CouchLink",
    [STRID_PRODUCT]        = "CouchLink Pico",
    [STRID_SERIAL]         = serial_str,
    [STRID_CDC_INTERFACE]  = "Parsec CouchLink Setup",
    [STRID_DIAG_INTERFACE] = "Parsec CouchLink Diag",
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

// -------- diag vendor-class glue (setup mode only) ---------------------
//
// In setup mode, interface 2 (DIAG_ITF_NUM) is a vendor-class interface
// with no bulk endpoints. Windows binds WinUSB to it automatically via
// the MS OS 2.0 descriptor set below. The host reads the firmware diag
// ring via a vendor IN control transfer on EP0 -- independent of the
// CDC bulk endpoint state, so it survives any CDC FIFO wedge. Wire
// format of the diag transfer matches the CDC CMD_GET_LOG_BUFFER
// response payload: [lost_bytes_le32][raw_log_bytes].

#define MS_OS_20_VENDOR_REQ_CODE  0x20
#define MS_OS_20_DESCRIPTOR_INDEX 0x0007
#define DIAG_GET_LOG_REQ          0x01

#define MS_OS_20_DESC_SET_TOTAL_LEN  38

static const uint8_t desc_ms_os_20[MS_OS_20_DESC_SET_TOTAL_LEN] = {
    // Set header (10 bytes).
    0x0A, 0x00,                          // wLength = 10
    0x00, 0x00,                          // wDescriptorType = SET_HEADER
    0x00, 0x00, 0x03, 0x06,              // dwWindowsVersion = Windows 8.1+
    MS_OS_20_DESC_SET_TOTAL_LEN, 0x00,   // wTotalLength

    // Function subset header (8 bytes): scopes the rest of the set to
    // interface DIAG_ITF_NUM only, so usbser.sys's binding to the CDC
    // interfaces is unaffected.
    0x08, 0x00,                          // wLength = 8
    0x02, 0x00,                          // wDescriptorType = SUBSET_HEADER_FUNCTION
    DIAG_ITF_NUM,                        // bFirstInterface
    0x00,                                // bReserved
    0x14, 0x00,                          // wSubsetLength = 8 + 20 = 28

    // Compatible ID feature descriptor (20 bytes): tells Windows to
    // bind WinUSB.
    0x14, 0x00,                          // wLength = 20
    0x03, 0x00,                          // wDescriptorType = FEATURE_COMPATIBLE_ID
    'W', 'I', 'N', 'U', 'S', 'B', 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
};

#define BOS_DESC_TOTAL_LEN  (5 + 28)

static const uint8_t desc_bos[BOS_DESC_TOTAL_LEN] = {
    // BOS header (5 bytes).
    0x05,                                // bLength
    0x0F,                                // bDescriptorType = BOS
    BOS_DESC_TOTAL_LEN, 0x00,            // wTotalLength
    0x01,                                // bNumDeviceCaps

    // Platform device capability (28 bytes) advertising MS OS 2.0.
    0x1C,                                // bLength
    0x10,                                // bDescriptorType = DEVICE_CAPABILITY
    0x05,                                // bDevCapType = PLATFORM
    0x00,                                // bReserved
    // MS OS 2.0 platform-capability UUID: {D8DD60DF-4589-4CC7-9CD2-659D9E648A9F}
    0xDF, 0x60, 0xDD, 0xD8,
    0x89, 0x45, 0xC7, 0x4C,
    0x9C, 0xD2,
    0x65, 0x9D, 0x9E, 0x64, 0x8A, 0x9F,
    0x00, 0x00, 0x03, 0x06,              // dwWindowsVersion = Windows 8.1+
    MS_OS_20_DESC_SET_TOTAL_LEN, 0x00,   // wMSOSDescriptorSetTotalLength
    MS_OS_20_VENDOR_REQ_CODE,            // bMS_VendorCode
    0x00,                                // bAltEnumCode
};

uint8_t const *tud_descriptor_bos_cb(void) {
    // Only setup mode advertises WinUSB binding. XInput's binding to
    // xusb22.sys is sensitive to extra descriptors and capability
    // declarations; run mode skips BOS entirely.
    if (boot_mode_current() == BOOT_MODE_RUN) return NULL;
    return desc_bos;
}

// Static response buffer for GET_DIAG_LOG. Filled in the SETUP stage of
// the control transfer; TinyUSB chunks it into 64-byte EP0 DATA packets
// automatically up to req->wLength. Sized to hold [lost_le32] plus the
// full diag ring (4 KiB).
static uint8_t diag_xfer_buf[4 + 4096];

bool tud_vendor_control_xfer_cb(uint8_t rhport, uint8_t stage,
                                tusb_control_request_t const *req) {
    if (stage != CONTROL_STAGE_SETUP) return true;

    // GET_MS_OS_20_DESCRIPTOR: bmRequestType=0xC0 (vendor IN, device),
    // bRequest=MS_OS_20_VENDOR_REQ_CODE, wIndex=7.
    if (req->bmRequestType == 0xC0
        && req->bRequest == MS_OS_20_VENDOR_REQ_CODE
        && req->wIndex == MS_OS_20_DESCRIPTOR_INDEX) {
        uint16_t want = req->wLength;
        if (want > sizeof(desc_ms_os_20)) want = sizeof(desc_ms_os_20);
        return tud_control_xfer(rhport, req, (void *)desc_ms_os_20, want);
    }

    // GET_DIAG_LOG: bmRequestType=0xC1 (vendor IN, interface),
    // bRequest=DIAG_GET_LOG_REQ, wIndex.low == DIAG_ITF_NUM. The host
    // gets back [lost_le32][snapshot_of_diag_ring_tail].
    if (req->bmRequestType == 0xC1
        && req->bRequest == DIAG_GET_LOG_REQ
        && (req->wIndex & 0xFFu) == DIAG_ITF_NUM) {
        uint32_t lost = 0;
        size_t n = diag_log_snapshot(diag_xfer_buf + 4,
                                     sizeof(diag_xfer_buf) - 4, &lost);
        diag_xfer_buf[0] = (uint8_t)(lost & 0xFFu);
        diag_xfer_buf[1] = (uint8_t)((lost >>  8) & 0xFFu);
        diag_xfer_buf[2] = (uint8_t)((lost >> 16) & 0xFFu);
        diag_xfer_buf[3] = (uint8_t)((lost >> 24) & 0xFFu);
        uint16_t avail = (uint16_t)(4 + n);
        uint16_t want = req->wLength;
        if (want > avail) want = avail;
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
    diag_log_msg("usb_init: tud_mount_cb -- enumeration complete");
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
