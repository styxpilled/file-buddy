//! Deleting files through the Windows shell, as Explorer does: to the recycle
//! bin, or for good after the shell's own confirmation dialog.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_WANTNUKEWARNING,
    FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
};
use windows::core::{HRESULT, PCWSTR};

/// A decoder closes its file on its own thread, a moment after the clip is
/// dropped, and the shell cannot delete the file until it has.
const RELEASE_WAIT: Duration = Duration::from_millis(50);
const RELEASE_TRIES: usize = 20;
/// `COPYENGINE_E_USER_CANCELLED`, which the shell returns for a cancelled dialog.
const USER_CANCELLED: HRESULT = HRESULT(0x8027_0000_u32 as i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `delete`. No dialog, as in Explorer. The shell still asks before a file
    /// that cannot go to the recycle bin is deleted for good.
    Trash,
    /// `shift-delete`. The shell asks first.
    Delete,
}

/// Delete `paths` and wait for the shell to finish, dialogs included. `owner` is
/// the window the dialogs belong to. The caller checks which files are gone: the
/// user can cancel, and a file can fail while the others go.
pub fn delete(paths: Vec<PathBuf>, mode: Mode, owner: isize) -> Result<()> {
    // The shell wants a single-threaded apartment, and its dialogs run a message
    // loop of their own, which must not run inside the window's.
    std::thread::spawn(move || {
        for _ in 0..RELEASE_TRIES {
            if paths.iter().all(|path| released(path)) {
                break;
            }
            std::thread::sleep(RELEASE_WAIT);
        }
        // SAFETY: this thread starts no other apartment, and every call below
        // runs on it, before `CoUninitialize`.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let result = perform(&paths, mode, HWND(owner as _));
            CoUninitialize();
            result
        }
    })
    .join()
    .map_err(|_| anyhow!("the delete thread panicked"))?
}

unsafe fn perform(paths: &[PathBuf], mode: Mode, owner: HWND) -> Result<()> {
    unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        let flags = match mode {
            Mode::Trash => {
                FOF_ALLOWUNDO.0
                    | FOF_NOCONFIRMATION.0
                    | FOF_WANTNUKEWARNING.0
                    | FOFX_RECYCLEONDELETE.0
            }
            Mode::Delete => 0,
        };
        operation.SetOperationFlags(FILEOPERATION_FLAGS(flags))?;
        operation.SetOwnerWindow(owner)?;
        for path in paths {
            // A root from the config can mix the two separators, and the shell
            // takes only backslashes.
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .map(|unit| {
                    if unit == u16::from(b'/') {
                        u16::from(b'\\')
                    } else {
                        unit
                    }
                })
                .chain([0])
                .collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
                .with_context(|| format!("find {}", path.display()))?;
            operation.DeleteItem(&item, None)?;
        }
        match operation.PerformOperations() {
            Err(e) if e.code() == USER_CANCELLED || e.code() == ERROR_CANCELLED.to_hresult() => {
                Ok(())
            }
            result => result.context("delete failed"),
        }
    }
}

/// Whether nothing holds the file open without delete sharing.
fn released(path: &Path) -> bool {
    std::fs::OpenOptions::new()
        .access_mode(DELETE.0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .open(path)
        .is_ok()
}
