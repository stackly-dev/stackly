//! The runtime download cache of `cargo xtask` (spec 0003, *Download dan
//! cache*), read by native tests (spec 0018 AC-20): the artifact files the
//! spike downloaded once, found by digest. Nothing here downloads anything.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The cache folder of `cargo xtask` on this Mac, relative to `$HOME`.
const MACOS_CACHE: &str = "Library/Caches/dev.stackly.xtask/runtime";

/// The runtime cache that `cargo xtask runtime spike` fills: one file per
/// artifact, named by the SHA-256 of its bytes.
///
/// ```
/// use stackly_testkit::RuntimeCache;
///
/// let dir = tempfile::tempdir()?;
/// let cache = RuntimeCache::at(dir.path());
/// assert_eq!(cache.entry(&"0".repeat(64), 3), None);
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct RuntimeCache {
    dir: PathBuf,
}

impl RuntimeCache {
    /// The cache of this user: `~/Library/Caches/dev.stackly.xtask/runtime`.
    /// `None` when `HOME` is not set.
    #[must_use]
    pub fn open() -> Option<Self> {
        let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
        Some(Self::at(&Path::new(&home).join(MACOS_CACHE)))
    }

    /// The cache in `dir`, which does not have to exist.
    #[must_use]
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// The cache folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The cached file of the artifact with this digest and size, or `None`
    /// when there is no regular file of that name, its size differs, or its
    /// bytes do not hash to the digest. A file that fails the check is never
    /// returned, so a test never installs from a damaged entry.
    #[must_use]
    pub fn entry(&self, sha256: &str, size: u64) -> Option<PathBuf> {
        let path = self.dir.join(sha256);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() != size {
            return None;
        }
        (digest_of(&path).ok()?.as_str() == sha256).then_some(path)
    }

    /// Copies the entry into `downloads` under its digest, the way the
    /// daemon keeps a finished download, and returns the new path. `None`
    /// when [`entry`](Self::entry) finds nothing.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the folder or the copy cannot be made.
    pub fn copy_to(
        &self,
        sha256: &str,
        size: u64,
        downloads: &Path,
    ) -> io::Result<Option<PathBuf>> {
        let Some(source) = self.entry(sha256, size) else {
            return Ok(None);
        };
        fs::create_dir_all(downloads)?;
        let target = downloads.join(sha256);
        fs::copy(&source, &target)?;
        Ok(Some(target))
    }
}

/// The SHA-256 of the file at `path`, 64 lowercase hex characters.
fn digest_of(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0_u8; 1 << 16];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn an_entry_is_found_only_when_its_size_and_digest_both_match() {
        let dir = tempfile::tempdir().unwrap();
        let cache = RuntimeCache::at(dir.path());
        fs::write(dir.path().join(ABC_SHA256), "abc").unwrap();
        assert_eq!(
            cache.entry(ABC_SHA256, 3),
            Some(dir.path().join(ABC_SHA256))
        );
        assert_eq!(cache.entry(ABC_SHA256, 4), None, "wrong size");
        let other = "1".repeat(64);
        fs::write(dir.path().join(&other), "abc").unwrap();
        assert_eq!(cache.entry(&other, 3), None, "wrong digest");
        assert_eq!(cache.entry(&"2".repeat(64), 3), None, "missing");
    }

    #[test]
    fn a_link_in_the_cache_is_not_an_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = RuntimeCache::at(dir.path());
        let real = dir.path().join("real");
        fs::write(&real, "abc").unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join(ABC_SHA256)).unwrap();
        assert_eq!(cache.entry(ABC_SHA256, 3), None);
    }

    #[test]
    fn copy_to_places_the_bytes_under_their_digest() {
        let dir = tempfile::tempdir().unwrap();
        let cache = RuntimeCache::at(dir.path());
        fs::write(dir.path().join(ABC_SHA256), "abc").unwrap();
        let downloads = dir.path().join("downloads");
        let copied = cache.copy_to(ABC_SHA256, 3, &downloads).unwrap().unwrap();
        assert_eq!(copied, downloads.join(ABC_SHA256));
        assert_eq!(fs::read(copied).unwrap(), b"abc");
        assert_eq!(cache.copy_to(&"3".repeat(64), 3, &downloads).unwrap(), None);
    }
}
