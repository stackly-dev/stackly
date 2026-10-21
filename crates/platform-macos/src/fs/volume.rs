use std::io;
use std::mem::MaybeUninit;
use std::path::Path;

use stackly_platform::{PlatformError, VolumeFacts, VolumeSpace};

use super::fd::c_path;

/// [`Filesystem::volume_space`](stackly_platform::Filesystem::volume_space):
/// `f_blocks × f_bsize` and `f_bavail × f_bsize` from one `statfs`.
pub(super) fn volume_space(path: &Path) -> Result<VolumeSpace, PlatformError> {
    let stat = read_statfs("volume_space", path)?;
    let block = u64::from(stat.f_bsize);
    Ok(VolumeSpace::new(
        stat.f_blocks.saturating_mul(block),
        stat.f_bavail.saturating_mul(block),
    ))
}

/// [`Filesystem::volume_facts`](stackly_platform::Filesystem::volume_facts):
/// `f_fstypename` and the `MNT_LOCAL` flag from one `statfs`.
pub(super) fn volume_facts(path: &Path) -> Result<VolumeFacts, PlatformError> {
    let stat = read_statfs("volume_facts", path)?;
    let name: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|&&byte| byte != 0)
        // `c_char` is `i8`; the cast reinterprets each byte unchanged.
        .map(|&byte| byte as u8)
        .collect();
    let local = stat.f_flags & (libc::MNT_LOCAL as u32) != 0;
    Ok(VolumeFacts::new(
        String::from_utf8_lossy(&name).into_owned(),
        local,
    ))
}

/// Reads the volume that holds `path` with `statfs`, the one helper every
/// volume query of this adapter shares (spec 0004).
pub(super) fn read_statfs(op: &'static str, path: &Path) -> Result<libc::statfs, PlatformError> {
    let c_path = c_path(op, path)?;
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c_path` is NUL terminated and `stat` points to writable
    // memory the size of `libc::statfs`; `statfs` keeps neither pointer.
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return Err(PlatformError::from_io(
            op,
            path,
            &io::Error::last_os_error(),
        ));
    }
    // SAFETY: `statfs` returned 0, so it filled `stat`.
    Ok(unsafe { stat.assume_init() })
}
