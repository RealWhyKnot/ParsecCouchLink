#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "mbedtls/md.h"

#define UDP_AUTH_FRAME_LEN 17
#define UDP_AUTH_KEY_LEN 32
#define UDP_AUTH_NONCE_LEN 8
#define UDP_AUTH_COUNTER_LEN 8
#define UDP_AUTH_TAG_LEN 32
#define UDP_AUTH_CHALLENGE_LEN 8
#define UDP_AUTH_SIGNED_LEN (UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN + UDP_AUTH_TAG_LEN)
#define UDP_AUTH_ACK_EXT_LEN (UDP_AUTH_FRAME_LEN + UDP_AUTH_NONCE_LEN + UDP_AUTH_TAG_LEN)
#define UDP_AUTH_DOMAIN_FRAME 0x01
#define UDP_AUTH_DOMAIN_ACK 0x02
#define UDP_AUTH_DISCOVER_MARKER 0x02

typedef enum {
    UDP_AUTH_OK = 0,
    UDP_AUTH_OPEN,
    UDP_AUTH_UNSIGNED,
    UDP_AUTH_BAD_LENGTH,
    UDP_AUTH_BAD_TAG,
    UDP_AUTH_REPLAY,
    UDP_AUTH_NOT_KEYED,
} udp_auth_result_t;

typedef struct {
    bool keyed;
    bool ready;
    bool recovery_open;
    uint8_t nonce[UDP_AUTH_NONCE_LEN];
    uint64_t last_counter;
    mbedtls_md_context_t md;
} udp_auth_t;

bool udp_auth_init(udp_auth_t *auth, const uint8_t *key, const uint8_t *nonce);
void udp_auth_free(udp_auth_t *auth);
bool udp_auth_type_is_open(uint8_t type);
void udp_auth_set_recovery(udp_auth_t *auth, bool open);
udp_auth_result_t udp_auth_check(udp_auth_t *auth, const uint8_t *buf, size_t len);
bool udp_auth_discover_challenge(const uint8_t *frame, const uint8_t **challenge);
bool udp_auth_sign_ack(udp_auth_t *auth, const uint8_t *ack, const uint8_t *challenge,
                       uint8_t *out);
const char *udp_auth_result_name(udp_auth_result_t r);
