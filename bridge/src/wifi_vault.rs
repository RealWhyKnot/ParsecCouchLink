//! On-disk vault for the host's Wi-Fi credentials, used by lab-mode so
//! the remote operator never has to receive (or store) the host's
//! password to push it to the Pico.
//!
//! Design choices and threat model
//! --------------------------------
//!
//! * **At-rest encryption**: Windows DPAPI (`CryptProtectData`) at the
//!   CurrentUser scope. Only the host's own Windows login can decrypt
//!   the blob; copying the file to another machine or another user
//!   account yields nothing.
//! * **Wire transparency**: the cleartext SSID and password never leave
//!   the host. Lab-mode's `wifi_apply_saved` command decrypts the blob
//!   in memory, hands the bytes to `pico.set_wifi(...)`, zeroizes the
//!   buffer, and emits a `WifiResult` that names the SSID (so the
//!   operator can confirm) but never the password.
//! * **Scope**: this vault stores exactly one (SSID, password) pair.
//!   Replaces, doesn't merge. The host can wipe it at any time with
//!   `couchlink save-wifi --clear`.
//! * **Optional**: lab-mode works fine without a saved blob. The
//!   tunnel will refuse `wifi_apply_saved` with an explicit "no
//!   creds saved on the host" message and the operator can fall back
//!   to a one-off prompt-driven flow via `couchlink configure-wifi`
//!   on the host's terminal.
//!
//! The vault is intentionally not a general "remembered credential"
//! feature for the rest of the bridge. Setup mode and `configure-wifi`
//! keep their interactive-prompt flow so a non-lab user is never
//! tricked into thinking their password is being saved.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use zeroize::Zeroize;

use crate::config;

/// Hard cap matches `flash_creds_t` on the firmware side.
pub const SSID_MAX: usize = 32;
/// Hard cap matches `flash_creds_t` on the firmware side.
pub const PASSWORD_MAX: usize = 63;

/// File written into the per-user config directory. DPAPI-encrypted;
/// not portable between Windows logins or machines.
const VAULT_FILENAME: &str = "lab-wifi.bin";

/// Wire format inside the encrypted blob:
///   byte 0:        version (1)
///   byte 1:        ssid_len (1..=32)
///   bytes 2..n:    ssid utf-8 (no null)
///   byte n:        pass_len (0..=63)
///   bytes n+1..m:  password utf-8 (no null)
const FORMAT_VERSION: u8 = 1;

pub fn vault_path() -> Result<PathBuf> {
    Ok(config::config_dir()?.join(VAULT_FILENAME))
}

/// True if a saved vault exists at the canonical path.
pub fn exists() -> bool {
    vault_path().map(|p| p.exists()).unwrap_or(false)
}

/// Delete the vault file. No-op (Ok) if it does not exist.
pub fn clear() -> Result<()> {
    let p = vault_path()?;
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", p.display())),
    }
}

/// Encrypt and persist `(ssid, password)`. The caller is expected to
/// hold the password in a buffer that will be `Zeroize`d after this
/// returns; we zeroize the intermediate plaintext-blob buffer we build
/// internally on every path.
pub fn save(ssid: &str, password: &str) -> Result<()> {
    if ssid.is_empty() || ssid.len() > SSID_MAX {
        bail!("ssid length {} outside 1..={SSID_MAX}", ssid.len());
    }
    if password.len() > PASSWORD_MAX {
        bail!("password length {} exceeds {PASSWORD_MAX}", password.len());
    }

    let mut plaintext: Vec<u8> = Vec::with_capacity(2 + ssid.len() + 1 + password.len());
    plaintext.push(FORMAT_VERSION);
    plaintext.push(ssid.len() as u8);
    plaintext.extend_from_slice(ssid.as_bytes());
    plaintext.push(password.len() as u8);
    plaintext.extend_from_slice(password.as_bytes());

    let encrypted = dpapi_protect(&plaintext);
    plaintext.zeroize();
    let encrypted = encrypted?;

    let path = vault_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, &encrypted).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Decrypt the vault. Returns the SSID (cheap to clone, often logged
/// just by name) plus a `Zeroizing<String>` password the caller must
/// drop promptly. Returns `Ok(None)` if there is no vault file --
/// callers can use this to differentiate "no creds saved" from "decrypt
/// failed because the wrong login is trying to read."
pub fn load() -> Result<Option<LoadedCreds>> {
    let path = vault_path()?;
    let encrypted = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    let mut plaintext = dpapi_unprotect(&encrypted)?;
    let parsed = parse(&plaintext);
    plaintext.zeroize();
    let parsed = parsed.context("vault plaintext malformed")?;
    Ok(Some(parsed))
}

/// Tuple-like result type so callers can pull the SSID out by-value
/// while keeping the password under `Zeroizing` discipline.
pub struct LoadedCreds {
    pub ssid: String,
    pub password: zeroize::Zeroizing<String>,
}

fn parse(plaintext: &[u8]) -> Result<LoadedCreds> {
    if plaintext.len() < 3 {
        bail!("plaintext too short ({} bytes)", plaintext.len());
    }
    if plaintext[0] != FORMAT_VERSION {
        bail!("unknown vault format version {}", plaintext[0]);
    }
    let ssid_len = plaintext[1] as usize;
    if ssid_len == 0 || ssid_len > SSID_MAX {
        bail!("ssid_len {ssid_len} outside 1..={SSID_MAX}");
    }
    if plaintext.len() < 2 + ssid_len + 1 {
        bail!(
            "plaintext truncated -- need at least {} bytes for header+ssid+passlen",
            2 + ssid_len + 1
        );
    }
    let ssid_bytes = &plaintext[2..2 + ssid_len];
    let ssid = std::str::from_utf8(ssid_bytes)
        .map_err(|e| anyhow!("ssid is not valid UTF-8: {e}"))?
        .to_string();
    let pass_len = plaintext[2 + ssid_len] as usize;
    if pass_len > PASSWORD_MAX {
        bail!("pass_len {pass_len} exceeds {PASSWORD_MAX}");
    }
    let pass_start = 2 + ssid_len + 1;
    if plaintext.len() != pass_start + pass_len {
        bail!(
            "plaintext length {} does not match declared payload {}",
            plaintext.len(),
            pass_start + pass_len
        );
    }
    let pass_bytes = &plaintext[pass_start..pass_start + pass_len];
    let password = std::str::from_utf8(pass_bytes)
        .map_err(|e| anyhow!("password is not valid UTF-8: {e}"))?
        .to_string();
    Ok(LoadedCreds {
        ssid,
        password: zeroize::Zeroizing::new(password),
    })
}

// ---- DPAPI wrappers --------------------------------------------------

#[cfg(windows)]
fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let ok = unsafe {
        CryptProtectData(
            &in_blob,
            None,
            None,
            None,
            None,
            0, // CurrentUser scope (no CRYPTPROTECT_LOCAL_MACHINE)
            &mut out_blob,
        )
    };
    ok.ok().context("CryptProtectData")?;
    if out_blob.pbData.is_null() || out_blob.cbData == 0 {
        bail!("CryptProtectData returned an empty buffer");
    }
    let len = out_blob.cbData as usize;
    let slice = unsafe { std::slice::from_raw_parts(out_blob.pbData, len) };
    let out = slice.to_vec();
    unsafe {
        let _ = LocalFree(HLOCAL(out_blob.pbData.cast()));
    }
    Ok(out)
}

#[cfg(windows)]
fn dpapi_unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let ok = unsafe { CryptUnprotectData(&in_blob, None, None, None, None, 0, &mut out_blob) };
    ok.ok()
        .context("CryptUnprotectData (wrong Windows login? vault corrupted?)")?;
    if out_blob.pbData.is_null() || out_blob.cbData == 0 {
        bail!("CryptUnprotectData returned an empty buffer");
    }
    let len = out_blob.cbData as usize;
    let slice = unsafe { std::slice::from_raw_parts(out_blob.pbData, len) };
    let out = slice.to_vec();
    // Zero the LocalAlloc'd plaintext before LocalFree releases it back
    // to the heap. Defense in depth -- LocalFree does not zero.
    unsafe {
        std::ptr::write_bytes(out_blob.pbData, 0, len);
        let _ = LocalFree(HLOCAL(out_blob.pbData.cast()));
    }
    Ok(out)
}

#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>> {
    bail!("wifi_vault: DPAPI is Windows-only")
}

#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>> {
    bail!("wifi_vault: DPAPI is Windows-only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip_in_memory() {
        let payload = b"\x01\x05home2\x08hunter12";
        let cipher = dpapi_protect(payload).expect("protect");
        assert_ne!(
            cipher.as_slice(),
            payload.as_slice(),
            "ciphertext == plaintext"
        );
        let back = dpapi_unprotect(&cipher).expect("unprotect");
        assert_eq!(&back, payload, "round-trip changed bytes");
    }

    #[test]
    fn parse_round_trips_known_payload() {
        let mut buf = Vec::new();
        buf.push(FORMAT_VERSION);
        buf.push(5); // ssid_len
        buf.extend_from_slice(b"home2");
        buf.push(8); // pass_len
        buf.extend_from_slice(b"hunter12");
        let parsed = parse(&buf).expect("parse ok");
        assert_eq!(parsed.ssid, "home2");
        assert_eq!(parsed.password.as_str(), "hunter12");
    }

    #[test]
    fn parse_rejects_short_buffer() {
        let buf = [FORMAT_VERSION, 5, b'h'];
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn parse_rejects_oversized_ssid() {
        let buf = [FORMAT_VERSION, 200u8, 0, 0];
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let mut buf = Vec::new();
        buf.push(99); // bogus version
        buf.push(4);
        buf.extend_from_slice(b"home");
        buf.push(0);
        assert!(parse(&buf).is_err());
    }
}
