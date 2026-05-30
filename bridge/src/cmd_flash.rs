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

use crate::cdc;

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

    /// Map an INFO_UF2.TXT `Board-ID:` value (case sensitive, as
    /// written by the bootrom) to a `BootselBoard`. Returns `None`
    /// for unrecognized board IDs -- which can happen on OTP
    /// white-labelled RP2350 devices that override the field.
    pub fn from_info_uf2_board_id(s: &str) -> Option<Self> {
        match s.trim() {
            "RPI-RP2" => Some(BootselBoard::Rp2040),
            "RP2350" => Some(BootselBoard::Rp2350),
            _ => None,
        }
    }

    /// UF2 file family ID that this board's bootloader accepts.
    /// RP2350 has three variants (secure / non-secure / RISC-V); all
    /// are acceptable for our needs since we ship the secure ARM build.
    pub fn accepts_family(self, family_id: u32) -> bool {
        match self {
            BootselBoard::Rp2040 => matches!(family_id, FAMILY_RP2040 | FAMILY_ABSOLUTE),
            BootselBoard::Rp2350 => {
                matches!(
                    family_id,
                    FAMILY_RP2350_ARM_S
                        | FAMILY_RP2350_ARM_NS
                        | FAMILY_RP2350_RISCV
                        | FAMILY_ABSOLUTE
                )
            }
        }
    }
}

// UF2 family IDs from microsoft/uf2 `uf2families.json`. ABSOLUTE is the
// universal family any chip's boot ROM accepts.
const FAMILY_RP2040: u32 = 0xE48BFF56;
const FAMILY_ABSOLUTE: u32 = 0xE48BFF57;
const FAMILY_DATA: u32 = 0xE48BFF58;
const FAMILY_RP2350_ARM_S: u32 = 0xE48BFF59;
const FAMILY_RP2350_ARM_NS: u32 = 0xE48BFF5A;
const FAMILY_RP2350_RISCV: u32 = 0xE48BFF5B;

/// Short human label for a UF2 family ID. Used in operator-facing
/// error messages so a mismatch reads naturally.
fn family_name(family_id: u32) -> &'static str {
    match family_id {
        FAMILY_RP2040 => "rp2040",
        FAMILY_ABSOLUTE => "absolute",
        FAMILY_DATA => "data",
        FAMILY_RP2350_ARM_S => "rp2350-arm-s",
        FAMILY_RP2350_ARM_NS => "rp2350-arm-ns",
        FAMILY_RP2350_RISCV => "rp2350-riscv",
        _ => "unknown",
    }
}

/// Structured result of a flash operation. Returned by the in-process
/// `flash_uf2_to_bootsel` core so callers get a typed answer instead
/// of parsing stdout.
#[allow(dead_code)] // returned for tests and future local automation
#[derive(Clone, Debug)]
pub struct FlashOutcome {
    pub board: BootselBoard,
    pub mount: PathBuf,
    pub uf2_path: PathBuf,
    pub bytes_written: usize,
    pub wait_seconds: u64,
    /// True if the drive vanished mid-write (the Pico rebooted into the
    /// freshly-flashed firmware). This is the expected happy path on a
    /// fast machine and is not an error.
    pub rebooted_during_copy: bool,
}

/// Core flash sequence: wait for BOOTSEL drive, validate UF2 family,
/// copy. Logs progress via tracing so the terminal and rotating log
/// see the same operator-readable timeline.
///
/// `uf2` is the file to copy -- already resolved by the caller.
/// `wait_timeout` bounds the BOOTSEL-drive scan.
#[allow(dead_code)]
pub async fn flash_uf2_to_bootsel(uf2: &Path, wait_timeout: Duration) -> Result<FlashOutcome> {
    tracing::info!("flash: waiting for BOOTSEL drive (RPI-RP2 or RP2350)...");
    let start = Instant::now();
    let (mount, board) = wait_for_bootsel_mount(wait_timeout)
        .await
        .inspect_err(|_| {
            tracing::error!(
                "flash: timeout after {}s -- no BOOTSEL drive observed",
                wait_timeout.as_secs()
            )
        })?;
    let wait_seconds = start.elapsed().as_secs();
    tracing::info!(
        "flash: BOOTSEL drive {} mounted as {} after {} s",
        board.label(),
        mount.display(),
        wait_seconds,
    );

    flash_uf2_to_mount(uf2, mount, board, wait_seconds).await
}

async fn flash_uf2_to_mount(
    uf2: &Path,
    mount: PathBuf,
    board: BootselBoard,
    wait_seconds: u64,
) -> Result<FlashOutcome> {
    // Pre-flight: peek the UF2 header and refuse to write if the family
    // ID does not match the BOOTSEL board. The bootloader silently
    // discards wrong-family blocks and remounts as BOOTSEL with no
    // feedback to the operator -- a copy that looks like it succeeded
    // but produces a device that never enumerates. Block here instead.
    match read_uf2_family_id(uf2).await {
        Ok(Some(family)) => {
            if board.accepts_family(family) {
                tracing::debug!(
                    "flash: family-id ok (0x{:08X} accepted by {})",
                    family,
                    board.label()
                );
            } else {
                tracing::error!(
                    "flash: UF2 family mismatch -- board={} (drive {}) but UF2 family=0x{:08X} \
                     ({}). Expected file for this board: {}",
                    board.label(),
                    mount.display(),
                    family,
                    family_name(family),
                    board.canonical_uf2(),
                );
                bail!(
                    "UF2 family mismatch: {} has family 0x{:08X} ({}), but the detected \
                     BOOTSEL drive is {}. The bootloader would silently reject this write \
                     and the Pico would never come out of BOOTSEL.\n\n  For {}, use {}.",
                    uf2.display(),
                    family,
                    family_name(family),
                    board.label(),
                    board.label(),
                    board.canonical_uf2(),
                );
            }
        }
        Ok(None) => {
            tracing::warn!(
                "flash: UF2 magic not recognized in {}; skipping family check",
                uf2.display(),
            );
        }
        Err(e) => {
            tracing::debug!("flash: family-id peek failed: {e}");
        }
    }

    // The bootloader reboots the Pico as soon as the file copy finishes,
    // so the OS may return EOF or a write error on the very last block.
    // Treat a partial-write error after the drive vanished as success.
    let dest_name = uf2
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "couchlink-pico.uf2".into());
    let dest = mount.join(&dest_name);

    let bytes = tokio::fs::read(uf2)
        .await
        .with_context(|| format!("reading {}", uf2.display()))?;
    let bytes_written = bytes.len();
    let rebooted_during_copy = match tokio::fs::write(&dest, bytes).await {
        Ok(()) => {
            tracing::info!(
                "flash: copied {} bytes to {}",
                bytes_written,
                dest.display()
            );
            false
        }
        Err(e) => {
            if !mount.exists() {
                tracing::info!(
                    "flash: drive vanished after copy -- treating as success, Pico is rebooting"
                );
                true
            } else {
                return Err(e)
                    .with_context(|| format!("writing {} (Pico still present)", dest.display()));
            }
        }
    };

    let mut cfg = crate::config::load().unwrap_or_default();
    cfg.last_uf2 = Some(uf2.to_path_buf());
    let _ = crate::config::save(&cfg);

    Ok(FlashOutcome {
        board,
        mount,
        uf2_path: uf2.to_path_buf(),
        bytes_written,
        wait_seconds,
        rebooted_during_copy,
    })
}

pub async fn run(uf2: Option<PathBuf>, all: bool, from_usb: bool) -> Result<()> {
    let expected = if from_usb {
        reboot_setup_picos_to_bootsel(all)
            .await
            .context("asking setup-mode Pico(s) to enter BOOTSEL")?
    } else {
        print_manual_bootsel_hint();
        1
    };

    if all {
        flash_all_visible_bootsel(uf2.as_deref(), expected).await?;
    } else {
        flash_one_visible_bootsel(uf2.as_deref()).await?;
    }
    Ok(())
}

fn print_manual_bootsel_hint() {
    println!("Looking for a Pico in BOOTSEL mode (RPI-RP2 or RP2350 drive, 60 s timeout)...");
    println!("If your Pico is not in BOOTSEL yet: hold the BOOTSEL button, plug the Pico");
    println!("into this PC with a micro-USB data cable, then release BOOTSEL as soon as");
    println!("the RPI-RP2 or RP2350 drive appears in File Explorer.");
}

async fn flash_one_visible_bootsel(uf2: Option<&Path>) -> Result<FlashOutcome> {
    let start = Instant::now();
    let (mount, board) = wait_for_bootsel_mount(Duration::from_secs(60)).await?;
    let wait_seconds = start.elapsed().as_secs();
    println!("Detected {} at {}", board.label(), mount.display());
    let uf2_path = resolve_uf2_path(uf2, board).context("resolving which UF2 to flash")?;
    println!("Using firmware: {}", uf2_path.display());
    println!("Copying {} -> {} ...", uf2_path.display(), mount.display());
    let outcome = flash_uf2_to_mount(&uf2_path, mount, board, wait_seconds).await?;
    print_outcome(&outcome);
    Ok(outcome)
}

async fn flash_all_visible_bootsel(
    uf2: Option<&Path>,
    expected_min: usize,
) -> Result<Vec<FlashOutcome>> {
    let start = Instant::now();
    let mounts = wait_for_bootsel_mounts(
        Duration::from_secs(60),
        expected_min,
        Duration::from_millis(1200),
    )
    .await?;
    let wait_seconds = start.elapsed().as_secs();
    println!("Detected {} BOOTSEL drive(s).", mounts.len());

    let mut outcomes = Vec::with_capacity(mounts.len());
    for (mount, board) in mounts {
        println!("Detected {} at {}", board.label(), mount.display());
        let uf2_path = resolve_uf2_path(uf2, board).context("resolving which UF2 to flash")?;
        println!("Using firmware: {}", uf2_path.display());
        println!("Copying {} -> {} ...", uf2_path.display(), mount.display());
        let outcome = flash_uf2_to_mount(&uf2_path, mount, board, wait_seconds).await?;
        print_outcome(&outcome);
        outcomes.push(outcome);
    }

    Ok(outcomes)
}

async fn reboot_setup_picos_to_bootsel(all: bool) -> Result<usize> {
    let mut ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        bail!(
            "no setup-mode Pico found (looking for VID 0x{:04X} PID 0x{:04X})",
            cdc::SETUP_VID,
            cdc::SETUP_PID,
        );
    }
    if !all && ports.len() > 1 {
        tracing::warn!(
            "flash: multiple setup-mode Pico ports present {:?} -- picking the first one. \
             Pass --all to reboot and flash all of them.",
            ports
        );
        ports.truncate(1);
    }

    println!(
        "Asking {} setup-mode Pico(s) to enter BOOTSEL...",
        ports.len()
    );
    for port in &ports {
        let port_for_task = port.clone();
        let hello = tokio::task::spawn_blocking(move || -> Result<cdc::HelloAck> {
            let mut pico = cdc::PicoSetup::open_named(&port_for_task)?;
            let hello = pico.hello()?;
            pico.reboot_to_bootsel()?;
            Ok(hello)
        })
        .await
        .context("CDC reboot task failed")??;
        println!(
            "  {} fw v{} board=0x{:02X} -> BOOTSEL",
            port,
            hello.firmware_version(),
            hello.board_type
        );
    }

    Ok(ports.len())
}

fn print_outcome(outcome: &FlashOutcome) {
    if outcome.rebooted_during_copy {
        println!(
            "Pico rebooted mid-write (this is normal). Approximately {} bytes transferred before reboot.",
            outcome.bytes_written,
        );
    } else {
        println!(
            "Wrote {} bytes. The Pico should now reboot into the new firmware.",
            outcome.bytes_written,
        );
    }
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
    let mut mounts = wait_for_bootsel_mounts(timeout, 1, Duration::from_millis(0)).await?;
    Ok(mounts.remove(0))
}

pub async fn wait_for_bootsel_mounts(
    timeout: Duration,
    expected_min: usize,
    settle: Duration,
) -> Result<Vec<(PathBuf, BootselBoard)>> {
    let deadline = Instant::now() + timeout;
    let min_count = expected_min.max(1);
    let mut stable_since: Option<Instant> = None;
    let mut stable_count = 0usize;
    loop {
        let mounts = find_bootsel_mounts();
        if mounts.len() >= min_count {
            if settle.is_zero() {
                return Ok(mounts);
            }
            let now = Instant::now();
            if stable_since.is_none() || stable_count != mounts.len() {
                stable_since = Some(now);
                stable_count = mounts.len();
            }
            if stable_since
                .map(|seen| now.duration_since(seen) >= settle)
                .unwrap_or(false)
            {
                return Ok(mounts);
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "timeout: saw {} BOOTSEL drive(s), expected at least {} within {} s.",
                mounts.len(),
                min_count,
                timeout.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(windows)]
fn find_bootsel_mounts() -> Vec<(PathBuf, BootselBoard)> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_ACCESS_DENIED, ERROR_NOT_READY};
    use windows::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };
    use windows::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};
    // GetDriveTypeW returns one of these as a u32. windows-rs 0.58 doesn't
    // re-export the named constants under FileSystem, so we use the
    // literal from the Win32 docs.
    const DRIVE_REMOVABLE: u32 = 2;

    // Suppress "There is no disk in the drive" pop-ups when a legacy
    // USB card reader is connected without media -- GetVolumeInformationW
    // can otherwise raise a hardware-error dialog. Saved + restored so
    // the rest of the process sees the original mode.
    let prev_mode = unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };

    let mut buf = vec![0u16; 4096];
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    if n == 0 {
        let err = unsafe { GetLastError() };
        tracing::warn!(
            "flash: GetLogicalDriveStringsW returned 0 (GetLastError={:?})",
            err
        );
        unsafe { SetErrorMode(prev_mode) };
        return Vec::new();
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

    let mut hits: Vec<(String, BootselBoard, &'static str)> = Vec::new();

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
        if let Err(e) = r {
            // ERROR_NOT_READY (21) is the "card reader with no media"
            // case -- expected and very noisy if logged at warn. Anything
            // else on a removable drive is unusual enough to debug-log.
            let code = (e.code().0 & 0xFFFF) as u32;
            if code == ERROR_NOT_READY.0 {
                tracing::trace!("flash: GetVolumeInformationW({root}) = NOT_READY (no media)");
            } else if code == ERROR_ACCESS_DENIED.0 {
                tracing::warn!(
                    "flash: GetVolumeInformationW({root}) = ACCESS_DENIED (possibly BitLocker / AppLocker)"
                );
            } else {
                tracing::debug!("flash: GetVolumeInformationW({root}) failed: {e:?}");
            }
            continue;
        }
        let label_end = volume_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(volume_name.len());
        let label = String::from_utf16_lossy(&volume_name[..label_end]);

        // Defense-in-depth: read INFO_UF2.TXT and use its Board-ID as
        // the primary signal. OTP-whitelabelled RP2350 devices can
        // change the volume label, the SCSI vendor/product, and the
        // INFO_UF2 "Model:" / "Board-ID:" fields -- but the "UF2
        // Bootloader " prefix on line 1 is bootrom-baked and the most
        // reliable invariant. If INFO_UF2.TXT is unreadable (mount
        // race -- Windows announces the drive before the FAT mount
        // settles), fall back to the volume label.
        let info_path = format!("{root}INFO_UF2.TXT");
        let (board, detect) = match parse_info_uf2(&info_path) {
            Some(b) => (b, "INFO_UF2.TXT"),
            None => {
                let b = match label.as_str() {
                    "RPI-RP2" => BootselBoard::Rp2040,
                    "RP2350" => BootselBoard::Rp2350,
                    _ => continue,
                };
                tracing::debug!(
                    "flash: INFO_UF2.TXT not readable on {root}, falling back to volume label '{label}'"
                );
                (b, "volume-label")
            }
        };
        tracing::info!(
            "flash: BOOTSEL candidate root={root} board={} detect={detect}",
            board.label()
        );
        hits.push((root, board, detect));
    }

    unsafe { SetErrorMode(prev_mode) };

    if hits.is_empty() {
        return Vec::new();
    }
    if hits.len() > 1 {
        hits.sort_by(|a, b| a.0.cmp(&b.0));
    }
    hits.into_iter()
        .map(|(root, board, _)| (PathBuf::from(root), board))
        .collect()
}

#[cfg(windows)]
fn parse_info_uf2(path: &str) -> Option<BootselBoard> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut s = String::with_capacity(256);
    // The file is 3 short lines; cap reads conservatively.
    let mut buf = [0u8; 256];
    let n = f.read(&mut buf).ok()?;
    s.push_str(&String::from_utf8_lossy(&buf[..n]));
    // First line must start with "UF2 Bootloader " (bootrom invariant
    // across both RP2040 and RP2350; immune to OTP whitelabel which
    // only overrides Board-ID and Model).
    let first_line = s.lines().next()?;
    if !first_line.starts_with("UF2 Bootloader ") {
        return None;
    }
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("Board-ID:") {
            if let Some(b) = BootselBoard::from_info_uf2_board_id(rest) {
                return Some(b);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn find_bootsel_mounts() -> Vec<(PathBuf, BootselBoard)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rp2040_accepts_rp2040_and_absolute_only() {
        let b = BootselBoard::Rp2040;
        assert!(b.accepts_family(FAMILY_RP2040));
        assert!(b.accepts_family(FAMILY_ABSOLUTE));
        assert!(!b.accepts_family(FAMILY_DATA));
        assert!(!b.accepts_family(FAMILY_RP2350_ARM_S));
        assert!(!b.accepts_family(FAMILY_RP2350_ARM_NS));
        assert!(!b.accepts_family(FAMILY_RP2350_RISCV));
        assert!(!b.accepts_family(0xDEADBEEF));
    }

    #[test]
    fn rp2350_accepts_rp2350_variants_and_absolute_only() {
        let b = BootselBoard::Rp2350;
        assert!(b.accepts_family(FAMILY_RP2350_ARM_S));
        assert!(b.accepts_family(FAMILY_RP2350_ARM_NS));
        assert!(b.accepts_family(FAMILY_RP2350_RISCV));
        assert!(b.accepts_family(FAMILY_ABSOLUTE));
        assert!(!b.accepts_family(FAMILY_RP2040));
        assert!(!b.accepts_family(FAMILY_DATA));
        assert!(!b.accepts_family(0xDEADBEEF));
    }

    #[test]
    fn family_name_covers_known_ids() {
        assert_eq!(family_name(FAMILY_RP2040), "rp2040");
        assert_eq!(family_name(FAMILY_ABSOLUTE), "absolute");
        assert_eq!(family_name(FAMILY_DATA), "data");
        assert_eq!(family_name(FAMILY_RP2350_ARM_S), "rp2350-arm-s");
        assert_eq!(family_name(FAMILY_RP2350_ARM_NS), "rp2350-arm-ns");
        assert_eq!(family_name(FAMILY_RP2350_RISCV), "rp2350-riscv");
        assert_eq!(family_name(0xDEADBEEF), "unknown");
    }

    #[test]
    fn info_uf2_board_id_matches_known_boards() {
        assert_eq!(
            BootselBoard::from_info_uf2_board_id("RPI-RP2"),
            Some(BootselBoard::Rp2040)
        );
        assert_eq!(
            BootselBoard::from_info_uf2_board_id("RP2350"),
            Some(BootselBoard::Rp2350)
        );
        // Trailing whitespace from "Board-ID: <id>\r\n" is normalized.
        assert_eq!(
            BootselBoard::from_info_uf2_board_id(" RPI-RP2 \r\n"),
            Some(BootselBoard::Rp2040)
        );
        // OTP whitelabel string is rejected -- forces a label fallback.
        assert_eq!(
            BootselBoard::from_info_uf2_board_id("CUSTOMVENDOR-WIDGET"),
            None
        );
        assert_eq!(BootselBoard::from_info_uf2_board_id(""), None);
    }
}
