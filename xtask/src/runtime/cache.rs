//! The download cache shared by `runtime pin`, `runtime verify`, and the
//! spike (spec 0003).
//!
//! Every file is stored under its SHA-256 and hashed again each time it is
//! used; an entry that no longer matches is deleted. Downloads go to a
//! temporary file in the cache folder first and are only moved into place
//! once their digest is known.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256, Sha512};
use tempfile::NamedTempFile;

use super::inventory::is_https;

/// The largest file `runtime pin` downloads: 4 GiB.
pub const PIN_MAX_SIZE: u64 = 4 * 1024 * 1024 * 1024;
/// The largest checksum or signature file read from upstream: 1 MiB.
pub const SIDE_FILE_MAX_SIZE: u64 = 1024 * 1024;
/// The curl binary; never one from PATH.
const CURL: &str = "/usr/bin/curl";
/// Seconds one download may take.
const MAX_TIME_SECONDS: &str = "1800";
/// curl's exit code when the announced size is over `--max-filesize`.
const CURL_TOO_LARGE: i32 = 63;
/// What curl prints when it stops a transfer that grew past the limit
/// without announcing its size first (exit code 56).
const CURL_TOO_LARGE_MESSAGE: &str = "Maximum file size exceeded";

/// A download that is larger than its limit. `runtime verify` reports it
/// as a size mismatch.
#[derive(Debug)]
pub struct TooLarge {
    /// The URL.
    pub url: String,
    /// The limit in bytes.
    pub max_size: u64,
}

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot download {}: it is larger than {} bytes",
            self.url, self.max_size
        )
    }
}

impl std::error::Error for TooLarge {}

/// The cache folder.
pub struct Cache {
    dir: PathBuf,
}

/// A finished download that is not in the cache yet. Dropping it deletes
/// the temporary file.
pub struct Download {
    pub(super) file: NamedTempFile,
    /// The URL it came from.
    pub url: String,
    /// The SHA-256 of the bytes, 64 lowercase hex characters.
    pub sha256: String,
    /// The number of bytes.
    pub size: u64,
}

impl Download {
    /// The temporary file holding the bytes.
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}

impl Cache {
    /// Opens the cache of this user, creating the folder when needed.
    ///
    /// macOS: `~/Library/Caches/dev.stackly.xtask/runtime/`. Elsewhere:
    /// `$XDG_CACHE_HOME` (when absolute) or `~/.cache`, then
    /// `stackly-xtask/runtime/`.
    pub fn open() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .context("HOME is not set, so the runtime cache has no home")?;
        let xdg = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
        Self::at(default_dir(
            cfg!(target_os = "macos"),
            &home,
            xdg.as_deref(),
        ))
    }

    /// Opens the cache at `dir`, creating the folder when needed.
    pub fn at(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create the runtime cache {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// The cache folder itself, which also holds the spike lock.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the file with this digest lives.
    pub fn path(&self, sha256: &str) -> PathBuf {
        self.dir.join(sha256)
    }

    /// Returns the cached file when it exists and still has this digest and
    /// size. An entry that no longer matches is deleted.
    pub fn lookup(&self, sha256: &str, size: u64) -> Result<Option<PathBuf>> {
        let path = self.path(sha256);
        match fs::symlink_metadata(&path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| format!("cannot read {}", path.display()));
            }
            Ok(meta) if meta.is_file() => {
                let (actual, actual_size) = sha256_file(&path)?;
                if actual == sha256 && actual_size == size {
                    return Ok(Some(path));
                }
            }
            Ok(_) => {}
        }
        eprintln!(
            "warning: runtime cache entry {sha256} no longer matches its digest; deleting it"
        );
        remove_entry(&path)?;
        Ok(None)
    }

    /// Downloads `url` to a temporary file in the cache folder, refusing
    /// more than `max_size` bytes, and hashes it. Nothing is extracted or
    /// run.
    pub fn download(&self, url: &str, max_size: u64) -> Result<Download> {
        if !is_https(url) {
            bail!("url must be an https:// URL, found `{url}`");
        }
        let file = tempfile::Builder::new()
            .prefix(".download-")
            .tempfile_in(&self.dir)
            .with_context(|| format!("cannot create a download file in {}", self.dir.display()))?;
        let output = Command::new(CURL)
            .args([
                // `-q` must come first: it stops curl from reading
                // ~/.curlrc, which could otherwise turn off TLS checks.
                "-q",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--max-time",
                MAX_TIME_SECONDS,
                "--max-filesize",
                &max_size.to_string(),
                "--output",
            ])
            .arg(file.path())
            .arg(url)
            .output()
            .with_context(|| format!("cannot run {CURL}"))?;
        let too_large = || TooLarge {
            url: url.to_owned(),
            max_size,
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() == Some(CURL_TOO_LARGE)
                || stderr.contains(CURL_TOO_LARGE_MESSAGE)
            {
                return Err(too_large().into());
            }
            bail!("cannot download {url}: {}", stderr.trim());
        }
        let (sha256, size) = sha256_file(file.path())?;
        if size > max_size {
            return Err(too_large().into());
        }
        Ok(Download {
            file,
            url: url.to_owned(),
            sha256,
            size,
        })
    }

    /// Moves a download into the cache under its digest. When a matching
    /// entry is already there, the download is dropped instead.
    pub fn store(&self, download: Download) -> Result<PathBuf> {
        if let Some(path) = self.lookup(&download.sha256, download.size)? {
            return Ok(path);
        }
        let path = self.path(&download.sha256);
        download
            .file
            .persist(&path)
            .map_err(|err| err.error)
            .with_context(|| format!("cannot move the download into {}", path.display()))?;
        Ok(path)
    }
}

/// The cache folder for this platform, from `HOME` and `XDG_CACHE_HOME`.
fn default_dir(macos: bool, home: &Path, xdg_cache_home: Option<&Path>) -> PathBuf {
    if macos {
        return home.join("Library/Caches/dev.stackly.xtask/runtime");
    }
    xdg_cache_home
        .filter(|xdg| xdg.is_absolute())
        .map_or_else(|| home.join(".cache"), Path::to_path_buf)
        .join("stackly-xtask/runtime")
}

/// Deletes a cache entry, whether it is a file, a link, or a folder.
fn remove_entry(path: &Path) -> Result<()> {
    let result = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    };
    result.with_context(|| format!("cannot delete {}", path.display()))
}

/// The SHA-256 of a file as 64 lowercase hex characters, and its size.
pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let size = stream(path, |chunk| hasher.update(chunk))?;
    Ok((hex(&hasher.finalize()), size))
}

/// The SHA-512 of a file as 128 lowercase hex characters.
pub fn sha512_file(path: &Path) -> Result<String> {
    let mut hasher = Sha512::new();
    stream(path, |chunk| hasher.update(chunk))?;
    Ok(hex(&hasher.finalize()))
}

/// Feeds the file to `sink` in 64 KiB chunks and returns its size.
fn stream(path: &Path, mut sink: impl FnMut(&[u8])) -> Result<u64> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut size: u64 = 0;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => return Ok(size),
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        sink(&buffer[..read]);
        size += read as u64;
    }
}

/// Bytes as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of the three bytes `abc` (FIPS 180-2 test vector).
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn cache() -> (tempfile::TempDir, Cache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().join("runtime")).unwrap();
        (dir, cache)
    }

    #[test]
    fn digests_match_the_published_test_vectors() {
        let (_dir, cache) = cache();
        let path = cache.path("input");
        fs::write(&path, "abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), (ABC_SHA256.to_owned(), 3));
        assert_eq!(
            sha512_file(&path).unwrap(),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn a_matching_entry_is_found() {
        let (_dir, cache) = cache();
        fs::write(cache.path(ABC_SHA256), "abc").unwrap();
        assert_eq!(
            cache.lookup(ABC_SHA256, 3).unwrap(),
            Some(cache.path(ABC_SHA256))
        );
        assert_eq!(cache.lookup(&"0".repeat(64), 3).unwrap(), None);
    }

    #[test]
    fn an_entry_changed_by_one_byte_is_deleted() {
        let (_dir, cache) = cache();
        fs::write(cache.path(ABC_SHA256), "abd").unwrap();
        assert_eq!(cache.lookup(ABC_SHA256, 3).unwrap(), None);
        assert!(!cache.path(ABC_SHA256).exists());
    }

    #[test]
    fn an_entry_of_the_wrong_size_or_kind_is_deleted() {
        let (_dir, cache) = cache();
        fs::write(cache.path(ABC_SHA256), "abc").unwrap();
        assert_eq!(cache.lookup(ABC_SHA256, 4).unwrap(), None);
        assert!(!cache.path(ABC_SHA256).exists());

        fs::create_dir(cache.path(ABC_SHA256)).unwrap();
        assert_eq!(cache.lookup(ABC_SHA256, 3).unwrap(), None);
        assert!(!cache.path(ABC_SHA256).exists());
    }

    #[test]
    fn store_moves_a_download_under_its_digest_and_keeps_a_good_entry() {
        let (_dir, cache) = cache();
        let download = |content: &str| {
            let file = tempfile::Builder::new()
                .prefix(".download-")
                .tempfile_in(&cache.dir)
                .unwrap();
            fs::write(file.path(), content).unwrap();
            let (sha256, size) = sha256_file(file.path()).unwrap();
            Download {
                file,
                url: "https://example.org/abc".to_owned(),
                sha256,
                size,
            }
        };
        let stored = cache.store(download("abc")).unwrap();
        assert_eq!(stored, cache.path(ABC_SHA256));
        assert_eq!(fs::read_to_string(&stored).unwrap(), "abc");
        assert_eq!(cache.store(download("abc")).unwrap(), stored);
        let names: Vec<_> = fs::read_dir(&cache.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [ABC_SHA256]);
    }

    #[test]
    fn download_refuses_a_url_that_is_not_https_before_running_curl() {
        let (_dir, cache) = cache();
        let err = cache
            .download("http://example.org/a.tar.gz", 10)
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "url must be an https:// URL, found `http://example.org/a.tar.gz`"
        );
        assert_eq!(fs::read_dir(&cache.dir).unwrap().count(), 0);
    }

    #[test]
    fn the_cache_folder_follows_the_platform_convention() {
        let home = Path::new("/home/dev");
        assert_eq!(
            default_dir(true, home, Some(Path::new("/xdg"))),
            Path::new("/home/dev/Library/Caches/dev.stackly.xtask/runtime")
        );
        assert_eq!(
            default_dir(false, home, Some(Path::new("/xdg"))),
            Path::new("/xdg/stackly-xtask/runtime")
        );
        assert_eq!(
            default_dir(false, home, Some(Path::new("relative"))),
            Path::new("/home/dev/.cache/stackly-xtask/runtime")
        );
        assert_eq!(
            default_dir(false, home, None),
            Path::new("/home/dev/.cache/stackly-xtask/runtime")
        );
    }
}
