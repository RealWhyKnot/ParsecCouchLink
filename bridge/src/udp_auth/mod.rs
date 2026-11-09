use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::protocol::{self, DecodeError, PACKET_SIZE, Packet, PacketKind};
use crate::{cdc, config, net};

pub(crate) const KEY_LEN: usize = 32;
pub(crate) const NONCE_LEN: usize = 8;
pub(crate) const COUNTER_LEN: usize = 8;
pub(crate) const TAG_LEN: usize = 32;
pub(crate) const CHALLENGE_LEN: usize = 8;
pub(crate) const SIGNED_LEN: usize = PACKET_SIZE + COUNTER_LEN + TAG_LEN;
pub(crate) const ACK_EXT_LEN: usize = PACKET_SIZE + NONCE_LEN + TAG_LEN;

const DOMAIN_FRAME: u8 = 0x01;
const DOMAIN_ACK: u8 = 0x02;
const DISCOVER_MARKER: u8 = 0x02;
const CHALLENGE_RING: usize = 16;
const LEARN_ATTEMPTS: u8 = 3;
const LEARN_WAIT: Duration = Duration::from_millis(500);

pub(crate) const RECOVERY_HINT: &str = "This PC isn't paired with that Pico, so it only takes the switch to setup mode in its first two minutes after power-up. If nothing happens, unplug the Pico, plug it back in and try again.";

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
            if let PacketKind::Ack(info) = packet.kind
                && info.proto_version == protocol::PROTO_VERSION_OPEN
            {
                self.peers.insert(ip, PeerAuth::Open);
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
            .expect("an extended ack holds a full nonce");
        let tag = &buf[PACKET_SIZE + NONCE_LEN..];
        let state = match lookup(uid) {
            Some(key)
                if self
                    .challenges
                    .iter()
                    .any(|c| ack_tag_matches(&key, nonce, *c, ack, tag)) =>
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
                Sealed::signed(sign_frame(key, *nonce, frame, counter))
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

pub(crate) struct Sealed {
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
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

pub(crate) fn sign_frame(
    key: &[u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
    frame: &[u8; PACKET_SIZE],
    counter: u64,
) -> [u8; SIGNED_LEN] {
    let mut out = [0u8; SIGNED_LEN];
    out[..PACKET_SIZE].copy_from_slice(frame);
    out[PACKET_SIZE..PACKET_SIZE + COUNTER_LEN].copy_from_slice(&counter.to_le_bytes());
    let mut mac = hmac_with(key);
    mac.update(&[DOMAIN_FRAME]);
    mac.update(&nonce);
    mac.update(&out[..PACKET_SIZE + COUNTER_LEN]);
    out[PACKET_SIZE + COUNTER_LEN..].copy_from_slice(&mac.finalize().into_bytes());
    out
}

fn ack_mac(
    key: &[u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
    challenge: [u8; CHALLENGE_LEN],
    ack: &[u8],
) -> HmacSha256 {
    let mut mac = hmac_with(key);
    mac.update(&[DOMAIN_ACK]);
    mac.update(&nonce);
    mac.update(&challenge);
    mac.update(ack);
    mac
}

fn ack_tag_matches(
    key: &[u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
    challenge: [u8; CHALLENGE_LEN],
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
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
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

pub(crate) fn next_counter() -> u64 {
    advance_counter(&COUNTER, unix_micros())
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).expect("OS random source unavailable");
    out
}

fn discover_frame_with(seq: u8, challenge: [u8; CHALLENGE_LEN]) -> [u8; PACKET_SIZE] {
    let mut buf = Packet::discover(seq).encode();
    buf[4] = DISCOVER_MARKER;
    buf[5..5 + CHALLENGE_LEN].copy_from_slice(&challenge);
    buf[16] = protocol::crc8(&buf[..16]);
    buf
}

pub(crate) fn discover_frame(seq: u8) -> [u8; PACKET_SIZE] {
    let challenge = random_bytes::<CHALLENGE_LEN>();
    sessions().remember_challenge(challenge);
    discover_frame_with(seq, challenge)
}

pub(crate) fn decode_reply(peer: SocketAddr, buf: &[u8]) -> Result<Packet, DecodeError> {
    sessions().accept_reply(peer.ip(), buf, &mut stored_key)
}

pub(crate) fn seal(peer: SocketAddr, frame: &[u8; PACKET_SIZE]) -> Sealed {
    sessions().seal(peer.ip(), frame, next_counter())
}

pub(crate) fn check_access(peer: SocketAddr) -> Result<()> {
    sessions().check_access(peer)
}

pub(crate) async fn prepare(peer: SocketAddr) -> Result<()> {
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
                () = &mut deadline => break,
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

pub(crate) fn provision(pico: &mut cdc::PicoSetup) -> Result<bool> {
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

pub(crate) fn pairing_summary(paired: bool) -> &'static str {
    if paired {
        "Paired with this PC. The Pico now only accepts signed commands over Wi-Fi."
    } else {
        "This firmware can't store a pairing key, so Wi-Fi commands stay unsigned. Update the firmware to turn on pairing."
    }
}

pub(crate) fn protect_key(key: &[u8; KEY_LEN]) -> Result<String> {
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

pub(crate) fn unprotect_key(text: &str) -> Result<Key> {
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
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;
    use zeroize::{Zeroize, Zeroizing};

    fn take_blob(blob: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
        if blob.pbData.is_null() {
            return Zeroizing::new(Vec::new());
        }
        // SAFETY: DPAPI returned `pbData` with `cbData` bytes and it is non-null here.
        let out = unsafe {
            std::slice::from_raw_parts_mut(blob.pbData, crate::convert::usize_from_u32(blob.cbData))
        };
        let copy = Zeroizing::new(out.to_vec());
        out.zeroize();
        // SAFETY: DPAPI allocated `pbData` with LocalAlloc and it is freed once, after its last use.
        unsafe {
            LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        copy
    }

    pub(super) fn protect(data: &[u8]) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).context("key too large")?,
            pbData: data.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `data` for the whole call and `output` is a live blob.
        unsafe {
            CryptProtectData(
                &raw const input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut output,
            )
        }
        .context("CryptProtectData failed")?;
        Ok(take_blob(output).to_vec())
    }

    pub(super) fn unprotect(data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).context("key blob too large")?,
            pbData: data.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `data` for the whole call and `output` is a live blob.
        unsafe {
            CryptUnprotectData(
                &raw const input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut output,
            )
        }
        .context("CryptUnprotectData failed")?;
        Ok(take_blob(output))
    }
}

#[cfg(test)]
mod tests;
