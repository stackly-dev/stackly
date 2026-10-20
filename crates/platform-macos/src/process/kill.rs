//! The only file of the workspace that names `SIGKILL` (spec 0010, *Key
//! invariants*).
//!
//! A kill reaches only two kinds of process: a suspended main process that
//! never ran, through [`kill_unrun`], and the group of a helper that is not
//! reaped yet, through [`signal_group`]. Both are `pub(crate)`, so the
//! codesign helper of feature 18 stops its own child with the same code
//! (spec 0004, K9).

use std::io;

use stackly_domain::Pid;

/// Sends `SIGKILL` to a suspended child that never ran.
///
/// The caller holds the child's slot, so the PID is not reaped and cannot
/// belong to another process.
///
/// # Errors
///
/// Returns the OS error of `kill(2)`, `ESRCH` when the child is gone.
pub(crate) fn kill_unrun(pid: Pid) -> io::Result<()> {
    send(pid.as_i32(), libc::SIGKILL)
}

/// Sends `SIGTERM`, or `SIGKILL` when `force` is true, to every process in
/// the group `group`.
///
/// The caller holds the slot of the helper that leads the group and has
/// not reaped it, so the group ID is still that helper's.
///
/// # Errors
///
/// Returns the OS error of `kill(2)`, `ESRCH` when the group is empty.
pub(crate) fn signal_group(group: Pid, force: bool) -> io::Result<()> {
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    send(-group.as_i32(), signal)
}

fn send(target: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: kill takes two integers and touches no memory of ours.
    let result = unsafe { libc::kill(target, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
