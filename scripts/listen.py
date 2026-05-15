#!/usr/bin/env python3
"""
Throwaway bench listener for couchlink.exe development.

Binds UDP 4242, decodes incoming Parsec CouchLink wire-protocol datagrams
per wiki/Protocol.md, replies to discover packets with a fake Pico ack, and
prints decoded state in real time.

Use this to verify the bridge end of the chain before any Pico firmware
exists: run this script on the same machine (or another LAN host), start
couchlink.exe, watch packets flow.

Usage:
    python listen.py [--port 4242] [--fw 0.1.0] [--hw 1]

Exit with Ctrl-C.
"""

import argparse
import socket
import struct
import sys
import time

MAGIC = 0xA5
PACKET_SIZE = 17

TYPE_STATE = 0x01
TYPE_HEARTBEAT = 0x02
TYPE_DISCOVER = 0x03
TYPE_ACK = 0x04

TYPE_NAMES = {
    TYPE_STATE: "state",
    TYPE_HEARTBEAT: "heartbeat",
    TYPE_DISCOVER: "discover",
    TYPE_ACK: "ack",
}

BUTTON_NAMES = [
    "DUP", "DDN", "DLF", "DRT",
    "STR", "BCK", "LTH", "RTH",
    "LB", "RB", "rsv10", "rsv11",
    "A", "B", "X", "Y",
]


def crc8(data: bytes) -> int:
    crc = 0
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = ((crc << 1) ^ 0x07) & 0xFF if (crc & 0x80) else (crc << 1) & 0xFF
    return crc


def decode(buf: bytes):
    if len(buf) != PACKET_SIZE:
        return None, "wrong size: %d" % len(buf)
    if buf[0] != MAGIC:
        return None, "wrong magic: 0x%02X" % buf[0]
    expected_crc = crc8(buf[:16])
    if buf[16] != expected_crc:
        return None, "bad crc (got 0x%02X, want 0x%02X)" % (buf[16], expected_crc)
    typ = buf[1]
    seq = buf[2]
    flags = buf[3]
    buttons, lt, rt, lx, ly, rx, ry = struct.unpack("<HBBhhhh", buf[4:16])
    return {
        "type": typ,
        "type_name": TYPE_NAMES.get(typ, "0x%02X" % typ),
        "seq": seq,
        "flags": flags,
        "buttons": buttons,
        "lt": lt, "rt": rt,
        "lx": lx, "ly": ly, "rx": rx, "ry": ry,
    }, None


PROTO_VERSION = 1
BOARD_PICO_2_W = 0x01


def encode_ack(seq: int, fw_major: int, fw_minor: int, fw_patch: int,
               board_type: int, uptime_seconds: int = 0,
               unique_id_short: int = 0) -> bytes:
    body = bytearray(12)
    body[0] = PROTO_VERSION
    body[1] = fw_major
    body[2] = fw_minor
    body[3] = fw_patch
    body[4] = board_type
    up = uptime_seconds & 0xFFFFFF
    body[5] = up & 0xFF
    body[6] = (up >> 8) & 0xFF
    body[7] = (up >> 16) & 0xFF
    body[8:12] = struct.pack("<I", unique_id_short & 0xFFFFFFFF)
    pkt = bytes([MAGIC, TYPE_ACK, seq, 0]) + bytes(body)
    return pkt + bytes([crc8(pkt)])


def buttons_str(b: int) -> str:
    return ",".join(BUTTON_NAMES[i] for i in range(16) if b & (1 << i)) or "."


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--port", type=int, default=4242)
    p.add_argument("--fw", default="0.1.0", help="firmware version M.m.p")
    p.add_argument("--board", type=int, default=BOARD_PICO_2_W,
                   help="board_type byte (1=Pico 2 W, 2=Pico W)")
    p.add_argument("--uid", type=lambda x: int(x, 0), default=0xDEADBEEF,
                   help="unique_id_short, hex or decimal")
    p.add_argument("--quiet-heartbeats", action="store_true",
                   help="print only non-heartbeat packets")
    args = p.parse_args()

    fw_parts = args.fw.split(".")
    fw_major, fw_minor, fw_patch = (int(fw_parts[i]) if i < len(fw_parts) else 0 for i in range(3))
    start_time = time.monotonic()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(("0.0.0.0", args.port))
    print("listen.py bound on 0.0.0.0:%d, fw=%d.%d.%d board=0x%02X uid=0x%08X"
          % (args.port, fw_major, fw_minor, fw_patch, args.board, args.uid))
    print("press Ctrl-C to quit")

    counts = {k: 0 for k in TYPE_NAMES}
    last_seq = {k: None for k in TYPE_NAMES}
    last_report = time.monotonic()
    ack_seq = 0

    try:
        while True:
            buf, peer = sock.recvfrom(64)
            pkt, err = decode(buf)
            if err:
                print("[drop %s] %s" % (peer, err))
                continue

            counts[pkt["type"]] = counts.get(pkt["type"], 0) + 1
            last_seq[pkt["type"]] = pkt["seq"]

            if pkt["type"] == TYPE_DISCOVER:
                uptime = int(time.monotonic() - start_time)
                reply = encode_ack(
                    ack_seq, fw_major, fw_minor, fw_patch,
                    args.board, uptime, args.uid,
                )
                ack_seq = (ack_seq + 1) & 0xFF
                sock.sendto(reply, peer)
                print("[%s] discover seq=%d -> ack (uptime=%ds)"
                      % (peer[0], pkt["seq"], uptime))
                continue

            if args.quiet_heartbeats and pkt["type"] == TYPE_HEARTBEAT:
                pass
            else:
                print("[%s] %-9s seq=%03d flags=0x%02X buttons={%s} "
                      "LT=%3d RT=%3d L=(%6d,%6d) R=(%6d,%6d)"
                      % (peer[0], pkt["type_name"], pkt["seq"], pkt["flags"],
                         buttons_str(pkt["buttons"]),
                         pkt["lt"], pkt["rt"],
                         pkt["lx"], pkt["ly"], pkt["rx"], pkt["ry"]))

            now = time.monotonic()
            if now - last_report >= 5.0:
                last_report = now
                summary = ", ".join("%s=%d" % (TYPE_NAMES[k], counts[k])
                                    for k in (TYPE_STATE, TYPE_HEARTBEAT, TYPE_DISCOVER))
                print("--- 5s summary: %s ---" % summary)
                counts = {k: 0 for k in TYPE_NAMES}

    except KeyboardInterrupt:
        print("\nbye")
        return 0


if __name__ == "__main__":
    sys.exit(main())
