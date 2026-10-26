#include <assert.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "flash_creds.h"
#include "hardware/flash.h"
#include "pico/flash.h"

uint8_t test_flash[PICO_FLASH_SIZE_BYTES];

void flash_range_erase(uint32_t offset, size_t count) {
    memset(&test_flash[offset], 0xFF, count);
}

void flash_range_program(uint32_t offset, const uint8_t *data, size_t count) {
    for (size_t i = 0; i < count; i++)
        test_flash[offset + i] &= data[i];
}

int flash_safe_execute(void (*func)(void *), void *param, uint32_t enter_exit_timeout_ms) {
    (void)enter_exit_timeout_ms;
    func(param);
    return PICO_OK;
}

void diag_log_msg(const char *msg) {
    (void)msg;
}

void diag_log_printf(const char *fmt, ...) {
    (void)fmt;
}

static void erase_all(void) {
    memset(test_flash, 0xFF, sizeof(test_flash));
}

static flash_creds_t base_record(const char *ssid) {
    flash_creds_t rec;
    memset(&rec, 0, sizeof(rec));
    rec.ssid_len = (uint8_t)strlen(ssid);
    memcpy(rec.ssid, ssid, rec.ssid_len);
    rec.pass_len = 6;
    memcpy(rec.password, "secret", 6);
    rec.name_len = 4;
    memcpy(rec.device_name, "den1", 4);
    rec.usb_persona = FLASH_PERSONA_PS4;
    return rec;
}

static void fill_key(uint8_t *key, uint8_t seed) {
    for (int i = 0; i < FLASH_CREDS_AUTH_KEY_LEN; i++)
        key[i] = (uint8_t)(seed + i);
}

static void key_sits_inside_the_crc_and_keeps_old_offsets(void) {
    assert(offsetof(flash_creds_t, device_name) == 108);
    assert(offsetof(flash_creds_t, usb_persona) == 140);
    assert(offsetof(flash_creds_t, auth_key_set) == 141);
    assert(offsetof(flash_creds_t, auth_key) == 142);
    assert(offsetof(flash_creds_t, auth_key) + FLASH_CREDS_AUTH_KEY_LEN <=
           offsetof(flash_creds_t, crc32));
    assert(offsetof(flash_creds_t, crc32) == FLASH_CREDS_RECORD_SIZE - 4);
}

static void key_round_trips_through_flash(void) {
    erase_all();
    flash_creds_t rec = base_record("home");
    rec.auth_key_set = 1;
    fill_key(rec.auth_key, 0xA0);
    assert(flash_creds_store(&rec) == 0);

    flash_creds_t back;
    assert(flash_creds_load(&back));
    assert(back.auth_key_set == 1);
    assert(memcmp(back.auth_key, rec.auth_key, FLASH_CREDS_AUTH_KEY_LEN) == 0);
    assert(back.ssid_len == 4 && memcmp(back.ssid, "home", 4) == 0);
    assert(back.usb_persona == FLASH_PERSONA_PS4);
}

static void unkeyed_record_matches_the_old_layout(void) {
    erase_all();
    flash_creds_t rec = base_record("home");
    memset(rec.reserved, 0x5A, sizeof(rec.reserved));
    assert(flash_creds_store(&rec) == 0);

    flash_creds_t back;
    assert(flash_creds_load(&back));
    assert(back.auth_key_set == 0);
    const uint8_t *raw = (const uint8_t *)&back;
    for (size_t i = offsetof(flash_creds_t, usb_persona) + 1; i < offsetof(flash_creds_t, crc32);
         i++)
        assert(raw[i] == 0);
}

static void corrupt_key_byte_invalidates_the_slot(void) {
    erase_all();
    flash_creds_t rec = base_record("home");
    rec.auth_key_set = 1;
    fill_key(rec.auth_key, 0x10);
    assert(flash_creds_store(&rec) == 0);
    size_t slot_a = PICO_FLASH_SIZE_BYTES - 2u * FLASH_SECTOR_SIZE;
    test_flash[slot_a + offsetof(flash_creds_t, auth_key) + 5] ^= 0x01;
    flash_creds_t back;
    assert(!flash_creds_load(&back));
}

static void wifi_rewrite_keeps_name_persona_and_key(void) {
    erase_all();
    flash_creds_t first = base_record("home");
    first.auth_key_set = 1;
    fill_key(first.auth_key, 0x40);
    assert(flash_creds_store(&first) == 0);

    flash_creds_t next;
    memset(&next, 0, sizeof(next));
    next.ssid_len = 5;
    memcpy(next.ssid, "other", 5);
    flash_creds_t prev;
    assert(flash_creds_load(&prev));
    flash_creds_keep_settings(&next, &prev);
    assert(flash_creds_store(&next) == 0);

    flash_creds_t back;
    assert(flash_creds_load(&back));
    assert(back.generation == 2);
    assert(back.ssid_len == 5 && memcmp(back.ssid, "other", 5) == 0);
    assert(back.pass_len == 0);
    assert(back.name_len == 4 && memcmp(back.device_name, "den1", 4) == 0);
    assert(back.usb_persona == FLASH_PERSONA_PS4);
    assert(back.auth_key_set == 1);
    assert(memcmp(back.auth_key, first.auth_key, FLASH_CREDS_AUTH_KEY_LEN) == 0);
}

int main(void) {
    key_sits_inside_the_crc_and_keeps_old_offsets();
    key_round_trips_through_flash();
    unkeyed_record_matches_the_old_layout();
    corrupt_key_byte_invalidates_the_slot();
    wifi_rewrite_keeps_name_persona_and_key();
    printf("flash_creds tests passed\n");
    return 0;
}
