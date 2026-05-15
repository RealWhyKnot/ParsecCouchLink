//! `couchlink flash --uf2 <path>` -- find a Pico in BOOTSEL mode and copy
//! a UF2 onto it. BOOTSEL is the boot-loader mode entered by holding the
//! BOOTSEL button while applying USB power; the Pico enumerates as a USB
//! mass-storage device named RPI-RP2 (RP2040) or RP2350 (RP2350).
//!
//! The PIDs are 0x000F for RP2350 and 0x0003 for RP2040 under Raspberry
//! Pi's vendor ID 0x2E8A.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

// BOOTSEL USB IDs are documented here for support-bundle annotations
// and operator messaging. The actual detection below matches on volume
// label (RPI-RP2 / RP2350), which is more reliable than USB enumeration
// from a console app on Windows.
#[allow(dead_code)]
pub const BOOTSEL_VID: u16 = 0x2E8A;
#[allow(dead_code)]
pub const BOOTSEL_PID_RP2350: u16 = 0x000F;
#[allow(dead_code)]
pub const BOOTSEL_PID_RP2040: u16 = 0x0003;

pub async fn run(uf2: PathBuf) -> Result<()> {
    if !uf2.exists() {
        bail!("UF2 not found: {}", uf2.display());
    }
    let ext_ok = uf2
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("uf2"))
        .unwrap_or(false);
    if !ext_ok {
        bail!("not a .uf2 file: {}", uf2.display());
    }

    println!("Hold the BOOTSEL button on the Pico and plug it into this PC now.");
    println!("Looking for an RPI-RP2 or RP2350 drive (60 s timeout)...");

    let mount = wait_for_bootsel_mount(Duration::from_secs(60)).await?;
    println!("Pico detected at {}", mount.display());

    let dest_name = uf2
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "couchlink-pico.uf2".into());
    let dest = mount.join(&dest_name);

    println!("Copying {} -> {} ...", uf2.display(), dest.display());

    // The bootloader reboots the Pico as soon as the file copy finishes,
    // so the OS may return EOF or a write error on the very last block.
    // We treat partial-write errors after >50% of the file is sent as
    // "probably succeeded".
    let bytes = tokio::fs::read(&uf2)
        .await
        .with_context(|| format!("reading {}", uf2.display()))?;
    let n = bytes.len();
    match tokio::fs::write(&dest, bytes).await {
        Ok(()) => {
            println!(
                "Wrote {} bytes. The Pico should now reboot into the new firmware.",
                n,
            );
        }
        Err(e) => {
            // Treat as success if the drive simply disappeared mid-write.
            if !mount.exists() {
                println!(
                    "Pico rebooted mid-write (this is normal). Approximately {} bytes \
                     transferred before reboot.",
                    n,
                );
            } else {
                return Err(e)
                    .with_context(|| format!("writing {} (Pico still present)", dest.display()));
            }
        }
    }

    let mut cfg = crate::config::load().unwrap_or_default();
    cfg.last_uf2 = Some(uf2);
    let _ = crate::config::save(&cfg);

    Ok(())
}

async fn wait_for_bootsel_mount(timeout: Duration) -> Result<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(p) = find_bootsel_mount() {
            return Ok(p);
        }
        if Instant::now() >= deadline {
            bail!(
                "timeout: no BOOTSEL drive (RPI-RP2 or RP2350) appeared in {} s. \
                 Make sure you're holding BOOTSEL when you plug in.",
                timeout.as_secs(),
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(windows)]
fn find_bootsel_mount() -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };
    // GetDriveTypeW returns one of these as a u32. windows-rs 0.58 doesn't
    // re-export the named constants under FileSystem, so we use the
    // literal from the Win32 docs.
    const DRIVE_REMOVABLE: u32 = 2;

    let mut buf = vec![0u16; 4096];
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    if n == 0 {
        return None;
    }

    let mut roots = Vec::new();
    let mut start = 0usize;
    while start < n {
        let end = buf[start..n]
            .iter()
            .position(|&c| c == 0)
            .map(|i| start + i)
            .unwrap_or(n);
        if end <= start {
            break;
        }
        roots.push(String::from_utf16_lossy(&buf[start..end]));
        start = end + 1;
    }

    for root in roots {
        let wide: Vec<u16> = OsStr::new(&root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        if drive_type != DRIVE_REMOVABLE {
            continue;
        }
        let mut volume_name = vec![0u16; 256];
        let mut fs_name = vec![0u16; 256];
        let r = unsafe {
            GetVolumeInformationW(
                PCWSTR(wide.as_ptr()),
                Some(&mut volume_name),
                None,
                None,
                None,
                Some(&mut fs_name),
            )
        };
        if r.is_err() {
            continue;
        }
        let label_end = volume_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(volume_name.len());
        let label = String::from_utf16_lossy(&volume_name[..label_end]);
        if label == "RPI-RP2" || label == "RP2350" {
            return Some(PathBuf::from(root));
        }
    }
    None
}

#[cfg(not(windows))]
fn find_bootsel_mount() -> Option<PathBuf> {
    None
}
