#pragma once

#include <stdbool.h>
#include <stdint.h>

// Credential record stored in flash. Two A/B slots in the last two
// sectors of flash. Each write picks the slot with the older generation
// counter, writes there, verifies CRC, then on success the new slot is
// "current" because it has the newer generation.
//
// The on-flash layout is 256 bytes per slot (one programming page). The
// rest of the 4 KB sector is left erased so we can re-write without an
// extra erase.

#define FLASH_CREDS_MAGIC 0xC0DEC0DE
#define FLASH_CREDS_VERSION 1
#define FLASH_CREDS_SSID_MAX 32
#define FLASH_CREDS_PASS_MAX 63

#define FLASH_CREDS_RECORD_SIZE 256
#define FLASH_CREDS_NAME_MAX 32

// Run-mode output persona stored alongside the credentials. The byte sits
// where the pad used to be all-zero, so a record written before this
// field existed reads back as the XInput persona with its CRC still
// valid -- no version bump or migration needed.
#define FLASH_PERSONA_XINPUT 0      // wired Xbox 360 / XInput (default)
#define FLASH_PERSONA_KEYBOARD 1    // USB HID boot keyboard
#define FLASH_PERSONA_MAPLE 2       // Xbox-compatible Dreamcast Maple adapter mode
#define FLASH_PERSONA_PS3 3         // Sony DualShock 3 / PS3 HID gamepad
#define FLASH_PERSONA_PS4 4         // Sony DualShock 4 / PS4 HID gamepad
#define FLASH_PERSONA_XBOXONE 5     // Xbox One-compatible XGIP gamepad
#define FLASH_PERSONA_DEBUG 6       // XInput shape with raw USB packet capture
#define FLASH_PERSONA_GENERIC_HID 7 // Generic HID gamepad for unknown-HID adapters

typedef struct __attribute__((packed)) {
    uint32_t magic;  // FLASH_CREDS_MAGIC
    uint8_t version; // FLASH_CREDS_VERSION
    uint8_t ssid_len;
    uint8_t pass_len;
    uint8_t name_len;
    uint32_t generation;                                      // monotonic; newer slot wins
    uint8_t ssid[FLASH_CREDS_SSID_MAX];                       // 32
    uint8_t password[FLASH_CREDS_PASS_MAX + 1];               // 64 with NUL room
    uint8_t device_name[FLASH_CREDS_NAME_MAX];                // 32
    uint8_t usb_persona;                                      // FLASH_PERSONA_*; 0 = XInput
    uint8_t reserved[256 - 4 - 4 - 4 - 32 - 64 - 32 - 1 - 4]; // pad
    uint32_t crc32;                                           // over the first 252 bytes
} flash_creds_t;

_Static_assert(sizeof(flash_creds_t) == FLASH_CREDS_RECORD_SIZE,
               "flash_creds_t must be exactly 256 bytes");

// Load the currently-active credential record. Returns true if a valid
// record was found (magic, version, CRC, generation ordering all OK).
bool flash_creds_load(flash_creds_t *out);

// Store a new credential record. Picks the older slot, writes there,
// verifies. Returns 0 on success, negative on failure.
//   -1 = bad ssid_len/pass_len
//   -2 = flash program failed
//   -3 = post-write verify (re-read + CRC) failed
int flash_creds_store(const flash_creds_t *rec);

// Erase both slots. Kept for explicit recovery commands or future
// maintenance paths; normal BOOTSEL setup recovery retains credentials.
void flash_creds_clear(void);
