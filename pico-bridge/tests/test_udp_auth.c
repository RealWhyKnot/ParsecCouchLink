#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "mbedtls/md.h"
#include "udp_auth.h"

static const uint8_t KEY[UDP_AUTH_KEY_LEN] = {
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
};

static const uint8_t NONCE[UDP_AUTH_NONCE_LEN] = {0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17};

static const uint8_t STATE_FRAME[UDP_AUTH_FRAME_LEN] = {
    0xA5, 0x01, 0x07, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
    0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xB2,
};

static const uint64_t VECTOR_COUNTER = 1700000000000000ull;

static const uint8_t VECTOR_TAG[UDP_AUTH_TAG_LEN] = {
    0xFC, 0x82, 0x20, 0x82, 0x19, 0x42, 0x2A, 0xF7, 0x5D, 0x47, 0x27, 0x5B, 0xAB, 0xED, 0x4C, 0x04,
    0x79, 0xB6, 0x95, 0xA3, 0x68, 0x76, 0xE6, 0x3A, 0x33, 0xBC, 0x8D, 0x0A, 0x0F, 0x75, 0xDB, 0xA2,
};

static const uint8_t VECTOR_ACK[UDP_AUTH_FRAME_LEN] = {
    0xA5, 0x04, 0x00, 0x17, 0x02, 0x1A, 0x0A, 0x1A, 0x01,
    0x05, 0x00, 0x00, 0xB6, 0x7E, 0xD3, 0x07, 0x27,
};

static const uint8_t VECTOR_CHALLENGE[UDP_AUTH_CHALLENGE_LEN] = {0x20, 0x21, 0x22, 0x23,
                                                                 0x24, 0x25, 0x26, 0x27};

static const uint8_t VECTOR_ACK_TAG[UDP_AUTH_TAG_LEN] = {
    0x3E, 0x27, 0x3A, 0x96, 0xE2, 0x50, 0x84, 0x05, 0x48, 0xBD, 0x9E, 0x06, 0xAD, 0xC6, 0xA5, 0x9E,
    0x55, 0x03, 0xA1, 0xA0, 0x89, 0xD3, 0x8F, 0x1A, 0x2F, 0x4D, 0x19, 0x8F, 0x16, 0x3E, 0xDF, 0xF9,
};

static void sign(const uint8_t *key, const uint8_t *nonce, const uint8_t *frame, uint64_t counter,
                 uint8_t *out) {
    memcpy(out, frame, UDP_AUTH_FRAME_LEN);
    for (int i = 0; i < UDP_AUTH_COUNTER_LEN; i++)
        out[UDP_AUTH_FRAME_LEN + i] = (uint8_t)(counter >> (8 * i));
    uint8_t msg[1 + UDP_AUTH_NONCE_LEN + UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN];
    msg[0] = UDP_AUTH_DOMAIN_FRAME;
    memcpy(&msg[1], nonce, UDP_AUTH_NONCE_LEN);
    memcpy(&msg[1 + UDP_AUTH_NONCE_LEN], out, UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN);
    int rc = mbedtls_md_hmac(mbedtls_md_info_from_type(MBEDTLS_MD_SHA256), key, UDP_AUTH_KEY_LEN,
                             msg, sizeof(msg), out + UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN);
    assert(rc == 0);
}

static void frame_of_type(uint8_t type, uint8_t *out) {
    memcpy(out, STATE_FRAME, UDP_AUTH_FRAME_LEN);
    out[1] = type;
}

static void unkeyed_pico_stays_open(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, NULL, NONCE));
    assert(!auth.keyed);
    uint8_t frame[UDP_AUTH_FRAME_LEN];
    const uint8_t types[] = {0x01, 0x02, 0x03, 0x05, 0x07, 0x08, 0x0A, 0x0B, 0x0E};
    for (size_t i = 0; i < sizeof(types); i++) {
        frame_of_type(types[i], frame);
        assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_OPEN);
    }
    uint8_t signed_frame[UDP_AUTH_SIGNED_LEN];
    sign(KEY, NONCE, STATE_FRAME, 5, signed_frame);
    assert(udp_auth_check(&auth, signed_frame, sizeof(signed_frame)) == UDP_AUTH_NOT_KEYED);
    uint8_t ext[UDP_AUTH_ACK_EXT_LEN];
    assert(!udp_auth_sign_ack(&auth, VECTOR_ACK, VECTOR_CHALLENGE, ext));
    udp_auth_free(&auth);
}

static void keyed_pico_drops_unsigned_commands(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, KEY, NONCE));
    uint8_t frame[UDP_AUTH_FRAME_LEN];
    const uint8_t locked[] = {0x01, 0x02, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0C, 0x0D, 0x0E};
    for (size_t i = 0; i < sizeof(locked); i++) {
        frame_of_type(locked[i], frame);
        assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_UNSIGNED);
    }
    frame_of_type(0x03, frame);
    assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_OPEN);
    frame_of_type(0x0B, frame);
    assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_OPEN);
    assert(udp_auth_check(&auth, frame, 16) == UDP_AUTH_BAD_LENGTH);
    assert(udp_auth_check(&auth, frame, UDP_AUTH_SIGNED_LEN - 1) == UDP_AUTH_BAD_LENGTH);
    udp_auth_free(&auth);
}

static void recovery_window_only_opens_reboot_to_setup(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, KEY, NONCE));
    assert(!auth.recovery_open);
    uint8_t frame[UDP_AUTH_FRAME_LEN];
    frame_of_type(0x07, frame);
    assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_UNSIGNED);
    udp_auth_set_recovery(&auth, true);
    assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_OPEN);
    const uint8_t locked[] = {0x01, 0x02, 0x05, 0x06, 0x08, 0x09, 0x0A, 0x0C, 0x0D, 0x0E};
    for (size_t i = 0; i < sizeof(locked); i++) {
        frame_of_type(locked[i], frame);
        assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_UNSIGNED);
    }
    udp_auth_set_recovery(&auth, false);
    frame_of_type(0x07, frame);
    assert(udp_auth_check(&auth, frame, sizeof(frame)) == UDP_AUTH_UNSIGNED);
    udp_auth_free(&auth);
}

static void known_vector_verifies(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, KEY, NONCE));
    uint8_t buf[UDP_AUTH_SIGNED_LEN];
    memcpy(buf, STATE_FRAME, UDP_AUTH_FRAME_LEN);
    for (int i = 0; i < UDP_AUTH_COUNTER_LEN; i++)
        buf[UDP_AUTH_FRAME_LEN + i] = (uint8_t)(VECTOR_COUNTER >> (8 * i));
    memcpy(buf + UDP_AUTH_FRAME_LEN + UDP_AUTH_COUNTER_LEN, VECTOR_TAG, UDP_AUTH_TAG_LEN);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_OK);
    assert(auth.last_counter == VECTOR_COUNTER);

    uint8_t again[UDP_AUTH_SIGNED_LEN];
    sign(KEY, NONCE, STATE_FRAME, VECTOR_COUNTER, again);
    assert(memcmp(again, buf, sizeof(buf)) == 0);
    udp_auth_free(&auth);
}

static void replays_and_old_counters_are_rejected(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, KEY, NONCE));
    uint8_t buf[UDP_AUTH_SIGNED_LEN];
    sign(KEY, NONCE, STATE_FRAME, 0, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_REPLAY);
    sign(KEY, NONCE, STATE_FRAME, 100, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_OK);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_REPLAY);
    sign(KEY, NONCE, STATE_FRAME, 99, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_REPLAY);
    sign(KEY, NONCE, STATE_FRAME, 101, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_OK);
    sign(KEY, NONCE, STATE_FRAME, UINT64_MAX, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_OK);
    sign(KEY, NONCE, STATE_FRAME, 102, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_REPLAY);
    udp_auth_free(&auth);
}

static void bad_tags_do_not_move_the_counter(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, KEY, NONCE));
    uint8_t buf[UDP_AUTH_SIGNED_LEN];

    sign(KEY, NONCE, STATE_FRAME, 1000, buf);
    buf[UDP_AUTH_SIGNED_LEN - 1] ^= 0x01;
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_BAD_TAG);

    sign(KEY, NONCE, STATE_FRAME, 1000, buf);
    buf[5] ^= 0x40;
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_BAD_TAG);

    sign(KEY, NONCE, STATE_FRAME, 1000, buf);
    buf[UDP_AUTH_FRAME_LEN] ^= 0x01;
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_BAD_TAG);

    uint8_t other_key[UDP_AUTH_KEY_LEN];
    memcpy(other_key, KEY, sizeof(other_key));
    other_key[0] ^= 0xFF;
    sign(other_key, NONCE, STATE_FRAME, 1000, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_BAD_TAG);

    assert(auth.last_counter == 0);
    sign(KEY, NONCE, STATE_FRAME, 1, buf);
    assert(udp_auth_check(&auth, buf, sizeof(buf)) == UDP_AUTH_OK);
    udp_auth_free(&auth);
}

static void new_boot_nonce_rejects_old_session_frames(void) {
    udp_auth_t before;
    assert(udp_auth_init(&before, KEY, NONCE));
    uint8_t buf[UDP_AUTH_SIGNED_LEN];
    sign(KEY, NONCE, STATE_FRAME, 50, buf);
    assert(udp_auth_check(&before, buf, sizeof(buf)) == UDP_AUTH_OK);
    udp_auth_free(&before);

    uint8_t rebooted_nonce[UDP_AUTH_NONCE_LEN];
    memcpy(rebooted_nonce, NONCE, sizeof(rebooted_nonce));
    rebooted_nonce[7] ^= 0x80;
    udp_auth_t after;
    assert(udp_auth_init(&after, KEY, rebooted_nonce));
    assert(after.last_counter == 0);
    assert(udp_auth_check(&after, buf, sizeof(buf)) == UDP_AUTH_BAD_TAG);
    sign(KEY, rebooted_nonce, STATE_FRAME, 1, buf);
    assert(udp_auth_check(&after, buf, sizeof(buf)) == UDP_AUTH_OK);
    udp_auth_free(&after);
}

static void extended_ack_matches_vector(void) {
    udp_auth_t auth;
    assert(udp_auth_init(&auth, KEY, NONCE));
    uint8_t out[UDP_AUTH_ACK_EXT_LEN];
    assert(udp_auth_sign_ack(&auth, VECTOR_ACK, VECTOR_CHALLENGE, out));
    assert(memcmp(out, VECTOR_ACK, UDP_AUTH_FRAME_LEN) == 0);
    assert(memcmp(out + UDP_AUTH_FRAME_LEN, NONCE, UDP_AUTH_NONCE_LEN) == 0);
    assert(memcmp(out + UDP_AUTH_FRAME_LEN + UDP_AUTH_NONCE_LEN, VECTOR_ACK_TAG,
                  UDP_AUTH_TAG_LEN) == 0);
    udp_auth_free(&auth);
}

static void discover_challenge_needs_the_marker(void) {
    uint8_t frame[UDP_AUTH_FRAME_LEN] = {0xA5, 0x03, 0x01, 0x00};
    const uint8_t *challenge = NULL;
    assert(!udp_auth_discover_challenge(frame, &challenge));
    frame[4] = UDP_AUTH_DISCOVER_MARKER;
    memcpy(&frame[5], VECTOR_CHALLENGE, UDP_AUTH_CHALLENGE_LEN);
    assert(udp_auth_discover_challenge(frame, &challenge));
    assert(challenge == &frame[5]);
    frame[1] = 0x01;
    assert(!udp_auth_discover_challenge(frame, &challenge));
}

int main(void) {
    unkeyed_pico_stays_open();
    keyed_pico_drops_unsigned_commands();
    recovery_window_only_opens_reboot_to_setup();
    known_vector_verifies();
    replays_and_old_counters_are_rejected();
    bad_tags_do_not_move_the_counter();
    new_boot_nonce_rejects_old_session_frames();
    extended_ack_matches_vector();
    discover_challenge_needs_the_marker();
    printf("udp_auth tests passed\n");
    return 0;
}
