//! The boot ID (spec 0010, *Adapter macOS*, row Boot): the UUID the kernel
//! draws at every boot, `kern.bootsessionuuid`.

use std::ffi::CStr;
use std::io;
use std::ptr;

use libc::c_void;
use stackly_domain::BootId;
use stackly_platform::ProcessError;

const OP: &str = "sysctl kern.bootsessionuuid";

/// Reads the ID of the current boot.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when `sysctlbyname` fails or answers
/// something that is not a hyphenated UUID.
pub(super) fn boot_id() -> Result<BootId, ProcessError> {
    let mut buffer = [0_u8; 64];
    let mut len = buffer.len();
    // SAFETY: the name is a static C string, the buffer is writable for
    // `len` bytes, and no new value is set.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            buffer.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(ProcessError::io(OP, None, &io::Error::last_os_error()));
    }
    let text = CStr::from_bytes_until_nul(&buffer[..len.min(buffer.len())])
        .ok()
        .and_then(|text| text.to_str().ok())
        .ok_or_else(|| invalid("the boot session UUID is not UTF-8 text".to_owned()))?;
    text.parse().map_err(|error| {
        invalid(format!(
            "the boot session UUID {text:?} is invalid: {error}"
        ))
    })
}

fn invalid(message: String) -> ProcessError {
    ProcessError::Io {
        op: OP,
        path: None,
        kind: io::ErrorKind::InvalidData,
        message,
    }
}
