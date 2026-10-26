use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::protocol::{self, DecodeError, Packet, PacketKind, PACKET_SIZE};
use crate::{cdc, config, net};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 8;
pub const COUNTER_LEN: usize = 8;
pub const TAG_LEN: usize = 32;
pub const CHALLENGE_LEN: usize = 8;
pub const SIGNED_LEN: usize = PACKET_SIZE + COUNTER_LEN + TAG_LEN;
pub const ACK_EXT_LEN: usize = PACKET_SIZE + NONCE_LEN + TAG_LEN;

const DOMAIN_FRAME: u8 = 0x01;
const DOMAIN_ACK: u8 = 0x02;
const DISCOVER_MARKER: u8 = 0x02;
const CHALLENGE_RING: usize = 16;
const LEARN_ATTEMPTS: u8 = 3;
const LEARN_WAIT: Duration = Duration::from_millis(500);

pub const RECOVERY_HINT: &str = "This PC isn't paired with that Pico, so it only takes the switch to setup mode in its first two minutes after power-up. If nothing happens, unplug the Pico, plug it back in and try again.";

type HmacSha256 = Hmac<Sha256>;
type Key = Zeroizing<[u8; KEY_LEN]>;

#[derive(Clone)]
enum PeerAuth {
    Open,
    Paired { key: Key, nonce: [u8; NONCE_LEN] },
    Locked { uid: u32 },
}

#[derive(Default)]
struct Sessions {
    peers: HashMap<IpAddr, PeerAuth>,
    challenges: VecDeque<[u8; CHALLENGE_LEN]>,
}

impl Sessions {
    fn remember_challenge(&mut self, challenge: [u8; CHALLENGE_LEN]) {
        if self.challenges.len() == CHALLENGE_RING {
            self.challenges.pop_front();
        }
        self.challenges.push_back(challenge);
    }

    fn accept_reply(
        &mut self,
        ip: IpAddr,
        buf: &[u8],
        lookup: &mut dyn FnMut(u32) -> Option<Key>,
    ) -> Result<Packet, DecodeError> {
        if buf.len() != ACK_EXT_LEN {
            let packet = Packet::decode(buf)?;
            if let PacketKind::Ack(info) = packet.kind {
                if info.proto_version == protocol::PROTO_VERSION_OPEN {
                    self.peers.insert(ip, PeerAuth::Open);
                }
            }
            return Ok(packet);
        }
        let ack = &buf[..PACKET_SIZE];
        let packet = Packet::decode(ack)?;
        let PacketKind::Ack(info) = packet.kind else {
            return Err(DecodeError::WrongSize);
        };
        let uid = info.unique_id_short;
        let nonce: [u8; NONCE_LEN] = buf[PACKET_SIZE..PACKET_SIZE + NONCE_LEN]
            .try_into()
            .unwrap();
        let tag = &buf[PACKET_SIZE + NONCE_LEN..];
        let state = match lookup(uid) {
            Some(key)
                if self
                    .challenges
                    .iter()
                    .any(|c| ack_tag_matches(&key, &nonce, c, ack, tag)) =>
            {
                PeerAuth::Paired { key, nonce }
            }
            Some(_) => {
                tracing::debug!("udp-auth: ack from {ip} uid 0x{uid:08X} failed verification");
                PeerAuth::Locked { uid }
            }
            None => {
                tracing::debug!("udp-auth: no key for uid 0x{uid:08X} at {ip}");
                PeerAuth::Locked { uid }
            }
        };
        self.peers.insert(ip, state);
        Ok(packet)
    }

    fn seal(&self, ip: IpAddr, frame: &[u8; PACKET_SIZE], counter: u64) -> Sealed {
        match self.peers.get(&ip) {
            Some(PeerAuth::Paired { key, nonce }) => {
                Sealed::signed(sign_frame(key, nonce, frame, counter))
            }
            _ => Sealed::plain(frame),
        }
    }

    fn check_access(&self, peer: SocketAddr) -> Result<()> {
        if let Some(PeerAuth::Locked { uid }) = self.peers.get(&peer.ip()) {
            bail!(locked_message(*uid, peer));
        }
        Ok(())
    }
}

pub struct Sealed {
    buf: [u8; SIGNED_LEN],
    len: usize,
}

impl Sealed {
    fn plain(frame: &[u8; PACKET_SIZE]) -> Self {
        let mut buf = [0u8; SIGNED_LEN];
        buf[..PACKET_SIZE].copy_from_slice(frame);
        Self {
            buf,
            len: PACKET_SIZE,
        }
    }

    fn signed(buf: [u8; SIGNED_LEN]) -> Self {
        Self {
            buf,
            len: SIGNED_LEN,
        }
    }
}

impl std::ops::Deref for Sealed {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

static SESSIONS: OnceLock<Mutex<Sessions>> = OnceLock::new();
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn sessions() -> MutexGuard<'static, Sessions> {
    SESSIONS
        .get_or_init(|| Mutex::new(Sessions::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn locked_message(uid: u32, peer: SocketAddr) -> String {
    format!(
        "Pico 0x{uid:08X} at {} only accepts signed commands, and this PC doesn't have its key. \
         To pair it again, plug the Pico into this PC and run `couchlink configure-wifi` within two minutes.",
        peer.ip()
    )
}

fn hmac_with(key: &[u8; KEY_LEN]) -> HmacSha256 {
    <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length")
}

pub fn sign_frame(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    frame: &[u8; PACKET_SIZE],
    counter: u64,
) -> [u8; SIGNED_LEN] {
    let mut out = [0u8; SIGNED_LEN];
    out[..PACKET_SIZE].copy_from_slice(frame);
    out[PACKET_SIZE..PACKET_SIZE + COUNTER_LEN].copy_from_slice(&counter.to_le_bytes());
    let mut mac = hmac_with(key);
    mac.update(&[DOMAIN_FRAME]);
    mac.update(nonce);
    mac.update(&out[..PACKET_SIZE + COUNTER_LEN]);
    out[PACKET_SIZE + COUNTER_LEN..].copy_from_slice(&mac.finalize().into_bytes());
    out
}

fn ack_mac(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    challenge: &[u8; CHALLENGE_LEN],
    ack: &[u8],
) -> HmacSha256 {
    let mut mac = hmac_with(key);
    mac.update(&[DOMAIN_ACK]);
    mac.update(nonce);
    mac.update(challenge);
    mac.update(ack);
    mac
}

fn ack_tag_matches(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    challenge: &[u8; CHALLENGE_LEN],
    ack: &[u8],
    tag: &[u8],
) -> bool {
    ack_mac(key, nonce, challenge, ack)
        .verify_slice(tag)
        .is_ok()
}

fn unix_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn advance_counter(counter: &AtomicU64, now: u64) -> u64 {
    let mut prev = counter.load(Ordering::Relaxed);
    loop {
        let next = prev.saturating_add(1).max(now);
        match counter.compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(actual) => prev = actual,
        }
    }
}

pub fn next_counter() -> u64 {
    advance_counter(&COUNTER, unix_micros())
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::getrandom(&mut out).expect("OS random source unavailable");
    out
}

fn discover_frame_with(seq: u8, challenge: &[u8; CHALLENGE_LEN]) -> [u8; PACKET_SIZE] {
    let mut buf = Packet::discover(seq).encode();
    buf[4] = DISCOVER_MARKER;
    buf[5..5 + CHALLENGE_LEN].copy_from_slice(challenge);
    buf[16] = protocol::crc8(&buf[..16]);
    buf
}

pub fn discover_frame(seq: u8) -> [u8; PACKET_SIZE] {
    let challenge = random_bytes::<CHALLENGE_LEN>();
    sessions().remember_challenge(challenge);
    discover_frame_with(seq, &challenge)
}

pub fn decode_reply(peer: SocketAddr, buf: &[u8]) -> Result<Packet, DecodeError> {
    sessions().accept_reply(peer.ip(), buf, &mut stored_key)
}

pub fn seal(peer: SocketAddr, frame: &[u8; PACKET_SIZE]) -> Sealed {
    sessions().seal(peer.ip(), frame, next_counter())
}

pub fn check_access(peer: SocketAddr) -> Result<()> {
    sessions().check_access(peer)
}

pub async fn prepare(peer: SocketAddr) -> Result<()> {
    let known = sessions().peers.contains_key(&peer.ip());
    if !known {
        learn(peer).await;
    }
    check_access(peer)
}

async fn learn(peer: SocketAddr) {
    let Ok(socket) = net::bind_udp("0.0.0.0:0").await else {
        return;
    };
    let mut buf = [0u8; 64];
    for attempt in 0..LEARN_ATTEMPTS {
        let frame = discover_frame(0xB0u8.wrapping_add(attempt));
        if socket.send_to(&frame, peer).await.is_err() {
            return;
        }
        let deadline = tokio::time::sleep(LEARN_WAIT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                r = socket.recv_from(&mut buf) => {
                    let Ok((n, from)) = r else { break };
                    if from.ip() != peer.ip() {
                        continue;
                    }
                    if matches!(
                        decode_reply(from, &buf[..n]),
                        Ok(Packet { kind: PacketKind::Ack(_), .. })
                    ) {
                        return;
                    }
                }
                _ = &mut deadline => break,
            }
        }
    }
    tracing::debug!("udp-auth: no discover reply from {peer}; sending unsigned");
}

fn stored_key(uid: u32) -> Option<Key> {
    let cfg = config::load().ok()?;
    let text = cfg.auth_key_for(uid)?;
    match unprotect_key(text) {
        Ok(key) => Some(key),
        Err(e) => {
            tracing::warn!("udp-auth: can't read the saved key for uid 0x{uid:08X}: {e:#}");
            None
        }
    }
}

pub fn provision(pico: &mut cdc::PicoSetup) -> Result<bool> {
    let uid = pico
        .unique_id_short()
        .context("reading the Pico's unique ID")?;
    let mut cfg = config::load().unwrap_or_default();
    let existing = cfg
        .auth_key_for(uid)
        .and_then(|text| unprotect_key(text).ok());
    let created = existing.is_none();
    let key = existing.unwrap_or_else(|| Zeroizing::new(random_bytes::<KEY_LEN>()));
    if created {
        cfg.set_auth_key(uid, protect_key(&key)?);
        config::save(&cfg).context("saving the pairing key")?;
    }
    match pico.set_auth_key(&key) {
        Ok(()) => Ok(true),
        Err(e) if cdc::error_has_nack_code(&e, cdc::ERR_UNKNOWN_COMMAND) => {
            if created {
                cfg.clear_auth_key(uid);
                config::save(&cfg).context("saving config")?;
            }
            Ok(false)
        }
        Err(e) => Err(e).context("SET_AUTH_KEY failed"),
    }
}

pub fn pairing_summary(paired: bool) -> &'static str {
    if paired {
        "Paired with this PC. The Pico now only accepts signed commands over Wi-Fi."
    } else {
        "This firmware can't store a pairing key, so Wi-Fi commands stay unsigned. Update the firmware to turn on pairing."
    }
}

pub fn protect_key(key: &[u8; KEY_LEN]) -> Result<String> {
    #[cfg(windows)]
    {
        let sealed = dpapi::protect(key)?;
        Ok(format!("dpapi:{}", hex::encode(sealed)))
    }
    #[cfg(not(windows))]
    {
        Ok(format!("plain:{}", hex::encode(key)))
    }
}

pub fn unprotect_key(text: &str) -> Result<Key> {
    let (scheme, body) = text
        .split_once(':')
        .ok_or_else(|| anyhow!("key has no scheme prefix"))?;
    let mut raw = Zeroizing::new(hex::decode(body).context("key is not hex")?);
    let plain = match scheme {
        #[cfg(windows)]
        "dpapi" => dpapi::unprotect(&raw)?,
        "plain" => Zeroizing::new(std::mem::take(&mut *raw)),
        other => bail!("unsupported key scheme {other:?}"),
    };
    if plain.len() != KEY_LEN {
        bail!("key is {} bytes, expected {KEY_LEN}", plain.len());
    }
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    key.copy_from_slice(&plain);
    Ok(key)
}

#[cfg(windows)]
mod dpapi {
    use anyhow::{Context, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use zeroize::{Zeroize, Zeroizing};

    fn take_blob(blob: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
        if blob.pbData.is_null() {
            return Zeroizing::new(Vec::new());
        }
        let out = unsafe { std::slice::from_raw_parts_mut(blob.pbData, blob.cbData as usize) };
        let copy = Zeroizing::new(out.to_vec());
        out.zeroize();
        unsafe {
            LocalFree(HLOCAL(blob.pbData.cast()));
        }
        copy
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).context("key too large")?,
            pbData: data.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .context("CryptProtectData failed")?;
        Ok(take_blob(output).to_vec())
    }

    pub fn unprotect(data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).context("key blob too large")?,
            pbData: data.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .context("CryptUnprotectData failed")?;
        Ok(take_blob(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const KEY: [u8; KEY_LEN] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
        0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D,
        0x1E, 0x1F,
    ];
    const NONCE: [u8; NONCE_LEN] = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];
    const STATE_FRAME: [u8; PACKET_SIZE] = [
        0xA5, 0x01, 0x07, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        0x0C, 0xB2,
    ];
    const VECTOR_COUNTER: u64 = 1_700_000_000_000_000;
    const VECTOR_TAG: &str = "fc82208219422af75d47275babed4c0479b695a36876e63a33bc8d0a0f75dba2";
    const VECTOR_ACK: [u8; PACKET_SIZE] = [
        0xA5, 0x04, 0x00, 0x17, 0x02, 0x1A, 0x0A, 0x1A, 0x01, 0x05, 0x00, 0x00, 0xB6, 0x7E, 0xD3,
        0x07, 0x27,
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

    fn ext_ack(key: &[u8; KEY_LEN], challenge: &[u8; CHALLENGE_LEN]) -> Vec<u8> {
        let tag = ack_mac(key, &NONCE, challenge, &VECTOR_ACK)
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
        let signed = sign_frame(&KEY, &NONCE, &STATE_FRAME, VECTOR_COUNTER);
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
        let base = sign_frame(&KEY, &NONCE, &STATE_FRAME, 5);
        let mut frame = STATE_FRAME;
        frame[6] ^= 1;
        assert_ne!(base[25..], sign_frame(&KEY, &NONCE, &frame, 5)[25..]);
        assert_ne!(base[25..], sign_frame(&KEY, &NONCE, &STATE_FRAME, 6)[25..]);
        let mut nonce = NONCE;
        nonce[0] ^= 1;
        assert_ne!(base[25..], sign_frame(&KEY, &nonce, &STATE_FRAME, 5)[25..]);
        let mut key = KEY;
        key[31] ^= 1;
        assert_ne!(base[25..], sign_frame(&key, &NONCE, &STATE_FRAME, 5)[25..]);
    }

    #[test]
    fn ack_tag_matches_firmware_vector() {
        let tag = ack_mac(&KEY, &NONCE, &VECTOR_CHALLENGE, &VECTOR_ACK)
            .finalize()
            .into_bytes();
        assert_eq!(hex::encode(tag), VECTOR_ACK_TAG);
    }

    #[test]
    fn counter_strictly_increases_even_when_the_clock_stalls_or_rewinds() {
        let counter = AtomicU64::new(0);
        let a = advance_counter(&counter, 1_000);
        let b = advance_counter(&counter, 1_000);
        let c = advance_counter(&counter, 10);
        let d = advance_counter(&counter, 5_000);
        assert_eq!(a, 1_000);
        assert_eq!(b, 1_001);
        assert_eq!(c, 1_002);
        assert_eq!(d, 5_000);
        let e = advance_counter(&counter, 0);
        assert!(e > d);
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
        let frame = discover_frame_with(9, &VECTOR_CHALLENGE);
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
            .accept_reply(ip(), &ext_ack(&KEY, &VECTOR_CHALLENGE), &mut with_key)
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
        s.accept_reply(ip(), &ext_ack(&KEY, &VECTOR_CHALLENGE), &mut with_key)
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
        s.accept_reply(ip(), &ext_ack(&other, &VECTOR_CHALLENGE), &mut with_key)
            .unwrap();
        assert!(s.check_access(peer()).is_err());
    }

    #[test]
    fn keyed_pico_without_a_saved_key_locks_the_peer() {
        let mut s = Sessions::default();
        s.remember_challenge(VECTOR_CHALLENGE);
        s.accept_reply(ip(), &ext_ack(&KEY, &VECTOR_CHALLENGE), &mut |_| None)
            .unwrap();
        assert!(s.check_access(peer()).is_err());
    }

    #[test]
    fn tampered_ack_body_fails_verification() {
        let mut s = Sessions::default();
        s.remember_challenge(VECTOR_CHALLENGE);
        let mut buf = ext_ack(&KEY, &VECTOR_CHALLENGE);
        buf[9] ^= 0x01;
        buf[16] = protocol::crc8(&buf[..16]);
        s.accept_reply(ip(), &buf, &mut with_key).unwrap();
        assert!(s.check_access(peer()).is_err());
    }

    #[test]
    fn open_pico_ack_clears_a_stale_pairing() {
        let mut s = Sessions::default();
        s.remember_challenge(VECTOR_CHALLENGE);
        s.accept_reply(ip(), &ext_ack(&KEY, &VECTOR_CHALLENGE), &mut with_key)
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
        s.accept_reply(ip(), &ext_ack(&KEY, &VECTOR_CHALLENGE), &mut with_key)
            .unwrap();
        let before = s.seal(ip(), &STATE_FRAME, 10);

        let rebooted = [0x99; NONCE_LEN];
        let tag = ack_mac(&KEY, &rebooted, &VECTOR_CHALLENGE, &VECTOR_ACK)
            .finalize()
            .into_bytes();
        let mut buf = VECTOR_ACK.to_vec();
        buf.extend_from_slice(&rebooted);
        buf.extend_from_slice(&tag);
        s.accept_reply(ip(), &buf, &mut with_key).unwrap();
        let after = s.seal(ip(), &STATE_FRAME, 10);
        assert_ne!(&*before, &*after);
        assert_eq!(*after, sign_frame(&KEY, &rebooted, &STATE_FRAME, 10));
    }

    #[test]
    fn challenge_ring_forgets_old_challenges() {
        let mut s = Sessions::default();
        s.remember_challenge(VECTOR_CHALLENGE);
        for i in 0..CHALLENGE_RING as u8 {
            s.remember_challenge([i; CHALLENGE_LEN]);
        }
        s.accept_reply(ip(), &ext_ack(&KEY, &VECTOR_CHALLENGE), &mut with_key)
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
}
