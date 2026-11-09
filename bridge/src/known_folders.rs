#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IPersistFile,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Startup, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{Interface, PCWSTR};

pub(crate) fn startup_folder() -> Result<PathBuf> {
    // SAFETY: `pwstr` is the string the shell allocated; it is read, then freed once.
    unsafe {
        let pwstr = SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DEFAULT, None)
            .map_err(|e| anyhow!("SHGetKnownFolderPath failed: {e}"))?;
        let s = pwstr
            .to_string()
            .context("startup path was not valid UTF-16")?;
        windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0.cast_const().cast()));
        Ok(PathBuf::from(s))
    }
}

pub(crate) fn shortcut_path_for(name: &str) -> Result<PathBuf> {
    Ok(startup_folder()?.join(format!("{name}.lnk")))
}

pub(crate) fn create_shortcut(
    link_path: &Path,
    target_exe: &Path,
    arguments: &str,
    working_dir: Option<&Path>,
    description: &str,
) -> Result<()> {
    // SAFETY: every pointer handed to COM below comes from a live NUL-terminated buffer.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let need_uninit = init.is_ok();

        let result: Result<()> = (|| {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("CoCreateInstance(ShellLink) failed: {e}"))?;

            let target_w = wide(target_exe.as_os_str());
            link.SetPath(PCWSTR(target_w.as_ptr()))
                .map_err(|e| anyhow!("IShellLink::SetPath failed: {e}"))?;

            if !arguments.is_empty() {
                let args_w = wide(OsStr::new(arguments));
                link.SetArguments(PCWSTR(args_w.as_ptr()))
                    .map_err(|e| anyhow!("IShellLink::SetArguments failed: {e}"))?;
            }

            if let Some(wd) = working_dir {
                let wd_w = wide(wd.as_os_str());
                link.SetWorkingDirectory(PCWSTR(wd_w.as_ptr()))
                    .map_err(|e| anyhow!("IShellLink::SetWorkingDirectory failed: {e}"))?;
            }

            if !description.is_empty() {
                let desc_w = wide(OsStr::new(description));
                link.SetDescription(PCWSTR(desc_w.as_ptr()))
                    .map_err(|e| anyhow!("IShellLink::SetDescription failed: {e}"))?;
            }

            let persist: IPersistFile = link
                .cast()
                .map_err(|e| anyhow!("QI(IPersistFile) failed: {e}"))?;
            let link_w = wide(link_path.as_os_str());
            persist
                .Save(PCWSTR(link_w.as_ptr()), true)
                .map_err(|e| anyhow!("IPersistFile::Save failed: {e}"))?;
            Ok(())
        })();

        if need_uninit {
            CoUninitialize();
        }
        result
    }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}
