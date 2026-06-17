#pragma once

#define XGIP_DEVICE_CLASS 0xFF
#define XGIP_DEVICE_SUBCLASS 0x47
#define XGIP_DEVICE_PROTOCOL 0xD0

#define XGIP_MS_OS_STRING_INDEX 0xEE
#define XGIP_MS_VENDOR_REQ_CODE 0x90

// clang-format off
#define XGIP_COMPATIBLE_ID_BYTES {'X', 'G', 'I', 'P', '1', '0', 0, 0}
// clang-format on
#define XGIP_NUM_INTERFACES_WITHOUT_AUDIO 0x01
