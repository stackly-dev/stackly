use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::LineSink;

/// A log file that rotates by size: `<name>`, then `<name>.1` (newest) up
/// to `<name>.<kept - 1>` (oldest).
///
/// The file is opened for append with mode 0600 (when created), without
/// following a symlink, and its size is counted from what is already there.
/// Before a line would take the file past `max_bytes`, the oldest file is
/// removed, every other one moves up by one, and a new `<name>` is opened.
/// Rotation only ever touches the files with exactly these names in `dir`.
///
/// The daemon log uses it with [`super::LOG_FILE_MAX`] and
/// [`super::LOG_FILES_KEPT`]; feature 27 may reuse it for instance logs.
#[derive(Debug)]
pub struct RotatingFile {
    dir: PathBuf,
    name: String,
    max_bytes: u64,
    kept: usize,
    file: File,
    size: u64,
}

impl RotatingFile {
    /// Opens (or creates) `dir/name`, keeping at most `kept` files of at
    /// most about `max_bytes` each. `kept` below 1 counts as 1.
    ///
    /// # Errors
    ///
    /// Returns the OS error when the file cannot be opened or inspected,
    /// and [`io::ErrorKind::InvalidInput`] when the path holds something
    /// other than a regular file (a symlink fails to open).
    pub fn open(dir: &Path, name: &str, max_bytes: u64, kept: usize) -> io::Result<Self> {
        let path = dir.join(name);
        let file = open_append(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            name: name.to_owned(),
            max_bytes,
            kept: kept.max(1),
            file,
            size,
        })
    }

    /// Returns the path of the current file, `dir/name`.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.name)
    }

    /// Returns how many bytes the current file holds.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    fn numbered(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{}.{index}", self.name))
    }

    fn rotate(&mut self) -> io::Result<()> {
        if self.kept > 1 {
            remove_if_present(&self.numbered(self.kept - 1))?;
            for index in (1..self.kept - 1).rev() {
                rename_if_present(&self.numbered(index), &self.numbered(index + 1))?;
            }
            rename_if_present(&self.path(), &self.numbered(1))?;
        } else {
            remove_if_present(&self.path())?;
        }
        self.file = open_append(&self.path())?;
        self.size = 0;
        Ok(())
    }
}

impl LineSink for RotatingFile {
    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        let len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.size > 0 && self.size.saturating_add(len) > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(line)?;
        self.size = self.size.saturating_add(len);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        // O_NONBLOCK keeps a FIFO at the path from blocking the open; it
        // changes nothing for a regular file.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if file.metadata()?.file_type().is_file() {
        Ok(file)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ))
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn rename_if_present(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use stackly_testkit::{TestRoot, make_fifo};

    use super::*;

    const NAME: &str = "stacklyd.log";

    fn line(number: usize) -> Vec<u8> {
        format!("line {number:04}\n").into_bytes()
    }

    fn numbers(path: &Path) -> Vec<usize> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| line.trim_start_matches("line ").parse().unwrap())
            .collect()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn rotation_with_a_small_threshold_keeps_exactly_five_files_in_order() {
        let root = TestRoot::new().unwrap();
        // Each line is 10 bytes, so every file holds three lines.
        let mut file = RotatingFile::open(root.path(), NAME, 30, 5).unwrap();

        for number in 0..40 {
            file.write_line(&line(number)).unwrap();
        }

        assert_eq!(
            files(root.path()),
            [
                "stacklyd.log",
                "stacklyd.log.1",
                "stacklyd.log.2",
                "stacklyd.log.3",
                "stacklyd.log.4"
            ]
        );
        assert_eq!(numbers(&root.path().join(NAME)), [39]);
        assert_eq!(numbers(&root.path().join("stacklyd.log.1")), [36, 37, 38]);
        assert_eq!(numbers(&root.path().join("stacklyd.log.2")), [33, 34, 35]);
        assert_eq!(numbers(&root.path().join("stacklyd.log.3")), [30, 31, 32]);
        assert_eq!(numbers(&root.path().join("stacklyd.log.4")), [27, 28, 29]);
        for name in files(root.path()) {
            assert!(std::fs::metadata(root.path().join(&name)).unwrap().len() <= 30);
        }
    }

    #[test]
    fn a_new_file_is_private() {
        let root = TestRoot::new().unwrap();
        let mut file = RotatingFile::open(root.path(), NAME, 30, 5).unwrap();
        file.write_line(&line(0)).unwrap();

        let mode = std::fs::metadata(file.path()).unwrap().permissions().mode();

        assert_eq!(mode & 0o7777, 0o600);
    }

    #[test]
    fn the_size_counts_what_the_file_already_holds() {
        let root = TestRoot::new().unwrap();
        std::fs::write(root.path().join(NAME), line(0).repeat(2)).unwrap();

        let mut file = RotatingFile::open(root.path(), NAME, 30, 5).unwrap();
        assert_eq!(file.size(), 20);
        file.write_line(&line(1)).unwrap();
        file.write_line(&line(2)).unwrap();

        assert_eq!(numbers(&root.path().join("stacklyd.log.1")), [0, 0, 1]);
        assert_eq!(numbers(&root.path().join(NAME)), [2]);
    }

    #[test]
    fn rotation_never_touches_other_files_in_the_folder() {
        let root = TestRoot::new().unwrap();
        for name in ["stacklyd.log.5", "stacklyd.log.old", "other.log"] {
            std::fs::write(root.path().join(name), b"keep").unwrap();
        }
        let mut file = RotatingFile::open(root.path(), NAME, 10, 5).unwrap();

        for number in 0..20 {
            file.write_line(&line(number)).unwrap();
        }

        for name in ["stacklyd.log.5", "stacklyd.log.old", "other.log"] {
            assert_eq!(std::fs::read(root.path().join(name)).unwrap(), b"keep");
        }
    }

    #[test]
    fn a_line_longer_than_the_limit_still_lands_in_a_fresh_file() {
        let root = TestRoot::new().unwrap();
        let mut file = RotatingFile::open(root.path(), NAME, 5, 5).unwrap();

        file.write_line(&line(0)).unwrap();
        file.write_line(&line(1)).unwrap();

        assert_eq!(numbers(&root.path().join(NAME)), [1]);
        assert_eq!(numbers(&root.path().join("stacklyd.log.1")), [0]);
    }

    #[test]
    fn a_symlink_or_fifo_in_place_of_the_file_is_refused() {
        let root = TestRoot::new().unwrap();
        let target = root.path().join("target");
        std::fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, root.path().join(NAME)).unwrap();
        make_fifo(&root.path().join("fifo.log")).unwrap();

        assert!(RotatingFile::open(root.path(), NAME, 30, 5).is_err());
        // Without a reader the nonblocking open fails right away (ENXIO).
        assert!(RotatingFile::open(root.path(), "fifo.log", 30, 5).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    }
}
