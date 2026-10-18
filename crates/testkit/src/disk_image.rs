use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::TestRoot;

/// Counter that makes each image file name unique within the process.
static IMAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A small APFS disk image attached at a folder inside a [`TestRoot`], so a
/// test can put another device inside a tree.
///
/// The image file lives at the top of the test root and is attached with
/// `hdiutil attach -nobrowse -mountpoint`. Drop detaches it with
/// `hdiutil detach -force`, so drop the image before the [`TestRoot`] that
/// holds it: declare it after the root.
#[derive(Debug)]
pub struct DiskImage {
    mount_point: PathBuf,
}

impl DiskImage {
    /// Creates a 2 MB APFS image in `root` and attaches it at
    /// `mount_point`, a folder inside `root` that is created here and must
    /// not exist yet.
    ///
    /// # Errors
    ///
    /// Returns an error naming the failed step and `hdiutil`'s output when
    /// the mount point cannot be created or `hdiutil` cannot create or
    /// attach the image, for example in a sandbox without disk arbitration.
    pub fn attach(root: &TestRoot, mount_point: &Path) -> io::Result<Self> {
        let image = root.path().join(format!(
            "disk-{}-{}.dmg",
            std::process::id(),
            IMAGE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(mount_point)?;
        run_hdiutil(
            "create",
            &[
                "create".as_ref(),
                "-size".as_ref(),
                "2m".as_ref(),
                "-fs".as_ref(),
                "APFS".as_ref(),
                "-volname".as_ref(),
                "stackly-test".as_ref(),
                "-quiet".as_ref(),
                image.as_os_str(),
            ],
        )?;
        run_hdiutil(
            "attach",
            &[
                "attach".as_ref(),
                "-nobrowse".as_ref(),
                "-noverify".as_ref(),
                "-noautoopen".as_ref(),
                "-mountpoint".as_ref(),
                mount_point.as_os_str(),
                image.as_os_str(),
            ],
        )?;
        Ok(Self {
            mount_point: mount_point.to_path_buf(),
        })
    }

    /// Returns the folder the image is attached at.
    #[must_use]
    pub fn mount_point(&self) -> &Path {
        &self.mount_point
    }
}

impl Drop for DiskImage {
    fn drop(&mut self) {
        // Best effort: a failed detach leaves the image attached until the
        // test root is removed, and the next attach uses a new name.
        let _ = Command::new("hdiutil")
            .args(["detach", "-force", "-quiet"])
            .arg(&self.mount_point)
            .output();
    }
}

fn run_hdiutil(step: &str, args: &[&std::ffi::OsStr]) -> io::Result<()> {
    let output = Command::new("hdiutil").args(args).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "hdiutil {step} exited with {}: {}",
            output.status,
            describe(&output)
        )))
    }
}

fn describe(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!("{} {}", stderr.trim(), stdout.trim())
        .trim()
        .to_owned()
}
