import struct
import unittest
from dataclasses import dataclass, field

from scripts import listen

PEER = ("192.0.2.7", 51000)
PICO = listen.FakePico(firmware_major=1, firmware_minor=2, firmware_patch=3, board_type=1, unique_id_short=0xDEADBEEF)


def build_packet(packet_type: int, seq: int, buttons: int = 0) -> bytes:
    body = bytes([listen.MAGIC, packet_type, seq, 0x40]) + struct.pack("<HBBhhhh", buttons, 10, 20, -1, 2, -3, 4)
    return body + bytes([listen.crc8(body)])


@dataclass
class Recorder:
    lines: list[str] = field(default_factory=list[str])
    sent: list[tuple[bytes, tuple[str, int]]] = field(default_factory=list[tuple[bytes, tuple[str, int]]])
    now: float = 100.0

    def emit(self, text: str) -> None:
        self.lines.append(text)

    def send(self, data: bytes, peer: tuple[str, int]) -> None:
        self.sent.append((data, peer))

    def clock(self) -> float:
        return self.now

    def listener(self, *, quiet_heartbeats: bool = False) -> listen.Listener:
        options = listen.Options(port=4242, pico=PICO, quiet_heartbeats=quiet_heartbeats)
        return listen.Listener(options=options, send=self.send, emit=self.emit, clock=self.clock)


class DecodeTests(unittest.TestCase):
    def test_crc8_matches_known_check_value(self) -> None:
        self.assertEqual(listen.crc8(b"123456789"), 0xF4)

    def test_decodes_state_packet_fields(self) -> None:
        packet = listen.decode(build_packet(listen.TYPE_STATE, 7, buttons=0x1001))
        self.assertEqual(
            packet,
            listen.Packet(
                type=listen.TYPE_STATE,
                type_name="state",
                seq=7,
                flags=0x40,
                buttons=0x1001,
                left_trigger=10,
                right_trigger=20,
                left_x=-1,
                left_y=2,
                right_x=-3,
                right_y=4,
            ),
        )

    def test_names_unknown_type_by_hex_value(self) -> None:
        self.assertEqual(listen.decode(build_packet(0x7F, 0)).type_name, "0x7F")

    def test_rejects_wrong_size(self) -> None:
        with self.assertRaisesRegex(listen.PacketError, r"^wrong size: 3$"):
            _ = listen.decode(b"\xa5\x01\x00")

    def test_rejects_wrong_magic(self) -> None:
        with self.assertRaisesRegex(listen.PacketError, r"^wrong magic: 0x00$"):
            _ = listen.decode(bytes(listen.PACKET_SIZE))

    def test_rejects_bad_crc(self) -> None:
        packet = bytearray(build_packet(listen.TYPE_STATE, 1))
        expected = packet[-1]
        packet[-1] ^= 0xFF
        with self.assertRaisesRegex(listen.PacketError, f"got 0x{packet[-1]:02X}, want 0x{expected:02X}"):
            _ = listen.decode(bytes(packet))


class EncodeTests(unittest.TestCase):
    def test_ack_carries_version_board_uptime_and_id(self) -> None:
        ack = listen.encode_ack(9, PICO, uptime_seconds=0x010203)
        self.assertEqual(ack[:4], bytes([listen.MAGIC, listen.TYPE_ACK, 9, 0]))
        self.assertEqual(ack[4:12], bytes([1, 1, 2, 3, 1, 0x03, 0x02, 0x01]))
        self.assertEqual(ack[12:16], struct.pack("<I", 0xDEADBEEF))
        self.assertEqual(ack[16], listen.crc8(ack[:16]))
        self.assertEqual(len(ack), listen.PACKET_SIZE)

    def test_buttons_str_lists_set_bits_or_dot(self) -> None:
        self.assertEqual(listen.buttons_str(0), ".")
        self.assertEqual(listen.buttons_str(0x1001), "DUP,A")


class ListenerTests(unittest.TestCase):
    def test_reports_dropped_datagram_with_peer_and_reason(self) -> None:
        recorder = Recorder()
        recorder.listener().handle(b"\x00", PEER)
        self.assertEqual(recorder.lines, ["[drop ('192.0.2.7', 51000)] wrong size: 1\n"])

    def test_answers_discover_with_ack_and_counts_up_the_ack_sequence(self) -> None:
        recorder = Recorder()
        listener = recorder.listener()
        recorder.now = 103.9
        listener.handle(build_packet(listen.TYPE_DISCOVER, 5), PEER)
        listener.handle(build_packet(listen.TYPE_DISCOVER, 6), PEER)
        self.assertEqual(
            recorder.sent,
            [(listen.encode_ack(0, PICO, 3), PEER), (listen.encode_ack(1, PICO, 3), PEER)],
        )
        self.assertEqual(recorder.lines[0], "[192.0.2.7] discover seq=5 -> ack (uptime=3s)\n")

    def test_prints_state_line_in_fixed_columns(self) -> None:
        recorder = Recorder()
        recorder.listener().handle(build_packet(listen.TYPE_STATE, 7, buttons=0x1001), PEER)
        self.assertEqual(
            recorder.lines,
            [
                (
                    "[192.0.2.7] state     seq=007 flags=0x40 buttons={DUP,A} "
                    "LT= 10 RT= 20 L=(    -1,     2) R=(    -3,     4)\n"
                )
            ],
        )

    def test_quiet_heartbeats_hides_heartbeat_lines_only(self) -> None:
        recorder = Recorder()
        listener = recorder.listener(quiet_heartbeats=True)
        listener.handle(build_packet(listen.TYPE_HEARTBEAT, 1), PEER)
        listener.handle(build_packet(listen.TYPE_STATE, 2), PEER)
        self.assertEqual(len(recorder.lines), 1)
        self.assertIn(" state ", recorder.lines[0])

    def test_summary_prints_after_five_seconds_and_resets_counts(self) -> None:
        recorder = Recorder()
        listener = recorder.listener(quiet_heartbeats=True)
        listener.handle(build_packet(listen.TYPE_DISCOVER, 1), PEER)
        listener.handle(build_packet(listen.TYPE_HEARTBEAT, 2), PEER)
        recorder.now = 105.0
        listener.handle(build_packet(listen.TYPE_HEARTBEAT, 3), PEER)
        listener.handle(build_packet(listen.TYPE_HEARTBEAT, 4), PEER)
        self.assertEqual(recorder.lines[1:], ["--- 5s summary: state=0, heartbeat=2, discover=1 ---\n"])
        self.assertEqual(listener.counts[listen.TYPE_HEARTBEAT], 1)
