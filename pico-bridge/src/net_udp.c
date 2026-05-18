#include "net_udp.h"

#include <string.h>

#include "pico/cyw43_arch.h"
#include "pico/stdlib.h"
#include "pico/unique_id.h"
#include "lwip/udp.h"
#include "lwip/pbuf.h"

#include "diag_log.h"
#include "gamepad_state.h"
#include "version.h"

// Translate an lwIP err_t to a short name. Without this every
// udp_sendto failure looks the same in pico-diag.txt -- with it,
// memory pressure (ERR_MEM, often a sign that PBUF_POOL_SIZE needs
// raising under burst), routing failures (ERR_RTE, AP dropped),
// netif-down (ERR_IF), and config bugs (ERR_VAL / ERR_ARG) are
// distinguishable at a glance.
static const char *lwip_err_name(err_t e) {
    switch (e) {
        case ERR_OK:         return "OK";
        case ERR_MEM:        return "ERR_MEM";
        case ERR_BUF:        return "ERR_BUF";
        case ERR_TIMEOUT:    return "ERR_TIMEOUT";
        case ERR_RTE:        return "ERR_RTE";
        case ERR_INPROGRESS: return "ERR_INPROGRESS";
        case ERR_VAL:        return "ERR_VAL";
        case ERR_WOULDBLOCK: return "ERR_WOULDBLOCK";
        case ERR_USE:        return "ERR_USE";
        case ERR_ALREADY:    return "ERR_ALREADY";
        case ERR_ISCONN:     return "ERR_ISCONN";
        case ERR_CONN:       return "ERR_CONN";
        case ERR_IF:         return "ERR_IF";
        case ERR_ABRT:       return "ERR_ABRT";
        case ERR_RST:        return "ERR_RST";
        case ERR_CLSD:       return "ERR_CLSD";
        case ERR_ARG:        return "ERR_ARG";
        default:             return "ERR_???";
    }
}

#define UDP_PORT          4242
#define WIRE_PKT_SIZE     17
#define MAGIC             0xA5

#define TYPE_STATE        0x01
#define TYPE_HEARTBEAT    0x02
#define TYPE_DISCOVER     0x03
#define TYPE_ACK          0x04

#define FLAG_PARSEC_CONNECTED 0x01

static struct udp_pcb *pcb;
static bool have_peer;
static ip_addr_t peer_addr;
static u16_t     peer_port;
static absolute_time_t next_keepalive;
static uint32_t  uid_short;

// CRC-8/SMBUS: poly 0x07, init 0x00, no reflect, no XOR-out.
static uint8_t crc8(const uint8_t *data, size_t n) {
    uint8_t c = 0;
    for (size_t i = 0; i < n; i++) {
        c ^= data[i];
        for (int b = 0; b < 8; b++) {
            c = (c & 0x80) ? ((c << 1) ^ 0x07) : (c << 1);
        }
    }
    return c;
}

static void build_state_fields(uint8_t *buf, uint8_t type, uint8_t seq) {
    buf[0] = MAGIC;
    buf[1] = type;
    buf[2] = seq;
    buf[3] = 0;
    // Body is 12 bytes of zeros for a Pico-side keepalive; not used.
    memset(&buf[4], 0, 12);
}

static void send_ack(const ip_addr_t *to_addr, u16_t to_port, uint8_t in_seq) {
    static uint8_t out_seq = 0;
    uint8_t buf[WIRE_PKT_SIZE];
    memset(buf, 0, sizeof(buf));
    buf[0] = MAGIC;
    buf[1] = TYPE_ACK;
    buf[2] = out_seq++;
    buf[3] = 0;
    // body[0..11]
    buf[4]  = PICO_BRIDGE_UDP_PROTO_VERSION;
    buf[5]  = PICO_BRIDGE_FW_MAJOR;
    buf[6]  = PICO_BRIDGE_FW_MINOR;
    buf[7]  = PICO_BRIDGE_FW_PATCH;
    buf[8]  = PICO_BRIDGE_BOARD_TYPE;
    uint32_t up = (uint32_t)(to_ms_since_boot(get_absolute_time()) / 1000);
    up &= 0x00FFFFFFu;
    buf[9]  = (uint8_t)(up & 0xFF);
    buf[10] = (uint8_t)((up >> 8) & 0xFF);
    buf[11] = (uint8_t)((up >> 16) & 0xFF);
    buf[12] = (uint8_t)((uid_short >>  0) & 0xFF);
    buf[13] = (uint8_t)((uid_short >>  8) & 0xFF);
    buf[14] = (uint8_t)((uid_short >> 16) & 0xFF);
    buf[15] = (uint8_t)((uid_short >> 24) & 0xFF);
    buf[16] = crc8(buf, 16);

    struct pbuf *p = pbuf_alloc(PBUF_TRANSPORT, WIRE_PKT_SIZE, PBUF_RAM);
    if (!p) {
        diag_log_msg("net_udp: ack pbuf_alloc failed (ERR_MEM)");
        return;
    }
    memcpy(p->payload, buf, WIRE_PKT_SIZE);
    err_t e = udp_sendto(pcb, p, to_addr, to_port);
    pbuf_free(p);
    if (e != ERR_OK) {
        diag_log_printf("net_udp: ack send err=%d (%s)", (int)e, lwip_err_name(e));
    } else {
        diag_log_printf("net_udp: ack -> %u.%u.%u.%u:%u (in_seq=%u)",
                        ip4_addr1(ip_2_ip4(to_addr)),
                        ip4_addr2(ip_2_ip4(to_addr)),
                        ip4_addr3(ip_2_ip4(to_addr)),
                        ip4_addr4(ip_2_ip4(to_addr)),
                        (unsigned)to_port, (unsigned)in_seq);
    }
}

static void apply_state_body(const uint8_t *body, uint8_t flags) {
    g_gamepad_state.buttons       = (uint16_t)body[0] | ((uint16_t)body[1] << 8);
    g_gamepad_state.left_trigger  = body[2];
    g_gamepad_state.right_trigger = body[3];
    g_gamepad_state.left_x        = (int16_t)((uint16_t)body[4] | ((uint16_t)body[5] << 8));
    g_gamepad_state.left_y        = (int16_t)((uint16_t)body[6] | ((uint16_t)body[7] << 8));
    g_gamepad_state.right_x       = (int16_t)((uint16_t)body[8] | ((uint16_t)body[9] << 8));
    g_gamepad_state.right_y       = (int16_t)((uint16_t)body[10] | ((uint16_t)body[11] << 8));
    g_parsec_connected = (flags & FLAG_PARSEC_CONNECTED) ? 1 : 0;
    g_last_packet_ms = to_ms_since_boot(get_absolute_time());
}

static void on_recv(void *arg, struct udp_pcb *pcb_in, struct pbuf *p,
                    const ip_addr_t *addr, u16_t port) {
    (void)arg; (void)pcb_in;
    if (!p) return;
    // Use tot_len, not len: a chained pbuf has len < tot_len. Copy into a
    // contiguous local buffer rather than reading p->payload directly so we
    // never read past the first segment of a chain.
    if (p->tot_len != WIRE_PKT_SIZE) { pbuf_free(p); return; }
    uint8_t buf[WIRE_PKT_SIZE];
    u16_t copied = pbuf_copy_partial(p, buf, WIRE_PKT_SIZE, 0);
    pbuf_free(p);
    p = NULL;
    if (copied != WIRE_PKT_SIZE) return;
    if (buf[0] != MAGIC) return;
    if (crc8(buf, 16) != buf[16]) return;

    uint8_t type  = buf[1];
    uint8_t seq   = buf[2];
    uint8_t flags = buf[3];

    if (type == TYPE_DISCOVER) {
        send_ack(addr, port, seq);
        // Don't latch the peer yet -- wait for the first STATE or
        // HEARTBEAT, which proves end-to-end works.
    } else if (type == TYPE_STATE || type == TYPE_HEARTBEAT) {
        if (!have_peer
            || !ip_addr_cmp(&peer_addr, addr)
            || peer_port != port) {
            peer_addr = *addr;
            peer_port = port;
            have_peer = true;
            next_keepalive = make_timeout_time_ms(1000);
            diag_log_printf("net_udp: peer latched %u.%u.%u.%u:%u",
                            ip4_addr1(ip_2_ip4(addr)),
                            ip4_addr2(ip_2_ip4(addr)),
                            ip4_addr3(ip_2_ip4(addr)),
                            ip4_addr4(ip_2_ip4(addr)),
                            (unsigned)port);
        }
        apply_state_body(&buf[4], flags);
    }
    // pbuf already freed above after pbuf_copy_partial; nothing to do here.
}

static void send_keepalive(void) {
    if (!have_peer) return;
    static uint8_t out_seq = 0;
    uint8_t buf[WIRE_PKT_SIZE];
    build_state_fields(buf, TYPE_HEARTBEAT, out_seq++);
    buf[16] = crc8(buf, 16);
    struct pbuf *p = pbuf_alloc(PBUF_TRANSPORT, WIRE_PKT_SIZE, PBUF_RAM);
    if (!p) {
        diag_log_msg("net_udp: keepalive pbuf_alloc failed (ERR_MEM)");
        return;
    }
    memcpy(p->payload, buf, WIRE_PKT_SIZE);
    err_t e = udp_sendto(pcb, p, &peer_addr, peer_port);
    pbuf_free(p);
    if (e != ERR_OK) {
        diag_log_printf("net_udp: keepalive send err=%d (%s)",
                        (int)e, lwip_err_name(e));
    }
}

bool net_udp_init(void) {
    pcb = udp_new();
    if (!pcb) {
        diag_log_msg("net_udp: udp_new failed");
        return false;
    }
    err_t e = udp_bind(pcb, IP_ANY_TYPE, UDP_PORT);
    if (e != ERR_OK) {
        diag_log_printf("net_udp: udp_bind err=%d (%s)",
                        (int)e, lwip_err_name(e));
        return false;
    }
    udp_recv(pcb, on_recv, NULL);

    // Cache the short unique-id once for ack bodies.
    pico_unique_board_id_t id;
    pico_get_unique_board_id(&id);
    uid_short = (uint32_t)id.id[0]
              | ((uint32_t)id.id[1] << 8)
              | ((uint32_t)id.id[2] << 16)
              | ((uint32_t)id.id[3] << 24);

    have_peer = false;
    next_keepalive = make_timeout_time_ms(1000);
    diag_log_msg("net_udp: listening on UDP/4242");
    return true;
}

void net_udp_task(void) {
    if (!have_peer) return;
    if (absolute_time_diff_us(get_absolute_time(), next_keepalive) <= 0) {
        send_keepalive();
        next_keepalive = make_timeout_time_ms(1000);
    }
}
