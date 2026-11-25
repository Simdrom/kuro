//! The few Windows calls that need COM or raw handles.
#![allow(unsafe_code)]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    IPersistFile,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{GUID, HSTRING, Interface};

fn known(id: &GUID) -> Option<PathBuf> {
    // SAFETY: the returned buffer is ours to free, and is read before that.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        path.map(PathBuf::from)
    }
}

/// The user's Start menu programs folder.
pub fn programs() -> Option<PathBuf> {
    known(&FOLDERID_Programs)
}

/// The user's desktop, wherever it was moved to.
pub fn desktop() -> Option<PathBuf> {
    known(&FOLDERID_Desktop)
}

/// Writes a shortcut to `target`.
pub fn shortcut(lnk: &Path, target: &Path) -> windows::core::Result<()> {
    // SAFETY: plain COM calls on interfaces created here; already initialised is fine.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.SetDescription(&HSTRING::from("Kuro"))?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(lnk.as_os_str()), true)
    }
}

/// Starts a program without a console window and does not wait for it.
pub fn run_hidden(program: &str, args: &str) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new(program).raw_arg(args).creation_flags(CREATE_NO_WINDOW).spawn();
}
