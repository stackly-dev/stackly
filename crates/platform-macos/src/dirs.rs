//! The two OS facts the macOS layout starts from: the user's home directory
//! from the user database, and the user's private temporary directory.

use std::ffi::{CStr, OsStr};
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;

use stackly_platform::PlatformError;

/// Data root, relative to the home directory.
pub(crate) const DATA_DIR: &str = "Library/Application Support/Stackly";
/// Logs root, relative to the home directory.
pub(crate) const LOGS_DIR: &str = "Library/Logs/Stackly";
/// Cache root, relative to the home directory.
pub(crate) const CACHE_DIR: &str = "Library/Caches/Stackly";
/// Runtime root, relative to the user's temporary directory.
pub(crate) const RUNTIME_DIR: &str = "stackly";

/// First buffer size for `getpwuid_r`; doubled on `ERANGE`.
const PASSWD_BUF_START: usize = 1024;
/// Largest buffer `getpwuid_r` gets before giving up.
const PASSWD_BUF_MAX: usize = 1024 * 1024;

/// Returns the home directory of the user this process runs as, from the
/// user database (`getpwuid_r(getuid())`), never from `HOME`.
pub(crate) fn home_dir() -> Result<PathBuf, PlatformError> {
    let uid = crate::identity::current_uid();
    let mut buf_len = PASSWD_BUF_START;
    loop {
        let mut buf = vec![0 as libc::c_char; buf_len];
        let mut entry = MaybeUninit::<libc::passwd>::uninit();
        let mut found: *mut libc::passwd = ptr::null_mut();
        // SAFETY: `entry` is writable storage for one `passwd`, `buf` holds
        // `buf.len()` writable bytes, and `found` is a writable pointer.
        // getpwuid_r writes only inside these three.
        let rc = unsafe {
            libc::getpwuid_r(
                uid,
                entry.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &raw mut found,
            )
        };
        if rc == libc::ERANGE && buf_len < PASSWD_BUF_MAX {
            buf_len *= 2;
            continue;
        }
        if rc != 0 || found.is_null() {
            return Err(PlatformError::HomeDirUnavailable);
        }
        // SAFETY: getpwuid_r returned 0 with a non null result, so it filled
        // `entry` (which `found` points to).
        let dir_ptr = unsafe { (*found).pw_dir };
        if dir_ptr.is_null() {
            return Err(PlatformError::HomeDirUnavailable);
        }
        // SAFETY: `pw_dir` is non null and points to a NUL terminated string
        // that getpwuid_r stored in `buf`, which is alive until this scope
        // ends; the bytes are copied out before then.
        let dir = unsafe { CStr::from_ptr(dir_ptr) };
        let home = PathBuf::from(OsStr::from_bytes(dir.to_bytes()));
        return if home.is_absolute() {
            Ok(home)
        } else {
            Err(PlatformError::HomeDirUnavailable)
        };
    }
}

/// Returns the user's private temporary directory, the 0700 folder macOS
/// creates per user under `/var/folders`, from
/// `confstr(_CS_DARWIN_USER_TEMP_DIR)`.
pub(crate) fn darwin_user_temp_dir() -> Result<PathBuf, PlatformError> {
    // SAFETY: a null buffer with length 0 asks confstr only for the size it
    // needs, including the trailing NUL; nothing is written.
    let needed = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, ptr::null_mut(), 0) };
    if needed == 0 {
        return Err(PlatformError::TempDirUnavailable);
    }
    let mut buf = vec![0_u8; needed];
    // SAFETY: `buf` holds `buf.len()` writable bytes, and confstr writes at
    // most that many, the trailing NUL included.
    let written = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if written == 0 || written > buf.len() {
        return Err(PlatformError::TempDirUnavailable);
    }
    let value = CStr::from_bytes_until_nul(&buf).map_err(|_| PlatformError::TempDirUnavailable)?;
    let temp = PathBuf::from(OsStr::from_bytes(value.to_bytes()));
    if temp.is_absolute() {
        Ok(temp)
    } else {
        Err(PlatformError::TempDirUnavailable)
    }
}
