use super::*;
use std::net::Ipv4Addr;

const KEY: [u8; KEY_LEN] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
];
const NONCE: [u8; NONCE_LEN] = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];
const STATE_FRAME: [u8; PACKET_SIZE] = [
    0xA5, 0x01, 0x07, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
    0xB2,
];
const VECTOR_COUNTER: u64 = 1_700_000_000_000_000;
const VECTOR_TAG: &str = "fc82208219422af75d47275babed4c0479b695a36876e63a33bc8d0a0f75dba2";
const VECTOR_ACK: [u8; PACKET_SIZE] = [
    0xA5, 0x04, 0x00, 0x17, 0x02, 0x1A, 0x0A, 0x1A, 0x01, 0x05, 0x00, 0x00, 0xB6, 0x7E, 0xD3, 0x07,
    0x27,
];
const VECTOR_CHALLENGE: [u8; CHALLENGE_LEN] = [0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27];
const VECTOR_ACK_TAG: &str = "3e273a96e250840548bd9e06adc6a59e5503a1a089d38f1a2f4d198f163edff9";
const VECTOR_UID: u32 = 0x07D3_7EB6;

fn vector_tag() -> Vec<u8> {
    hex::decode(VECTOR_TAG).unwrap()
}

fn ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50))
}

fn peer() -> SocketAddr {
    SocketAddr::new(ip(), protocol::PORT)
}

fn ext_ack(key: [u8; KEY_LEN], challenge: [u8; CHALLENGE_LEN]) -> Vec<u8> {
    let tag = ack_mac(&key, NONCE, challenge, &VECTOR_ACK)
        .finalize()
        .into_bytes();
    let mut buf = VECTOR_ACK.to_vec();
    buf.extend_from_slice(&NONCE);
    buf.extend_from_slice(&tag);
    buf
}

fn with_key(uid: u32) -> Option<Key> {
    (uid == VECTOR_UID).then(|| Zeroizing::new(KEY))
}

#[test]
fn signed_frame_matches_firmware_vector() {
    let signed = sign_frame(&KEY, NONCE, &STATE_FRAME, VECTOR_COUNTER);
    assert_eq!(signed.len(), 57);
    assert_eq!(&signed[..PACKET_SIZE], &STATE_FRAME);
    assert_eq!(
        &signed[PACKET_SIZE..PACKET_SIZE + COUNTER_LEN],
        &VECTOR_COUNTER.to_le_bytes()
    );
    assert_eq!(
        &signed[PACKET_SIZE + COUNTER_LEN..],
        vector_tag().as_slice()
    );
}

#[test]
fn signature_covers_frame_counter_nonce_and_key() {
    let base = sign_frame(&KEY, NONCE, &STATE_FRAME, 5);
    let mut frame = STATE_FRAME;
    frame[6] ^= 1;
    assert_ne!(base[25..], sign_frame(&KEY, NONCE, &frame, 5)[25..]);
    assert_ne!(base[25..], sign_frame(&KEY, NONCE, &STATE_FRAME, 6)[25..]);
    let mut nonce = NONCE;
    nonce[0] ^= 1;
    assert_ne!(base[25..], sign_frame(&KEY, nonce, &STATE_FRAME, 5)[25..]);
    let mut key = KEY;
    key[31] ^= 1;
    assert_ne!(base[25..], sign_frame(&key, NONCE, &STATE_FRAME, 5)[25..]);
}

#[test]
fn ack_tag_matches_firmware_vector() {
    let tag = ack_mac(&KEY, NONCE, VECTOR_CHALLENGE, &VECTOR_ACK)
        .finalize()
        .into_bytes();
    assert_eq!(hex::encode(tag), VECTOR_ACK_TAG);
}

#[test]
fn counter_strictly_increases_even_when_the_clock_stalls_or_rewinds() {
    let counter = AtomicU64::new(0);
    let first = advance_counter(&counter, 1_000);
    let stalled = advance_counter(&counter, 1_000);
    let rewound = advance_counter(&counter, 10);
    let jumped = advance_counter(&counter, 5_000);
    assert_eq!(first, 1_000);
    assert_eq!(stalled, 1_001);
    assert_eq!(rewound, 1_002);
    assert_eq!(jumped, 5_000);
    let after_zero = advance_counter(&counter, 0);
    assert!(after_zero > jumped);
}

#[test]
fn process_counter_starts_near_the_wall_clock() {
    let before = unix_micros();
    let first = next_counter();
    let second = next_counter();
    assert!(first >= before);
    assert!(second > first);
}

#[test]
fn discover_frame_carries_marker_and_challenge() {
    let frame = discover_frame_with(9, VECTOR_CHALLENGE);
    let pkt = Packet::decode(&frame).unwrap();
    assert_eq!(pkt.kind, PacketKind::Discover);
    assert_eq!(pkt.seq, 9);
    assert_eq!(frame[4], DISCOVER_MARKER);
    assert_eq!(&frame[5..13], &VECTOR_CHALLENGE);
    assert_eq!(&frame[13..16], &[0, 0, 0]);
}

#[test]
fn verified_ack_pairs_the_peer_and_seals_frames() {
    let mut s = Sessions::default();
    s.remember_challenge([0xEE; CHALLENGE_LEN]);
    s.remember_challenge(VECTOR_CHALLENGE);
    let pkt = s
        .accept_reply(ip(), &ext_ack(KEY, VECTOR_CHALLENGE), &mut with_key)
        .unwrap();
    let PacketKind::Ack(info) = pkt.kind else {
        panic!("expected ack")
    };
    assert_eq!(info.unique_id_short, VECTOR_UID);
    assert_eq!(info.proto_version, protocol::PROTO_VERSION);
    s.check_access(peer()).unwrap();

    let sealed = s.seal(ip(), &STATE_FRAME, VECTOR_COUNTER);
    assert_eq!(sealed.len(), SIGNED_LEN);
    assert_eq!(
        &sealed[PACKET_SIZE + COUNTER_LEN..],
        vector_tag().as_slice()
    );
}

#[test]
fn ack_for_an_unknown_challenge_locks_the_peer() {
    let mut s = Sessions::default();
    s.remember_challenge([0xEE; CHALLENGE_LEN]);
    s.accept_reply(ip(), &ext_ack(KEY, VECTOR_CHALLENGE), &mut with_key)
        .unwrap();
    let err = s.check_access(peer()).unwrap_err().to_string();
    assert!(err.contains("0x07D37EB6"), "{err}");
    assert_eq!(s.seal(ip(), &STATE_FRAME, 1).len(), PACKET_SIZE);
}

#[test]
fn ack_signed_with_another_key_locks_the_peer() {
    let mut s = Sessions::default();
    s.remember_challenge(VECTOR_CHALLENGE);
    let mut other = KEY;
    other[0] ^= 0x80;
    s.accept_reply(ip(), &ext_ack(other, VECTOR_CHALLENGE), &mut with_key)
        .unwrap();
    assert!(s.check_access(peer()).is_err());
}

#[test]
fn keyed_pico_without_a_saved_key_locks_the_peer() {
    let mut s = Sessions::default();
    s.remember_challenge(VECTOR_CHALLENGE);
    s.accept_reply(ip(), &ext_ack(KEY, VECTOR_CHALLENGE), &mut |_| None)
        .unwrap();
    assert!(s.check_access(peer()).is_err());
}

#[test]
fn tampered_ack_body_fails_verification() {
    let mut s = Sessions::default();
    s.remember_challenge(VECTOR_CHALLENGE);
    let mut buf = ext_ack(KEY, VECTOR_CHALLENGE);
    buf[9] ^= 0x01;
    buf[16] = protocol::crc8(&buf[..16]);
    s.accept_reply(ip(), &buf, &mut with_key).unwrap();
    assert!(s.check_access(peer()).is_err());
}

#[test]
fn open_pico_ack_clears_a_stale_pairing() {
    let mut s = Sessions::default();
    s.remember_challenge(VECTOR_CHALLENGE);
    s.accept_reply(ip(), &ext_ack(KEY, VECTOR_CHALLENGE), &mut with_key)
        .unwrap();
    let mut open = VECTOR_ACK;
    open[4] = protocol::PROTO_VERSION_OPEN;
    open[16] = protocol::crc8(&open[..16]);
    s.accept_reply(ip(), &open, &mut with_key).unwrap();
    s.check_access(peer()).unwrap();
    let sealed = s.seal(ip(), &STATE_FRAME, 1);
    assert_eq!(&*sealed, &STATE_FRAME);
}

#[test]
fn unknown_peers_get_plain_frames() {
    let s = Sessions::default();
    let sealed = s.seal(ip(), &STATE_FRAME, 1);
    assert_eq!(&*sealed, &STATE_FRAME);
    s.check_access(peer()).unwrap();
}

#[test]
fn reboot_nonce_is_picked_up_from_the_next_ack() {
    let mut s = Sessions::default();
    s.remember_challenge(VECTOR_CHALLENGE);
    s.accept_reply(ip(), &ext_ack(KEY, VECTOR_CHALLENGE), &mut with_key)
        .unwrap();
    let before = s.seal(ip(), &STATE_FRAME, 10);

    let rebooted = [0x99; NONCE_LEN];
    let tag = ack_mac(&KEY, rebooted, VECTOR_CHALLENGE, &VECTOR_ACK)
        .finalize()
        .into_bytes();
    let mut buf = VECTOR_ACK.to_vec();
    buf.extend_from_slice(&rebooted);
    buf.extend_from_slice(&tag);
    s.accept_reply(ip(), &buf, &mut with_key).unwrap();
    let after = s.seal(ip(), &STATE_FRAME, 10);
    assert_ne!(&*before, &*after);
    assert_eq!(*after, sign_frame(&KEY, rebooted, &STATE_FRAME, 10));
}

#[test]
fn challenge_ring_forgets_old_challenges() {
    let mut s = Sessions::default();
    s.remember_challenge(VECTOR_CHALLENGE);
    for i in 0..u8::try_from(CHALLENGE_RING).unwrap() {
        s.remember_challenge([i; CHALLENGE_LEN]);
    }
    s.accept_reply(ip(), &ext_ack(KEY, VECTOR_CHALLENGE), &mut with_key)
        .unwrap();
    assert!(s.check_access(peer()).is_err());
}

#[test]
fn plain_key_text_round_trips() {
    let text = format!("plain:{}", hex::encode(KEY));
    assert_eq!(*unprotect_key(&text).unwrap(), KEY);
    assert!(unprotect_key("plain:0011").is_err());
    assert!(unprotect_key("rot13:00").is_err());
    assert!(unprotect_key(&hex::encode(KEY)).is_err());
}

#[test]
fn protected_key_round_trips() {
    let text = protect_key(&KEY).unwrap();
    assert!(!text.contains(&hex::encode(KEY)));
    assert_eq!(*unprotect_key(&text).unwrap(), KEY);
}

#[cfg(windows)]
#[test]
fn dpapi_blob_is_tamper_evident() {
    let text = protect_key(&KEY).unwrap();
    assert!(text.starts_with("dpapi:"));
    let mut blob = hex::decode(&text["dpapi:".len()..]).unwrap();
    let last = blob.len() - 1;
    blob[last] ^= 0x01;
    assert!(unprotect_key(&format!("dpapi:{}", hex::encode(blob))).is_err());
}
