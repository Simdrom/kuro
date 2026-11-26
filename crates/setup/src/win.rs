//! The few Windows calls that need COM or raw handles.
#![allow(unsafe_code)]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    IPersistFile,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{GUID, HSTRING, Interface, PWSTR};

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

/// Where the running programs called `name` were started from. Those of other users cannot be
/// opened, so they are left out.
pub fn running(name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    // SAFETY: the snapshot and process handles are closed here, and the buffers outlive the
    // calls that fill them.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return found };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(name)
                && let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, e.th32ProcessID)
            {
                let mut buf = [0u16; 1024];
                let mut n = buf.len() as u32;
                if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n).is_ok()
                {
                    found.push(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])));
                }
                let _ = CloseHandle(h);
            }
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    found
}

/// Starts a program without a console window and does not wait for it.
pub fn run_hidden(program: &str, args: &str) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new(program).raw_arg(args).creation_flags(CREATE_NO_WINDOW).spawn();
}
