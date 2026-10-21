//! The code signing adapter (spec 0018, *Validasi binary*): `/usr/bin/codesign`
//! for signatures and `getxattr` for quarantine.
//!
//! The one tool it runs is `/usr/bin/codesign`, which is part of base macOS,
//! so a Mac without the Command Line Tools has it. The spawn goes around
//! [`ProcessSupervision`](stackly_platform::ProcessSupervision) because it
//! never runs the code of an artifact (spec 0004, *Key invariants*), but it
//! uses the same `posix_spawn` and the same kill as the process adapter
//! (spec 0010, K9): the program is fixed, stdin is `/dev/null`, the
//! environment is empty, only its own PID is reaped (no `waitpid(-1)` and no
//! `SIGCHLD` handler), and a call past its time limit is stopped through
//! the process adapter's group kill.

use std::ffi::{CString, c_void};
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use stackly_domain::{Pid, TeamId};
use stackly_platform::{
    CodeAssessment, CodeSigning, PlatformError, ProcessError, QuarantineInfo, SignatureKind,
};

use crate::process::{kill, spawn};

/// The only program this module runs.
const CODESIGN: &str = "/usr/bin/codesign";

/// Most paths one `--sign` call gets.
const SIGN_BATCH: usize = 64;

/// The longest one `--sign` call may take.
const SIGN_TIMEOUT: Duration = Duration::from_secs(120);

/// The longest a read call (`--verify`, `-dv`) may take.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// The most output kept from one call.
const OUTPUT_MAX: usize = 64 * 1024;

/// The extended attribute that marks a file downloaded by a quarantining
/// agent.
const QUARANTINE_ATTR: &str = "com.apple.quarantine";

/// Reads and writes signatures through `/usr/bin/codesign`, and reads
/// quarantine with `getxattr` without following symlinks.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacosCodeSigning;

impl MacosCodeSigning {
    /// Creates the adapter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// What a finished `codesign` call left.
struct Finished {
    code: Option<i32>,
    stderr: String,
}

impl CodeSigning for MacosCodeSigning {
    fn assess(&self, path: &Path) -> Result<CodeAssessment, PlatformError> {
        let verify = run(
            &["--verify".to_owned(), "--strict".to_owned(), text(path)?],
            READ_TIMEOUT,
        )?;
        let display = run(
            &["-dv".to_owned(), "--verbose=2".to_owned(), text(path)?],
            READ_TIMEOUT,
        )?;
        let strict_valid = verify.code == Some(0);
        let detail = (!strict_valid)
            .then(|| first_line(&verify.stderr).replace(&path.display().to_string(), "<file>"))
            .filter(|line| !line.is_empty());
        Ok(CodeAssessment {
            signature: parse_display(&display.stderr),
            strict_valid,
            detail,
        })
    }

    fn sign_adhoc(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        for batch in paths.chunks(SIGN_BATCH) {
            let mut args = vec!["--force".to_owned(), "--sign".to_owned(), "-".to_owned()];
            for path in batch {
                args.push(text(path)?);
            }
            let done = run(&args, SIGN_TIMEOUT)?;
            if done.code != Some(0) {
                return Err(PlatformError::Io {
                    op: "codesign --sign",
                    path: batch.first().cloned().unwrap_or_default(),
                    kind: std::io::ErrorKind::Other,
                    message: first_line(&done.stderr),
                });
            }
        }
        Ok(())
    }

    fn quarantine(&self, path: &Path) -> Result<Option<QuarantineInfo>, PlatformError> {
        let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| invalid(path))?;
        let c_name = CString::new(QUARANTINE_ATTR).map_err(|_| invalid(path))?;
        let mut value = vec![0_u8; 512];
        // SAFETY: `c_path` and `c_name` are NUL terminated and live through
        // the call, and `value` is a writable buffer of the length passed.
        let read = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_mut_ptr().cast::<c_void>(),
                value.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        if read < 0 {
            let error = std::io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::ENOATTR) => Ok(None),
                _ => Err(PlatformError::from_io("getxattr", path, &error)),
            };
        }
        let len = usize::try_from(read).unwrap_or(0).min(value.len());
        let text = String::from_utf8_lossy(&value[..len]);
        // `0083;5f3c1a2b;Safari;UUID`: flags, time, agent, UUID.
        let agent = text
            .split(';')
            .nth(2)
            .map(str::trim)
            .filter(|agent| !agent.is_empty())
            .map(str::to_owned);
        Ok(Some(QuarantineInfo { agent }))
    }
}

fn invalid(path: &Path) -> PlatformError {
    PlatformError::Io {
        op: "codesign",
        path: path.to_path_buf(),
        kind: std::io::ErrorKind::InvalidInput,
        message: "the path holds a NUL byte".to_owned(),
    }
}

fn text(path: &Path) -> Result<String, PlatformError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| PlatformError::Io {
            op: "codesign",
            path: path.to_path_buf(),
            kind: std::io::ErrorKind::InvalidInput,
            message: "the path is not UTF-8".to_owned(),
        })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

fn process_error(err: &ProcessError) -> PlatformError {
    PlatformError::Io {
        op: "codesign",
        path: PathBuf::from(CODESIGN),
        kind: std::io::ErrorKind::Other,
        message: err.to_string(),
    }
}

/// Runs `/usr/bin/codesign` with `args`, stdin from `/dev/null`, an empty
/// environment, and stdout and stderr read up to 64 KiB, and stops it with
/// its process group after `timeout`.
fn run(args: &[String], timeout: Duration) -> Result<Finished, PlatformError> {
    let (out_read, out_write) = spawn::pipe().map_err(|err| process_error(&err))?;
    let (err_read, err_write) = spawn::pipe().map_err(|err| process_error(&err))?;
    let pid = spawn::launch(&spawn::Launch {
        program: Path::new(CODESIGN),
        args,
        env: &[],
        cwd: Path::new("/"),
        stdin: None,
        stdout: std::os::fd::AsFd::as_fd(&out_write),
        stderr: std::os::fd::AsFd::as_fd(&err_write),
        lock: None,
        suspended: false,
    })
    .map_err(|err| process_error(&err))?;
    drop(out_write);
    drop(err_write);
    let readers = [out_read, err_read].map(|fd| thread::spawn(move || read_capped(fd)));
    let code = wait(pid, timeout);
    let mut outputs = readers.map(|reader| reader.join().unwrap_or_default());
    let stderr = std::mem::take(&mut outputs[1]);
    match code {
        Ok(code) => Ok(Finished {
            code,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }),
        Err(error) => Err(error),
    }
}

/// Reads `fd` to its end and keeps the first [`OUTPUT_MAX`] bytes, still
/// draining the rest so the child never blocks on a full pipe.
fn read_capped(fd: OwnedFd) -> Vec<u8> {
    let mut file = std::fs::File::from(fd);
    let mut kept = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match file.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = OUTPUT_MAX.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..read.min(room)]);
            }
        }
    }
    kept
}

/// Waits for `pid`, and stops its group once `timeout` has passed. Only this
/// PID is waited for.
fn wait(pid: Pid, timeout: Duration) -> Result<Option<i32>, PlatformError> {
    let deadline = Instant::now() + timeout;
    let mut status: libc::c_int = 0;
    loop {
        // SAFETY: waitpid writes one c_int into `status`, which is valid for
        // the call, and it waits for this child only.
        let waited = unsafe { libc::waitpid(pid.as_i32(), &raw mut status, libc::WNOHANG) };
        if waited == pid.as_i32() {
            return Ok(libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status)));
        }
        if waited < 0 {
            let error = std::io::Error::last_os_error();
            return Err(PlatformError::from_io(
                "waitpid",
                Path::new(CODESIGN),
                &error,
            ));
        }
        if Instant::now() >= deadline {
            let _ = kill::signal_group(pid, true);
            // SAFETY: as above; the child was signalled and exits soon.
            let _ = unsafe { libc::waitpid(pid.as_i32(), &raw mut status, 0) };
            return Err(PlatformError::Io {
                op: "codesign",
                path: PathBuf::from(CODESIGN),
                kind: std::io::ErrorKind::TimedOut,
                message: format!("codesign did not finish in {} seconds", timeout.as_secs()),
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Reads `codesign -dv --verbose=2` output (the format the spike of spec
/// 0003 already read).
pub(crate) fn parse_display(text: &str) -> SignatureKind {
    if text.contains("code object is not signed at all") {
        return SignatureKind::Unsigned;
    }
    let authority = text
        .lines()
        .find_map(|line| line.strip_prefix("Authority="))
        .map(str::to_owned);
    let team = text
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .and_then(|team| team.parse::<TeamId>().ok());
    let flags = text
        .lines()
        .find(|line| line.starts_with("CodeDirectory "))
        .unwrap_or_default();
    match (authority, team) {
        (Some(authority), Some(team)) if authority.starts_with("Developer ID Application") => {
            SignatureKind::DeveloperId { team, authority }
        }
        (Some(authority), _) => SignatureKind::OtherSigned { authority },
        (None, _) if flags.contains("linker-signed") => SignatureKind::LinkerSigned,
        (None, _) => SignatureKind::Adhoc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_is_classified_from_its_authority_and_flags() {
        let go = "Executable=/x/nats-server\nIdentifier=a.out\nCodeDirectory v=20400 size=136990 flags=0x20002(adhoc,linker-signed) hashes=4278+0 location=embedded\nSignature=adhoc\nTeamIdentifier=not set\n";
        assert_eq!(parse_display(go), SignatureKind::LinkerSigned);
        assert_eq!(
            parse_display(&go.replace("(adhoc,linker-signed)", "(adhoc)")),
            SignatureKind::Adhoc
        );
        let oracle = "CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=1+7\nAuthority=Developer ID Application: Oracle America, Inc. (VB5E2TV963)\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\nTeamIdentifier=VB5E2TV963\n";
        assert_eq!(
            parse_display(oracle),
            SignatureKind::DeveloperId {
                team: "VB5E2TV963".parse().expect("a valid team ID"),
                authority: "Developer ID Application: Oracle America, Inc. (VB5E2TV963)".to_owned(),
            }
        );
        let apple = "CodeDirectory v=20400 flags=0x0(none)\nAuthority=Software Signing\nAuthority=Apple Code Signing Certification Authority\nTeamIdentifier=not set\n";
        assert_eq!(
            parse_display(apple),
            SignatureKind::OtherSigned {
                authority: "Software Signing".to_owned()
            }
        );
        assert_eq!(
            parse_display("/x: code object is not signed at all\n"),
            SignatureKind::Unsigned
        );
    }
}
