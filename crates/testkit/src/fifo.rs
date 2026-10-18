use std::io;
use std::path::Path;
use std::process::Command;

/// Creates a FIFO at `path`, for tests that check a FIFO is never opened or
/// read. Testkit links no libc, so this runs the system `mkfifo` tool
/// instead of calling `mkfifo(2)`.
///
/// # Errors
///
/// Returns an error when the tool cannot run or exits with a failure, for
/// example because something already exists at `path`.
pub fn make_fifo(path: &Path) -> io::Result<()> {
    let status = Command::new("mkfifo").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "mkfifo {} exited with {status}",
            path.display()
        )))
    }
}
