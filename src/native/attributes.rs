//! Change attributes on the reparse point itself, never on its target.
use anyhow::{Context, Result};
use std::{
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};
pub fn writable_link(path: &Path) -> Result<()> {
    unsafe {
        let h = CreateFileW(
            super::windows::wide(path).as_ptr(),
            FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info: FILE_BASIC_INFO = std::mem::zeroed();
        let result = (|| -> Result<()> {
            if GetFileInformationByHandleEx(
                h,
                FileBasicInfo,
                (&mut info as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            info.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
            if SetFileInformationByHandle(
                h,
                FileBasicInfo,
                (&info as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(())
        })();
        CloseHandle(h);
        result.with_context(|| format!("Cannot clear link attributes: {}", path.display()))
    }
}
