#include "flash_creds.h"

#include <string.h>

#include "pico/stdlib.h"
#include "pico/flash.h"
#include "hardware/flash.h"
#include "hardware/sync.h"

#include "diag_log.h"

// Pico 2 W has 4 MB of QSPI flash; Pico W has 2 MB. Reserving the last
// two 4 KB sectors works for both. Offsets here are flash-relative
// (what flash_range_erase / flash_range_program expect); reads use
// XIP_BASE + offset.
#ifndef PICO_FLASH_SIZE_BYTES
#error "PICO_FLASH_SIZE_BYTES not defined; check board headers"
#endif

#define SLOT_SIZE FLASH_SECTOR_SIZE // 4096
#define SLOT_A_OFFSET (PICO_FLASH_SIZE_BYTES - 2 * SLOT_SIZE)
#define SLOT_B_OFFSET (PICO_FLASH_SIZE_BYTES - 1 * SLOT_SIZE)

_Static_assert(FLASH_CREDS_RECORD_SIZE <= FLASH_PAGE_SIZE,
               "credential record must fit in a single flash program page");

// CRC-32 (zlib / Ethernet variant): poly reflected 0xEDB88320, init
// 0xFFFFFFFF, final XOR 0xFFFFFFFF.
static uint32_t crc32_zlib(const uint8_t *data, size_t n) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < n; i++) {
        crc ^= data[i];
        for (int b = 0; b < 8; b++) {
            uint32_t mask = -(crc & 1u);
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return ~crc;
}

static bool record_is_valid(const flash_creds_t *r) {
    if (r->magic != FLASH_CREDS_MAGIC)
        return false;
    if (r->version != FLASH_CREDS_VERSION)
        return false;
    if (r->ssid_len == 0 || r->ssid_len > FLASH_CREDS_SSID_MAX)
        return false;
    if (r->pass_len > FLASH_CREDS_PASS_MAX)
        return false;
    if (r->name_len > FLASH_CREDS_NAME_MAX)
        return false;
    uint32_t crc = crc32_zlib((const uint8_t *)r, FLASH_CREDS_RECORD_SIZE - 4);
    return crc == r->crc32;
}

static const flash_creds_t *slot_ptr(uint32_t offset) {
    return (const flash_creds_t *)(XIP_BASE + offset);
}

bool flash_creds_load(flash_creds_t *out) {
    const flash_creds_t *a = slot_ptr(SLOT_A_OFFSET);
    const flash_creds_t *b = slot_ptr(SLOT_B_OFFSET);
    bool va = record_is_valid(a);
    bool vb = record_is_valid(b);
    const flash_creds_t *winner = NULL;
    if (va && vb) {
        // Wrap-aware comparison: prefer the newer generation.
        winner = ((int32_t)(a->generation - b->generation) > 0) ? a : b;
    } else if (va) {
        winner = a;
    } else if (vb) {
        winner = b;
    } else {
        return false;
    }
    memcpy(out, winner, FLASH_CREDS_RECORD_SIZE);
    return true;
}

typedef struct {
    uint32_t offset;
    const uint8_t *data;
} flash_op_args_t;

static void __no_inline_not_in_flash_func(do_program_slot)(void *param) {
    flash_op_args_t *a = (flash_op_args_t *)param;
    flash_range_erase(a->offset, SLOT_SIZE);
    flash_range_program(a->offset, a->data, FLASH_PAGE_SIZE);
}

static void __no_inline_not_in_flash_func(do_erase_slot)(void *param) {
    uint32_t offset = *(uint32_t *)param;
    flash_range_erase(offset, SLOT_SIZE);
}

static int program_slot(uint32_t offset, const flash_creds_t *rec) {
    uint8_t buf[FLASH_PAGE_SIZE];
    memset(buf, 0xFF, sizeof(buf));
    memcpy(buf, rec, FLASH_CREDS_RECORD_SIZE);

    flash_op_args_t args = {.offset = offset, .data = buf};
    // flash_safe_execute coordinates with core 1 (cyw43) when present and
    // keeps interrupt-disabled windows scoped to the actual erase/program
    // calls instead of one big block. 1 s lockout is far more than the
    // ~30 ms a sector erase + page program takes on RP2350.
    int rc = flash_safe_execute(do_program_slot, &args, 1000);
    if (rc != PICO_OK) {
        diag_log_printf("flash_creds: flash_safe_execute (program) rc=%d", rc);
        return -2;
    }

    const flash_creds_t *back = slot_ptr(offset);
    return record_is_valid(back) ? 0 : -3;
}

int flash_creds_store(const flash_creds_t *rec) {
    if (rec->ssid_len == 0 || rec->ssid_len > FLASH_CREDS_SSID_MAX)
        return -1;
    if (rec->pass_len > FLASH_CREDS_PASS_MAX)
        return -1;
    if (rec->name_len > FLASH_CREDS_NAME_MAX)
        return -1;

    // Pick the older slot to write into so we keep at least one valid
    // record at all times.
    const flash_creds_t *a = slot_ptr(SLOT_A_OFFSET);
    const flash_creds_t *b = slot_ptr(SLOT_B_OFFSET);
    bool va = record_is_valid(a);
    bool vb = record_is_valid(b);
    uint32_t target_offset;
    uint32_t new_generation;
    if (!va && !vb) {
        target_offset = SLOT_A_OFFSET;
        new_generation = 1;
    } else if (va && !vb) {
        target_offset = SLOT_B_OFFSET;
        new_generation = a->generation + 1;
    } else if (!va && vb) {
        target_offset = SLOT_A_OFFSET;
        new_generation = b->generation + 1;
    } else {
        const flash_creds_t *winner = ((int32_t)(a->generation - b->generation) > 0) ? a : b;
        target_offset = (winner == a) ? SLOT_B_OFFSET : SLOT_A_OFFSET;
        new_generation = winner->generation + 1;
    }

    flash_creds_t to_write = *rec;
    to_write.magic = FLASH_CREDS_MAGIC;
    to_write.version = FLASH_CREDS_VERSION;
    to_write.generation = new_generation;
    memset(to_write.reserved, 0, sizeof(to_write.reserved));
    to_write.crc32 = crc32_zlib((const uint8_t *)&to_write, FLASH_CREDS_RECORD_SIZE - 4);

    int rc = program_slot(target_offset, &to_write);
    if (rc == 0) {
        diag_log_printf("flash_creds: wrote slot %s, gen=%u",
                        target_offset == SLOT_A_OFFSET ? "A" : "B", (unsigned)new_generation);
    } else {
        diag_log_printf("flash_creds: store failed rc=%d", rc);
    }
    return rc;
}

void flash_creds_clear(void) {
    uint32_t a = SLOT_A_OFFSET;
    uint32_t b = SLOT_B_OFFSET;
    int rc_a = flash_safe_execute(do_erase_slot, &a, 1000);
    int rc_b = flash_safe_execute(do_erase_slot, &b, 1000);
    if (rc_a != PICO_OK || rc_b != PICO_OK) {
        diag_log_printf("flash_creds: clear rc_a=%d rc_b=%d", rc_a, rc_b);
    } else {
        diag_log_msg("flash_creds: both slots erased");
    }
}
