use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

use crate::cdc;
use crate::modes::{BootselEntry, PicoScope};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootselBoard {
    Rp2040,
    Rp2350,
}

impl BootselBoard {
    pub(crate) fn canonical_uf2(self) -> &'static str {
        match self {
            BootselBoard::Rp2040 => "couchlink-picow.uf2",
            BootselBoard::Rp2350 => "couchlink-pico2w.uf2",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            BootselBoard::Rp2040 => "Pico W / WH (RP2040)",
            BootselBoard::Rp2350 => "Pico 2 W (RP2350)",
        }
    }

    pub(crate) fn from_info_uf2_board_id(s: &str) -> Option<Self> {
        match s.trim() {
            "RPI-RP2" => Some(BootselBoard::Rp2040),
            "RP2350" => Some(BootselBoard::Rp2350),
            _ => None,
        }
    }

    pub(crate) fn accepts_family(self, family_id: u32) -> bool {
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

const FAMILY_RP2040: u32 = 0xE48B_FF56;
const FAMILY_ABSOLUTE: u32 = 0xE48B_FF57;
const FAMILY_DATA: u32 = 0xE48B_FF58;
const FAMILY_RP2350_ARM_S: u32 = 0xE48B_FF59;
const FAMILY_RP2350_ARM_NS: u32 = 0xE48B_FF5A;
const FAMILY_RP2350_RISCV: u32 = 0xE48B_FF5B;

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

#[derive(Clone, Debug)]
pub(crate) struct FlashOutcome {
    pub board: BootselBoard,
    pub uf2_path: PathBuf,
    pub bytes_written: usize,
    pub rebooted_during_copy: bool,
}

pub(crate) async fn flash_uf2_to_mount(
    uf2: &Path,
    mount: PathBuf,
    board: BootselBoard,
) -> Result<FlashOutcome> {
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

    let dest_name = uf2.file_name().map_or_else(
        || "couchlink-pico.uf2".into(),
        std::borrow::ToOwned::to_owned,
    );
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
            if mount.exists() {
                return Err(e)
                    .with_context(|| format!("writing {} (Pico still present)", dest.display()));
            }
            tracing::info!(
                "flash: drive vanished after copy -- treating as success, Pico is rebooting"
            );
            true
        }
    };

    let mut cfg = crate::config::load().unwrap_or_default();
    cfg.last_uf2 = Some(uf2.to_path_buf());
    let _ = crate::config::save(&cfg);

    Ok(FlashOutcome {
        board,
        uf2_path: uf2.to_path_buf(),
        bytes_written,
        rebooted_during_copy,
    })
}

pub(crate) async fn run(uf2: Option<PathBuf>, scope: PicoScope, entry: BootselEntry) -> Result<()> {
    let expected = if entry == BootselEntry::OverUsb {
        reboot_setup_picos_to_bootsel(scope.is_all())
            .await
            .context("asking setup-mode Pico(s) to enter BOOTSEL")?
    } else {
        print_manual_bootsel_hint();
        1
    };

    if scope.is_all() {
        flash_all_visible_bootsel(uf2.as_deref(), expected).await?;
    } else {
        flash_one_visible_bootsel(uf2.as_deref()).await?;
    }
    Ok(())
}

fn print_manual_bootsel_hint() {
    outln!("Looking for a Pico in BOOTSEL mode (RPI-RP2 or RP2350 drive, 60 s timeout)...");
    outln!("If your Pico is not in BOOTSEL yet: hold the BOOTSEL button, plug the Pico");
    outln!("into this PC with a micro-USB data cable, then release BOOTSEL as soon as");
    outln!("the RPI-RP2 or RP2350 drive appears in File Explorer.");
}

async fn flash_one_visible_bootsel(uf2: Option<&Path>) -> Result<FlashOutcome> {
    let (mount, board) = wait_for_bootsel_mount(Duration::from_secs(60)).await?;
    outln!("Detected {} at {}", board.label(), mount.display());
    let uf2_path = resolve_uf2_path(uf2, board).context("resolving which UF2 to flash")?;
    outln!("Using firmware: {}", uf2_path.display());
    outln!("Copying {} -> {} ...", uf2_path.display(), mount.display());
    let outcome = flash_uf2_to_mount(&uf2_path, mount, board).await?;
    print_outcome(&outcome);
    Ok(outcome)
}

async fn flash_all_visible_bootsel(
    uf2: Option<&Path>,
    expected_min: usize,
) -> Result<Vec<FlashOutcome>> {
    let mounts = wait_for_bootsel_mounts(
        Duration::from_secs(60),
        expected_min,
        Duration::from_millis(1200),
    )
    .await?;
    outln!("Detected {} BOOTSEL drive(s).", mounts.len());

    let mut outcomes = Vec::with_capacity(mounts.len());
    for (mount, board) in mounts {
        outln!("Detected {} at {}", board.label(), mount.display());
        let uf2_path = resolve_uf2_path(uf2, board).context("resolving which UF2 to flash")?;
        outln!("Using firmware: {}", uf2_path.display());
        outln!("Copying {} -> {} ...", uf2_path.display(), mount.display());
        let outcome = flash_uf2_to_mount(&uf2_path, mount, board).await?;
        print_outcome(&outcome);
        outcomes.push(outcome);
    }

    Ok(outcomes)
}

async fn reboot_setup_picos_to_bootsel(all: bool) -> Result<usize> {
    let mut ports = cdc::find_setup_ports()?;
    if ports.is_empty() {
        bail!(
            "no setup-mode Pico found (looking for VID 0x{:04X} PID 0x{:04X}). \
             No-button flashing only works when CouchLink setup-mode firmware is visible over USB. \
             If the Pico is already in run mode, use `couchlink flash --all` and put it in BOOTSEL, \
             or run `couchlink` and choose Update Pico firmware for the guided path.",
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

    outln!(
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
        outln!(
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
        outln!(
            "Pico rebooted mid-write (this is normal). Approximately {} bytes transferred before reboot.",
            outcome.bytes_written,
        );
    } else {
        outln!(
            "Wrote {} bytes. The Pico should now reboot into the new firmware.",
            outcome.bytes_written,
        );
    }
}

pub(crate) fn resolve_uf2_path(uf2: Option<&Path>, board: BootselBoard) -> Result<PathBuf> {
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

    auto_pick_uf2(board)
}

fn auto_pick_uf2(board: BootselBoard) -> Result<PathBuf> {
    let canonical = board.canonical_uf2();
    let legacy = "couchlink-pico.uf2";

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf));
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

async fn read_uf2_family_id(path: &Path) -> Result<Option<u32>> {
    const UF2_MAGIC_START: u32 = 0x0A32_4655;
    use tokio::io::AsyncReadExt;

    const UF2_MAGIC_START2: u32 = 0x9E5D_5157;
    let mut buf = [0u8; 32];
    let mut f = tokio::fs::File::open(path).await?;
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

pub(crate) async fn wait_for_bootsel_mount(timeout: Duration) -> Result<(PathBuf, BootselBoard)> {
    let mut mounts = wait_for_bootsel_mounts(timeout, 1, Duration::from_millis(0)).await?;
    Ok(mounts.remove(0))
}

pub(crate) async fn wait_for_bootsel_mounts(
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
            if stable_since.is_some_and(|seen| now.duration_since(seen) >= settle) {
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

pub(crate) fn visible_bootsel_mounts() -> Vec<(PathBuf, BootselBoard)> {
    find_bootsel_mounts()
}

#[cfg(windows)]
fn bootsel_candidate(root: &str) -> Option<(BootselBoard, &'static str)> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_NOT_READY};
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW};
    use windows::core::PCWSTR;
    const DRIVE_REMOVABLE: u32 = 2;

    let wide: Vec<u16> = OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
    if drive_type != DRIVE_REMOVABLE {
        return None;
    }
    let mut volume_name = vec![0u16; 256];
    let mut fs_name = vec![0u16; 256];
    // SAFETY: `wide` is NUL-terminated and both output slices are live for the call.
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
        let code = e.code().0.cast_unsigned() & 0xFFFF;
        if code == ERROR_NOT_READY.0 {
            tracing::trace!("flash: GetVolumeInformationW({root}) = NOT_READY (no media)");
        } else if code == ERROR_ACCESS_DENIED.0 {
            tracing::warn!(
                "flash: GetVolumeInformationW({root}) = ACCESS_DENIED (possibly BitLocker / AppLocker)"
            );
        } else {
            tracing::debug!("flash: GetVolumeInformationW({root}) failed: {e:?}");
        }
        return None;
    }
    let label_end = volume_name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(volume_name.len());
    let label = String::from_utf16_lossy(&volume_name[..label_end]);

    let info_path = format!("{root}INFO_UF2.TXT");
    let (board, detect) = if let Some(b) = parse_info_uf2(&info_path) {
        (b, "INFO_UF2.TXT")
    } else {
        let b = match label.as_str() {
            "RPI-RP2" => BootselBoard::Rp2040,
            "RP2350" => BootselBoard::Rp2350,
            _ => return None,
        };
        tracing::debug!(
            "flash: INFO_UF2.TXT not readable on {root}, falling back to volume label '{label}'"
        );
        (b, "volume-label")
    };
    Some((board, detect))
}

#[cfg(windows)]
fn find_bootsel_mounts() -> Vec<(PathBuf, BootselBoard)> {
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Storage::FileSystem::GetLogicalDriveStringsW;
    use windows::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};

    // SAFETY: SetErrorMode takes a flag value and touches no memory of ours.
    let prev_mode = unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };

    let mut buf = vec![0u16; 4096];
    // SAFETY: `buf` is a live slice and the call writes at most its length.
    let n = crate::convert::usize_from_u32(unsafe { GetLogicalDriveStringsW(Some(&mut buf)) });
    if n == 0 {
        // SAFETY: GetLastError only reads this thread's last error value.
        let err = unsafe { GetLastError() };
        tracing::warn!(
            "flash: GetLogicalDriveStringsW returned 0 (GetLastError={:?})",
            err
        );
        // SAFETY: this puts back the mode SetErrorMode returned above; no memory is involved.
        unsafe { SetErrorMode(prev_mode) };
        return Vec::new();
    }

    let mut roots = Vec::new();
    let mut start = 0usize;
    while start < n {
        let end = buf[start..n]
            .iter()
            .position(|&c| c == 0)
            .map_or(n, |i| start + i);
        if end <= start {
            break;
        }
        roots.push(String::from_utf16_lossy(&buf[start..end]));
        start = end + 1;
    }

    let mut hits: Vec<(String, BootselBoard, &'static str)> = Vec::new();

    for root in roots {
        let Some((board, detect)) = bootsel_candidate(&root) else {
            continue;
        };
        tracing::info!(
            "flash: BOOTSEL candidate root={root} board={} detect={detect}",
            board.label()
        );
        hits.push((root, board, detect));
    }

    // SAFETY: this puts back the mode SetErrorMode returned above; no memory is involved.
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
    let mut buf = [0u8; 256];
    let n = f.read(&mut buf).ok()?;
    s.push_str(&String::from_utf8_lossy(&buf[..n]));
    let first_line = s.lines().next()?;
    if !first_line.starts_with("UF2 Bootloader ") {
        return None;
    }
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("Board-ID:")
            && let Some(b) = BootselBoard::from_info_uf2_board_id(rest)
        {
            return Some(b);
        }
    }
    None
}

#[cfg(not(windows))]
fn find_bootsel_mounts() -> Vec<(PathBuf, BootselBoard)> {
    Vec::new()
}

#[cfg(test)]
mod tests;
