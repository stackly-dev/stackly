//! The three ways `serve` breaks the process contract of spec 0010 on
//! purpose (spec 0036, *Fake server*), only for the `testing/fake-rogue`
//! package of the conformance harness. Each is off by default and acts
//! once: the process it starts carries an internal flag that stops a
//! repeat.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use crate::Failure;
use crate::args::Flags;

/// What `serve` does after [`act`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Acted {
    /// Nothing changed: go on serving.
    Serve,
    /// The process started its replacement and exits 0 before listening.
    Exit,
}

/// Applies `--close-lock-fd`, `--re-exec`, and `--background`:
///
/// - `--close-lock-fd=true` replaces this process through `exec` with
///   `/bin/sh -c 'exec 3>&-; exec "$0" "$@"'`, which closes fd 3 (the
///   instance lock) and runs this binary again with `--lock-fd-closed=true`.
/// - `--re-exec=true` copies this binary to `<data>/fake-copy` and `exec`s
///   the copy with `--re-executed=true`, so the executable changes.
/// - `--background=true` spawns this binary again with
///   `--backgrounded=true`, which inherits fd 3 and the process group, and
///   this process exits 0 before it listens.
///
/// # Errors
///
/// A usage error for a flag that is not a boolean, and a runtime error
/// when the binary cannot be found, copied, or started.
pub(crate) fn act(flags: &Flags, data: &Path, args: &[String]) -> Result<Acted, Failure> {
    let close =
        flags.boolean("close-lock-fd", false)? && !flags.boolean("lock-fd-closed", false)?;
    let re_exec = flags.boolean("re-exec", false)? && !flags.boolean("re-executed", false)?;
    let background =
        flags.boolean("background", false)? && !flags.boolean("backgrounded", false)?;
    if !(close || re_exec || background) {
        return Ok(Acted::Serve);
    }
    let me = std::env::current_exe()
        .map_err(|error| Failure::Runtime(format!("cannot find this binary: {error}")))?;
    let again = |extra: &str| {
        let mut argv = vec!["serve".to_owned()];
        argv.extend(args.iter().cloned());
        argv.push(extra.to_owned());
        argv
    };
    if close {
        let error = Command::new("/bin/sh")
            .arg("-c")
            .arg("exec 3>&-; exec \"$0\" \"$@\"")
            .arg(&me)
            .args(again("--lock-fd-closed=true"))
            .exec();
        return Err(Failure::Runtime(format!("cannot exec /bin/sh: {error}")));
    }
    if re_exec {
        let copy = data.join("fake-copy");
        std::fs::copy(&me, &copy).map_err(|error| {
            Failure::Runtime(format!(
                "cannot copy this binary to {}: {error}",
                copy.display()
            ))
        })?;
        let error = Command::new(&copy).args(again("--re-executed=true")).exec();
        return Err(Failure::Runtime(format!(
            "cannot exec {}: {error}",
            copy.display()
        )));
    }
    Command::new(&me)
        .args(again("--backgrounded=true"))
        .spawn()
        .map_err(|error| Failure::Runtime(format!("cannot start the background copy: {error}")))?;
    Ok(Acted::Exit)
}
