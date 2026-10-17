//! The spike lock `<cache>/spike.lock` (spec 0003): one spike per Mac at a
//! time, and `--cleanup` or `--prune-cache` never touch a running spike.
//! The OS drops the lock when its process dies, so an interrupted run
//! never holds it.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// The lock file name inside the cache folder.
pub const LOCK_FILE: &str = "spike.lock";

/// The exclusive spike lock, held until dropped.
#[derive(Debug)]
pub struct SpikeLock {
    _file: File,
}

impl SpikeLock {
    /// Waits as long as it takes for the lock. The driver uses this, so
    /// spikes started in parallel run one after another.
    pub fn acquire(cache_dir: &Path) -> Result<Self> {
        let file = open(cache_dir)?;
        file.lock()
            .with_context(|| format!("cannot lock {}", cache_dir.join(LOCK_FILE).display()))?;
        Ok(Self { _file: file })
    }

    /// Waits at most `wait` for the lock, then fails with
    /// `spike in progress`.
    pub fn acquire_within(cache_dir: &Path, wait: Duration) -> Result<Self> {
        let file = open(cache_dir)?;
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(500));
                }
                Err(TryLockError::WouldBlock) => bail!("spike in progress"),
                Err(TryLockError::Error(err)) => {
                    return Err(err).with_context(|| {
                        format!("cannot lock {}", cache_dir.join(LOCK_FILE).display())
                    });
                }
            }
        }
    }
}

fn open(cache_dir: &Path) -> Result<File> {
    let path = cache_dir.join(LOCK_FILE);
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("cannot open {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_makes_a_bounded_wait_fail_with_spike_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let held = SpikeLock::acquire(dir.path()).unwrap();
        let err = SpikeLock::acquire_within(dir.path(), Duration::from_millis(600)).unwrap_err();
        assert_eq!(err.to_string(), "spike in progress");
        drop(held);
        SpikeLock::acquire_within(dir.path(), Duration::from_millis(600)).unwrap();
    }
}
