#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../src/xgip_constants.h"

static void require_true(int condition, const char *message) {
    if (!condition) {
        fprintf(stderr, "FAIL: %s\n", message);
        exit(1);
    }
}

int main(void) {
    static const uint8_t expected_id[8] = {'X', 'G', 'I', 'P', '1', '0', 0, 0};
    uint8_t actual_id[8] = XGIP_COMPATIBLE_ID_BYTES;

    require_true(XGIP_DEVICE_CLASS == 0xFF, "XGIP device class must be vendor-specific");
    require_true(XGIP_DEVICE_SUBCLASS == 0x47, "XGIP device subclass must match GIP USB");
    require_true(XGIP_DEVICE_PROTOCOL == 0xD0, "XGIP device protocol must match GIP USB");
    require_true(XGIP_MS_OS_STRING_INDEX == 0xEE, "MS OS 1.0 string index changed");
    require_true(XGIP_MS_VENDOR_REQ_CODE == 0x90, "GIP MS OS vendor request code changed");
    require_true(XGIP_NUM_INTERFACES_WITHOUT_AUDIO == 0x01,
                 "GIP controller-without-audio interface count changed");
    require_true(memcmp(actual_id, expected_id, sizeof(expected_id)) == 0,
                 "GIP compatible ID must be XGIP10");

    return 0;
}
