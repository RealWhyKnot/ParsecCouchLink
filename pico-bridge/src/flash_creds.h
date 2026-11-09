#pragma once

#include <stdbool.h>
#include <stdint.h>

#define FLASH_CREDS_MAGIC 0xC0DEC0DE
#define FLASH_CREDS_VERSION 1
#define FLASH_CREDS_SSID_MAX 32
#define FLASH_CREDS_PASS_MAX 63

#define FLASH_CREDS_RECORD_SIZE 256
#define FLASH_CREDS_NAME_MAX 32
#define FLASH_CREDS_AUTH_KEY_LEN 32

#define FLASH_PERSONA_XINPUT 0
#define FLASH_PERSONA_KEYBOARD 1
#define FLASH_PERSONA_MAPLE 2
#define FLASH_PERSONA_PS3 3
#define FLASH_PERSONA_PS4 4
#define FLASH_PERSONA_XBOXONE 5
#define FLASH_PERSONA_DEBUG 6
#define FLASH_PERSONA_GENERIC_HID 7
#define FLASH_PERSONA_BT_HID 8
#define FLASH_PERSONA_BT_XBOX 9
#define FLASH_PERSONA_BT_PS 10

typedef struct __attribute__((packed)) {
    uint32_t magic;
    uint8_t version;
    uint8_t ssid_len;
    uint8_t pass_len;
    uint8_t name_len;
    uint32_t generation;
    uint8_t ssid[FLASH_CREDS_SSID_MAX];
    uint8_t password[FLASH_CREDS_PASS_MAX + 1];
    uint8_t device_name[FLASH_CREDS_NAME_MAX];
    uint8_t usb_persona;
    uint8_t auth_key_set;
    uint8_t auth_key[FLASH_CREDS_AUTH_KEY_LEN];
    uint8_t reserved[256 - 4 - 4 - 4 - 32 - 64 - 32 - 1 - 1 - FLASH_CREDS_AUTH_KEY_LEN - 4];
    uint32_t crc32;
} flash_creds_t;

_Static_assert(sizeof(flash_creds_t) == FLASH_CREDS_RECORD_SIZE,
               "flash_creds_t must be exactly 256 bytes");

bool flash_creds_load(flash_creds_t *out);

int flash_creds_store(const flash_creds_t *rec);

void flash_creds_clear(void);

void flash_creds_keep_settings(flash_creds_t *rec, const flash_creds_t *prev);
