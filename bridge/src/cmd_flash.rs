//! `couchlink flash [--uf2 <path>]` -- find a Pico in BOOTSEL mode and
//! copy the matching UF2 onto it.
//!
//! BOOTSEL is the boot-loader mode entered by holding the BOOTSEL button
//! while applying USB power; the Pico enumerates as a USB mass-storage
//! device named `RPI-RP2` (RP2040) or `RP2350` (RP2350). The bootloader
//! is per-chip-family: an RP2040 image flashed onto an RP2350 (or vice
//! versa) is silently rejected after reset, so we auto-pick the matching
//! file based on which drive label appeared.
//!
//! When `--uf2` is omitted, the canonical filename for the detected
//! board is resolved next to the running .exe (then in `dist/` next to
//! it, then in CWD). When `--uf2 <file>` is given, that file is used
//! as-is. When `--uf2 <dir>` is given, the canonical filename is
//! resolved within that directory.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

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

/// Which Pico chip family a BOOTSEL drive belongs to. Determines which
/// UF2 the bootloader will accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootselBoard {
    /// Original Pico / Pico W -- Cortex-M0+, mounts as `RPI-RP2`.
    Rp2040,
    /// Pico 2 / Pico 2 W -- Cortex-M33, mounts as `RP2350`.
    Rp2350,
}

impl BootselBoard {
    /// Canonical release-side filename for this board's UF2.
    pub fn canonical_uf2(self) -> &'static str {
        match self {
            BootselBoard::Rp2040 => "couchlink-picow.uf2",
            BootselBoard::Rp2350 => "couchlink-pico2w.uf2",
        }
    }

    /// Human label for log lines.
    pub fn label(self) -> &'static str {
        match self {
            BootselBoard::Rp2040 => "Pico W / WH (RP2040)",
            BootselBoard::Rp2350 => "Pico 2 W (RP2350)",
        }
    }

    /// UF2 file family ID that this board's bootloader accepts.
    /// RP2350 has three variants (secure / non-secure / RISC-V); all
    /// are acceptable for our needs since we ship the secure ARM build.
    pub fn accepts_family(self, family_id: u32) -> bool {
        const RP2040: u32 = 0xE48BFF56;
        const RP2350_ARM_S: u32 = 0xE48BFF59;
        const RP2350_ARM_NS: u32 = 0xE48BFF5A;
        const RP2350_RISCV: u32 = 0xE48BFF5B;
        const ABSOLUTE: u32 = 0xE48BFF57; // universal; any chip accepts it
        match self {
            BootselBoard::Rp2040 => matches!(family_id, RP2040 | ABSOLUTE),
            BootselBoard::Rp2350 => {
                matches!(
                    family_id,
                    RP2350_ARM_S | RP2350_ARM_NS | RP2350_RISCV | ABSOLUTE
                )
            }
        }
    }
}

pub async fn run(uf2: Option<PathBuf>) -> Result<()> {
    println!("Hold the BOOTSEL button on the Pico and plug it into this PC now.");
    println!("Looking for an RPI-RP2 or RP2350 drive (60 s timeout)...");

    tracing::info!("flash: waiting for BOOTSEL drive (RPI-RP2 or RP2350)...");
    let start = Instant::now();
    let (mount, board) = wait_for_bootsel_mount(Duration::from_secs(60))
        .await
        .inspect_err(|_| {
            tracing::error!("flash: timeout after 60s -- no BOOTSEL drive observed")
        })?;
    let elapsed = start.elapsed().as_secs();
    tracing::info!(
        "flash: BOOTSEL drive {} mounted as {} after {} s",
        board.label(),
        mount.display(),
        elapsed,
    );
    println!("Detected {} at {}", board.label(), mount.display());

    let uf2_path =
        resolve_uf2_path(uf2.as_deref(), board).context("resolving which UF2 to flash")?;
    println!("Using firmware: {}", uf2_path.display());
    // Log after resolve so the path in the file is always the final resolved one.
    tracing::info!(
        "flash: using UF2 path={} source={}",
        uf2_path.display(),
        if uf2.is_some() {
            "override"
        } else {
            "auto-pick"
        },
    );

    // Optional pre-flight: peek the UF2 header and warn if the family ID
    // does not match the BOOTSEL board. We warn rather than block so
    // operators with custom builds can still force a write.
    match read_uf2_family_id(&uf2_path).await {
        Ok(Some(family)) => {
            if board.accepts_family(family) {
                tracing::debug!(
                    "flash: family-id ok (0x{:08X} accepted by {})",
                    family,
                    board.label()
                );
            } else {
                println!(
                    "  warning: UF2 family ID 0x{:08X} does not match the detected board {}. \
                     The bootloader will likely reject it after reset.",
                    family,
                    board.label(),
                );
                tracing::warn!(
                    "flash: family-id mismatch -- board={} got 0x{:08X}",
                    board.label(),
                    family,
                );
            }
        }
        Ok(None) => {
            // Not a recognizable UF2 magic; let the copy proceed and the
            // bootloader complain if it really is malformed.
        }
        Err(e) => {
            tracing::debug!("flash: family-id peek failed: {e}");
        }
    }

    println!("Copying {} -> {} ...", uf2_path.display(), mount.display());

    // The bootloader reboots the Pico as soon as the file copy finishes,
    // so the OS may return EOF or a write error on the very last block.
    // Treat a partial-write error after the drive vanished as success.
    let dest_name = uf2_path
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "couchlink-pico.uf2".into());
    let dest = mount.join(&dest_name);

    let bytes = tokio::fs::read(&uf2_path)
        .await
        .with_context(|| format!("reading {}", uf2_path.display()))?;
    let n = bytes.len();
    match tokio::fs::write(&dest, bytes).await {
        Ok(()) => {
            tracing::info!("flash: copied {} bytes to {}", n, dest.display());
            println!(
                "Wrote {} bytes. The Pico should now reboot into the new firmware.",
                n,
            );
        }
        Err(e) => {
            if !mount.exists() {
                tracing::info!(
                    "flash: drive vanished after copy -- treating as success, Pico is rebooting"
                );
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
    cfg.last_uf2 = Some(uf2_path);
    let _ = crate::config::save(&cfg);

    Ok(())
}

/// Resolve which UF2 file to flash, given an optional user override and
/// the detected BOOTSEL board.
fn resolve_uf2_path(uf2: Option<&Path>, board: BootselBoard) -> Result<PathBuf> {
    if let Some(p) = uf2 {
        if !p.exists() {
            bail!("UF2 path not found: {}", p.display());
        }
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        if p.is_dir() {
            let candidate = p.join(board.canonical_uf2());
            if candidate.exists() {
                return Ok(candidate);
            }
            // Legacy fallback for older release folders: a single
            // unsuffixed `couchlink-pico.uf2` used to mean Pico 2 W.
            if board == BootselBoard::Rp2350 {
                let legacy = p.join("couchlink-pico.uf2");
                if legacy.exists() {
                    return Ok(legacy);
                }
            }
            bail!(
                "no UF2 named {} in directory {}",
                board.canonical_uf2(),
                p.display(),
            );
        }
        bail!("UF2 path is neither file nor directory: {}", p.display());
    }

    // No override -- search canonical locations.
    auto_pick_uf2(board)
}

/// Search the exe's directory, `dist/` next to it, CWD, and `./dist/`
/// for the board's canonical UF2 filename. Returns the first hit.
fn auto_pick_uf2(board: BootselBoard) -> Result<PathBuf> {
    let canonical = board.canonical_uf2();
    let legacy = "couchlink-pico.uf2"; // older single-board name = Pico 2 W only

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|q| q.to_path_buf()));
    let cwd = std::env::current_dir().ok();

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir.as_ref() {
        search_dirs.push(d.clone());
        search_dirs.push(d.join("dist"));
    }
    if let Some(d) = cwd.as_ref() {
        search_dirs.push(d.clone());
        search_dirs.push(d.join("dist"));
    }

    for dir in &search_dirs {
        let candidate = dir.join(canonical);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    // Legacy fallback for old release layouts (only meaningful for Pico 2 W).
    if board == BootselBoard::Rp2350 {
        for dir in &search_dirs {
            let candidate = dir.join(legacy);
            if candidate.exists() {
                tracing::info!(
                    "using legacy filename {} as the Pico 2 W UF2 (release predates dual-board layout)",
                    candidate.display(),
                );
                return Ok(candidate);
            }
        }
    }

    let searched: Vec<String> = search_dirs
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    Err(anyhow!(
        "could not find {} in any of: {}. Pass --uf2 <path> to specify the firmware location.",
        canonical,
        searched.join(", "),
    ))
}

/// Read the family ID from the first UF2 block. Returns `Ok(None)` if
/// the file is not a recognizable UF2 (let the bootloader complain
/// instead of blocking the operator here).
async fn read_uf2_family_id(path: &Path) -> Result<Option<u32>> {
    // UF2 block layout: first u32 is start-magic, second is end-magic,
    // family ID lives at offset 28.
    const UF2_MAGIC_START: u32 = 0x0A324655;
    const UF2_MAGIC_START2: u32 = 0x9E5D5157;
    let mut buf = [0u8; 32];
    let mut f = tokio::fs::File::open(path).await?;
    use tokio::io::AsyncReadExt;
    if f.read_exact(&mut buf).await.is_err() {
        return Ok(None);
    }
    let m0 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let m1 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if m0 != UF2_MAGIC_START || m1 != UF2_MAGIC_START2 {
        return Ok(None);
    }
    let family = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]);
    Ok(Some(family))
}

pub async fn wait_for_bootsel_mount(timeout: Duration) -> Result<(PathBuf, BootselBoard)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some((p, b)) = find_bootsel_mount() {
            return Ok((p, b));
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
fn find_bootsel_mount() -> Option<(PathBuf, BootselBoard)> {
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
        let board = match label.as_str() {
            "RPI-RP2" => BootselBoard::Rp2040,
            "RP2350" => BootselBoard::Rp2350,
            _ => continue,
        };
        return Some((PathBuf::from(root), board));
    }
    None
}

#[cfg(not(windows))]
fn find_bootsel_mount() -> Option<(PathBuf, BootselBoard)> {
    None
}
