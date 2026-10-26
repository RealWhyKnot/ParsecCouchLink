#include "udp_auth.h"

#include <string.h>

#include "mbedtls/constant_time.h"

#define TYPE_DISCOVER 0x03
#define TYPE_REBOOT_TO_SETUP 0x07
#define TYPE_GET_VERSION 0x0B

static uint64_t get_u64_le(const uint8_t *p) {
    uint64_t v = 0;
    for (int i = 7; i >= 0; i--)
        v = (v << 8) | p[i];
    return v;
}

static bool hmac_tag(udp_auth_t *auth, uint8_t domain, const uint8_t *a, size_t a_len,
                     const uint8_t *b, size_t b_len, uint8_t *tag) {
    if (!auth->ready)
        return false;
    if (mbedtls_md_hmac_reset(&auth->md) != 0)
        return false;
    if (mbedtls_md_hmac_update(&auth->md, &domain, 1) != 0)
        return false;
    if (mbedtls_md_hmac_update(&auth->md, auth->nonce, UDP_AUTH_NONCE_LEN) != 0)
        return false;
    if (a_len && mbedtls_md_hmac_update(&auth->md, a, a_len) != 0)
        return false;
    if (b_len && mbedtls_md_hmac_update(&auth->md, b, b_len) != 0)
        return false;
    return mbedtls_md_hmac_finish(&auth->md, tag) == 0;
}

bool udp_auth_init(udp_auth_t *auth, const uint8_t *key, const uint8_t *nonce) {
    auth->keyed = false;
    auth->ready = false;
    auth->recovery_open = false;
    auth->last_counter = 0;
    memcpy(auth->nonce, nonce, UDP_AUTH_NONCE_LEN);
    mbedtls_md_init(&auth->md);
    if (!key)
        return true;
    auth->keyed = true;
    const mbedtls_md_info_t *info = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
    if (!info || mbedtls_md_setup(&auth->md, info, 1) != 0)
        return false;
    if (mbedtls_md_hmac_starts(&auth->md, key, UDP_AUTH_KEY_LEN) != 0) {
        mbedtls_md_free(&auth->md);
        mbedtls_md_init(&auth->md);
        return false;
    }
    auth->ready = true;
    return true;
}

void udp_auth_free(udp_auth_t *auth) {
    mbedtls_md_free(&auth->md);
    auth->keyed = false;
    auth->ready = false;
    auth->recovery_open = false;
    auth->last_counter = 0;
}

bool udp_auth_type_is_open(uint8_t type) {
    return type == TYPE_DISCOVER || type == TYPE_GET_VERSION;
}

void udp_auth_set_recovery(udp_auth_t *auth, bool open) {
    auth->recovery_open = open;
}

udp_auth_result_t udp_auth_check(udp_auth_t *auth, const uint8_t *buf, size_t len) {
    if (len == UDP_AUTH_FRAME_LEN) {
        if (!auth->keyed || udp_auth_type_is_open(buf[1]))
            return UDP_AUTH_OPEN;
        if (auth->recovery_open && buf[1] == TYPE_REBOOT_TO_SETUP)
            return UDP_AUTH_OPEN;
        return UDP_AUTH_UNSIGNED;
    }
    if (len != UDP_AUTH_SIGNED_LEN)
        return UDP_AUTH_BAD_LENGTH;
    if (!auth->keyed)
        return UDP_AUTH_NOT_KEYED;
    uint8_t tag[UDP_AUTH_TAG_LEN];
    if (!hmac_tag(auth, UDP_AUTH_DOMAIN_FRAME, buf, UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN, NULL,
                  0, tag))
        return UDP_AUTH_BAD_TAG;
    if (mbedtls_ct_memcmp(tag, buf + UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN, UDP_AUTH_TAG_LEN) !=
        0)
        return UDP_AUTH_BAD_TAG;
    uint64_t counter = get_u64_le(buf + UDP_AUTH_FRAME_LEN);
    if (counter <= auth->last_counter)
        return UDP_AUTH_REPLAY;
    auth->last_counter = counter;
    return UDP_AUTH_OK;
}

bool udp_auth_discover_challenge(const uint8_t *frame, const uint8_t **challenge) {
    if (frame[1] != TYPE_DISCOVER || frame[4] != UDP_AUTH_DISCOVER_MARKER)
        return false;
    *challenge = &frame[5];
    return true;
}

bool udp_auth_sign_ack(udp_auth_t *auth, const uint8_t *ack, const uint8_t *challenge,
                       uint8_t *out) {
    if (!auth->keyed)
        return false;
    uint8_t tag[UDP_AUTH_TAG_LEN];
    if (!hmac_tag(auth, UDP_AUTH_DOMAIN_ACK, challenge, UDP_AUTH_CHALLENGE_LEN, ack,
                  UDP_AUTH_FRAME_LEN, tag))
        return false;
    memcpy(out, ack, UDP_AUTH_FRAME_LEN);
    memcpy(out + UDP_AUTH_FRAME_LEN, auth->nonce, UDP_AUTH_NONCE_LEN);
    memcpy(out + UDP_AUTH_FRAME_LEN + UDP_AUTH_NONCE_LEN, tag, UDP_AUTH_TAG_LEN);
    return true;
}

const char *udp_auth_result_name(udp_auth_result_t r) {
    switch (r) {
    case UDP_AUTH_OK:
        return "ok";
    case UDP_AUTH_OPEN:
        return "open";
    case UDP_AUTH_UNSIGNED:
        return "unsigned";
    case UDP_AUTH_BAD_LENGTH:
        return "length";
    case UDP_AUTH_BAD_TAG:
        return "tag";
    case UDP_AUTH_REPLAY:
        return "replay";
    case UDP_AUTH_NOT_KEYED:
        return "unkeyed";
    default:
        return "?";
    }
}
