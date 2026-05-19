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
#define TYPE_GET_LOG      0x05
#define TYPE_LOG_CHUNK    0x85

#define FLAG_PARSEC_CONNECTED        0x01
// Capability bit set in ACK[3] (the flags byte, formerly always zero)
// so the bridge can detect that this firmware understands TYPE_GET_LOG
// without the protocol version bumping. Keeps old bridges compatible:
// they decode the flags byte but ignore unrecognised bits.
#define ACK_FLAG_LOG_CHUNK_SUPPORTED 0x01

// LogChunk layout (matches bridge protocol.rs):
//   header (12 bytes) + payload (<= 256 bytes) + crc16 (2 bytes)
#define LOG_CHUNK_HEADER_LEN   12
#define LOG_CHUNK_MAX_PAYLOAD  256
#define LOG_CHUNK_FLAG_LAST    0x01

// Diag log snapshot capacity. The ring buffer is 4 KiB; sized to match
// so we always pull the full ring in one snapshot call.
#define DIAG_SNAPSHOT_CAP      4096

static struct udp_pcb *pcb;
static bool have_peer;
static ip_addr_t peer_addr;
static u16_t     peer_port;
static absolute_time_t next_keepalive;
static uint32_t  uid_short;
static uint32_t  tx_count;   // ack + keepalive + log-chunk datagrams sent
static uint32_t  rx_count;   // received datagrams (including malformed)

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

// CRC-16/CCITT-FALSE (alias CRC-16/IBM-3740): poly 0x1021, init 0xFFFF,
// no reflect, no XOR-out. Used for the variable-length LogChunk frames;
// 17-byte fixed packets keep the lighter CRC-8.
static uint16_t crc16_ccitt_false(const uint8_t *data, size_t n) {
    uint16_t c = 0xFFFF;
    for (size_t i = 0; i < n; i++) {
        c ^= ((uint16_t)data[i]) << 8;
        for (int b = 0; b < 8; b++) {
            c = (c & 0x8000) ? ((c << 1) ^ 0x1021) : (c << 1);
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
    // Advertise the LogChunk capability in the flags byte. Old bridges
    // decode the flags field but ignore unknown bits; new bridges gate
    // their CMD_GET_LOG attempt on this bit being set.
    buf[3] = ACK_FLAG_LOG_CHUNK_SUPPORTED;
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
        tx_count++;
        diag_log_printf("net_udp: ack -> %u.%u.%u.%u:%u (in_seq=%u)",
                        ip4_addr1(ip_2_ip4(to_addr)),
                        ip4_addr2(ip_2_ip4(to_addr)),
                        ip4_addr3(ip_2_ip4(to_addr)),
                        ip4_addr4(ip_2_ip4(to_addr)),
                        (unsigned)to_port, (unsigned)in_seq);
    }
}

// Push the diag-log ring out as one or more LogChunk datagrams. Bounded
// by DIAG_SNAPSHOT_CAP / LOG_CHUNK_MAX_PAYLOAD = 16 chunks; each chunk
// is its own pbuf_alloc/udp_sendto/pbuf_free so memory pressure shows
// up as a per-chunk ERR_MEM rather than a single all-or-nothing alloc.
static void send_log_chunks(const ip_addr_t *to_addr, u16_t to_port,
                            uint8_t in_seq) {
    static uint8_t snapshot[DIAG_SNAPSHOT_CAP];
    uint32_t lost = 0;
    size_t got = diag_log_snapshot(snapshot, sizeof(snapshot), &lost);
    // Always send at least one chunk so the bridge sees a definitive
    // "Pico answered, log was empty" reply instead of timing out.
    size_t total_chunks = (got == 0) ? 1
                                     : ((got + LOG_CHUNK_MAX_PAYLOAD - 1)
                                        / LOG_CHUNK_MAX_PAYLOAD);
    if (total_chunks > 255) total_chunks = 255;

    diag_log_printf("net_udp: log_chunks -> %u.%u.%u.%u:%u %u chunks lost=%u in_seq=%u",
                    ip4_addr1(ip_2_ip4(to_addr)),
                    ip4_addr2(ip_2_ip4(to_addr)),
                    ip4_addr3(ip_2_ip4(to_addr)),
                    ip4_addr4(ip_2_ip4(to_addr)),
                    (unsigned)to_port,
                    (unsigned)total_chunks, (unsigned)lost,
                    (unsigned)in_seq);

    for (size_t i = 0; i < total_chunks; i++) {
        size_t offset = i * LOG_CHUNK_MAX_PAYLOAD;
        size_t remain = (offset < got) ? (got - offset) : 0;
        size_t payload_len = remain < LOG_CHUNK_MAX_PAYLOAD
                           ? remain
                           : LOG_CHUNK_MAX_PAYLOAD;
        bool is_last = (i == total_chunks - 1);

        size_t total_len = LOG_CHUNK_HEADER_LEN + payload_len + 2;
        struct pbuf *p = pbuf_alloc(PBUF_TRANSPORT, total_len, PBUF_RAM);
        if (!p) {
            diag_log_printf("net_udp: log_chunk %u pbuf_alloc failed (ERR_MEM)",
                            (unsigned)i);
            return;
        }
        uint8_t *buf = (uint8_t *)p->payload;
        buf[0] = MAGIC;
        buf[1] = TYPE_LOG_CHUNK;
        buf[2] = (uint8_t)i;
        buf[3] = is_last ? LOG_CHUNK_FLAG_LAST : 0;
        buf[4] = (uint8_t)total_chunks;
        buf[5] = (uint8_t)(payload_len & 0xFF);
        buf[6] = (uint8_t)((payload_len >> 8) & 0xFF);
        buf[7] = 0;  // reserved
        // lost_bytes is meaningful only in chunk 0; zero elsewhere.
        uint32_t lost_field = (i == 0) ? lost : 0;
        buf[8]  = (uint8_t)(lost_field & 0xFF);
        buf[9]  = (uint8_t)((lost_field >> 8) & 0xFF);
        buf[10] = (uint8_t)((lost_field >> 16) & 0xFF);
        buf[11] = (uint8_t)((lost_field >> 24) & 0xFF);
        if (payload_len) {
            memcpy(&buf[LOG_CHUNK_HEADER_LEN], &snapshot[offset], payload_len);
        }
        uint16_t crc = crc16_ccitt_false(buf, LOG_CHUNK_HEADER_LEN + payload_len);
        buf[LOG_CHUNK_HEADER_LEN + payload_len + 0] = (uint8_t)(crc & 0xFF);
        buf[LOG_CHUNK_HEADER_LEN + payload_len + 1] = (uint8_t)((crc >> 8) & 0xFF);

        err_t e = udp_sendto(pcb, p, to_addr, to_port);
        pbuf_free(p);
        if (e != ERR_OK) {
            diag_log_printf("net_udp: log_chunk %u sendto err=%d (%s)",
                            (unsigned)i, (int)e, lwip_err_name(e));
            return;
        }
        tx_count++;
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
    rx_count++;
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
    } else if (type == TYPE_GET_LOG) {
        // The bridge's bundle command sends this when capturing diag
        // from a running Pico. We don't require the caller to be the
        // latched peer -- the host firewall already gates 4242 inbound,
        // and this lets `couchlink bundle` work even when discovery is
        // racing the latch.
        send_log_chunks(addr, port, seq);
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
    } else {
        tx_count++;
    }
}

bool     net_udp_has_peer(void) { return have_peer; }
uint32_t net_udp_tx_count(void) { return tx_count; }
uint32_t net_udp_rx_count(void) { return rx_count; }

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
