#!/usr/bin/env python3
import argparse
import logging
import socket
import struct
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from types import TracebackType
from typing import NoReturn

MAGIC = 0xA5
PACKET_SIZE = 17
CHECKSUM_OFFSET = 16
RECEIVE_BUFFER_SIZE = 64
SUMMARY_INTERVAL_SECONDS = 5.0

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
    "DUP",
    "DDN",
    "DLF",
    "DRT",
    "STR",
    "BACK",
    "LTH",
    "RTH",
    "LB",
    "RB",
    "rsv10",
    "rsv11",
    "A",
    "B",
    "X",
    "Y",
]

PROTO_VERSION = 1
BOARD_PICO_2_W = 0x01

log = logging.getLogger("listen")


class PacketError(ValueError):
    pass


@dataclass(frozen=True)
class Packet:
    type: int
    type_name: str
    seq: int
    flags: int
    buttons: int
    left_trigger: int
    right_trigger: int
    left_x: int
    left_y: int
    right_x: int
    right_y: int


@dataclass(frozen=True)
class FakePico:
    firmware_major: int
    firmware_minor: int
    firmware_patch: int
    board_type: int
    unique_id_short: int


@dataclass(frozen=True)
class Options:
    port: int
    pico: FakePico
    quiet_heartbeats: bool


def crc8(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = ((crc << 1) ^ 0x07) & 0xFF if (crc & 0x80) else (crc << 1) & 0xFF
    return crc


def decode(buf: bytes) -> Packet:
    if len(buf) != PACKET_SIZE:
        message = f"wrong size: {len(buf)}"
        raise PacketError(message)
    if buf[0] != MAGIC:
        message = f"wrong magic: 0x{buf[0]:02X}"
        raise PacketError(message)
    expected_crc = crc8(buf[:CHECKSUM_OFFSET])
    if buf[CHECKSUM_OFFSET] != expected_crc:
        message = f"bad crc (got 0x{buf[CHECKSUM_OFFSET]:02X}, want 0x{expected_crc:02X})"
        raise PacketError(message)
    packet_type = buf[1]
    buttons, left_trigger, right_trigger, left_x, left_y, right_x, right_y = struct.unpack(
        "<HBBhhhh", buf[4:CHECKSUM_OFFSET]
    )
    return Packet(
        type=packet_type,
        type_name=TYPE_NAMES.get(packet_type, f"0x{packet_type:02X}"),
        seq=buf[2],
        flags=buf[3],
        buttons=buttons,
        left_trigger=left_trigger,
        right_trigger=right_trigger,
        left_x=left_x,
        left_y=left_y,
        right_x=right_x,
        right_y=right_y,
    )


def encode_ack(seq: int, pico: FakePico, uptime_seconds: int = 0) -> bytes:
    body = bytearray(12)
    body[0] = PROTO_VERSION
    body[1] = pico.firmware_major
    body[2] = pico.firmware_minor
    body[3] = pico.firmware_patch
    body[4] = pico.board_type
    uptime = uptime_seconds & 0xFFFFFF
    body[5] = uptime & 0xFF
    body[6] = (uptime >> 8) & 0xFF
    body[7] = (uptime >> 16) & 0xFF
    body[8:12] = struct.pack("<I", pico.unique_id_short & 0xFFFFFFFF)
    packet = bytes([MAGIC, TYPE_ACK, seq, 0]) + bytes(body)
    return packet + bytes([crc8(packet)])


def buttons_str(buttons: int) -> str:
    return ",".join(BUTTON_NAMES[bit] for bit in range(16) if buttons & (1 << bit)) or "."


def format_state(host: str, packet: Packet) -> str:
    return (
        f"[{host}] {packet.type_name:<9} seq={packet.seq:03d} flags=0x{packet.flags:02X} "
        f"buttons={{{buttons_str(packet.buttons)}}} "
        f"LT={packet.left_trigger:3d} RT={packet.right_trigger:3d} "
        f"L=({packet.left_x:6d},{packet.left_y:6d}) R=({packet.right_x:6d},{packet.right_y:6d})"
    )


@dataclass
class Listener:
    options: Options
    send: Callable[[bytes, tuple[str, int]], None]
    emit: Callable[[str], None]
    clock: Callable[[], float] = time.monotonic
    counts: dict[int, int] = field(default_factory=lambda: dict.fromkeys(TYPE_NAMES, 0))
    ack_seq: int = 0
    start_time: float = field(init=False)
    last_report: float = field(init=False)

    def __post_init__(self) -> None:
        self.start_time = self.clock()
        self.last_report = self.clock()

    def handle(self, buf: bytes, peer: tuple[str, int]) -> None:
        try:
            packet = decode(buf)
        except PacketError as error:
            self.emit(f"[drop {peer}] {error}\n")
            return

        self.counts[packet.type] = self.counts.get(packet.type, 0) + 1

        if packet.type == TYPE_DISCOVER:
            self.acknowledge(packet, peer)
            return

        if not (self.options.quiet_heartbeats and packet.type == TYPE_HEARTBEAT):
            self.emit(format_state(peer[0], packet) + "\n")

        self.report_summary()

    def acknowledge(self, packet: Packet, peer: tuple[str, int]) -> None:
        uptime = int(self.clock() - self.start_time)
        reply = encode_ack(self.ack_seq, self.options.pico, uptime)
        self.ack_seq = (self.ack_seq + 1) & 0xFF
        self.send(reply, peer)
        self.emit(f"[{peer[0]}] discover seq={packet.seq} -> ack (uptime={uptime}s)\n")

    def report_summary(self) -> None:
        now = self.clock()
        if now - self.last_report < SUMMARY_INTERVAL_SECONDS:
            return
        self.last_report = now
        summary = ", ".join(
            f"{TYPE_NAMES[kind]}={self.counts[kind]}" for kind in (TYPE_STATE, TYPE_HEARTBEAT, TYPE_DISCOVER)
        )
        self.emit(f"--- 5s summary: {summary} ---\n")
        self.counts = dict.fromkeys(TYPE_NAMES, 0)


def parse_integer(text: str) -> int:
    return int(text, 0)


def parse_options(arguments: list[str]) -> Options:
    parser = argparse.ArgumentParser()
    _ = parser.add_argument("--port", type=int, default=4242)
    _ = parser.add_argument("--fw", default="0.1.0", help="firmware version M.m.p")
    _ = parser.add_argument("--board", type=int, default=BOARD_PICO_2_W, help="board_type byte (1=Pico 2 W, 2=Pico W)")
    _ = parser.add_argument("--uid", type=parse_integer, default=0xDEADBEEF, help="unique_id_short, hex or decimal")
    _ = parser.add_argument("--quiet-heartbeats", action="store_true", help="print only non-heartbeat packets")
    parsed = parser.parse_args(arguments)

    firmware_parts = str(parsed.fw).split(".")
    major, minor, patch = (int(firmware_parts[index]) if index < len(firmware_parts) else 0 for index in range(3))
    return Options(
        port=int(parsed.port),
        pico=FakePico(
            firmware_major=major,
            firmware_minor=minor,
            firmware_patch=patch,
            board_type=int(parsed.board),
            unique_id_short=int(parsed.uid),
        ),
        quiet_heartbeats=bool(parsed.quiet_heartbeats),
    )


def serve(sock: socket.socket, listener: Listener) -> NoReturn:
    while True:
        buf, peer = sock.recvfrom(RECEIVE_BUFFER_SIZE)
        listener.handle(buf, peer)


def report_crash(kind: type[BaseException], error: BaseException, trace: TracebackType | None) -> None:
    log.error("listen.py stopped on an unhandled error", exc_info=(kind, error, trace))
    _ = sys.stderr.write(f"listen.py failed: {error}\n")


def write_output(text: str) -> None:
    _ = sys.stdout.write(text)


def main() -> int:
    sys.excepthook = report_crash
    options = parse_options(sys.argv[1:])
    pico = options.pico

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:

        def send_reply(data: bytes, peer: tuple[str, int]) -> None:
            _ = sock.sendto(data, peer)

        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("", options.port))
        banner = (
            f"listen.py bound on 0.0.0.0:{options.port}, "
            f"fw={pico.firmware_major}.{pico.firmware_minor}.{pico.firmware_patch} "
            f"board=0x{pico.board_type:02X} uid=0x{pico.unique_id_short:08X}\n"
        )
        write_output(banner)
        write_output("press Ctrl-C to quit\n")
        try:
            serve(sock, Listener(options=options, send=send_reply, emit=write_output))
        except KeyboardInterrupt:
            write_output("\nbye\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
