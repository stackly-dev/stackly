//! What the kernel says about a process (spec 0010, *Adapter macOS*, rows
//! Waktu start, Executable, Session, Fd yang dipegang, and Anggota group).
//!
//! The facts come from `proc_pidinfo(PROC_PIDTBSDINFO)`. That call answers
//! `EPERM` for every process of another user, PID 1 included, so those
//! facts come from `sysctl {CTL_KERN, KERN_PROC, KERN_PROC_PID}` instead:
//! a PID that another user now holds is then proven not to be ours rather
//! than left unproven.

use std::ffi::OsStr;
use std::io;
use std::mem::{self, size_of};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use libc::{c_int, c_void};
use stackly_domain::{Pid, ProcessStart};
use stackly_platform::{FileIdentity, HoldsFile, ProcessError, ProcessFacts, UserId};

use super::sys;
use crate::identity::current_uid;

/// What `kinfo_proc` says about one process: the fields the `inspect`
/// fallback reads, and the signal sets the AC-1 test checks.
///
/// Only [`MacosProcessSupervision::inspect_kinfo`] hands it out, for the
/// integration test that proves the fallback reads the same values as
/// `PROC_PIDTBSDINFO`.
///
/// [`MacosProcessSupervision::inspect_kinfo`]: super::MacosProcessSupervision::inspect_kinfo
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KinfoFacts {
    /// `p_starttime`.
    pub start: ProcessStart,
    /// True when `p_stat` is `SSTOP`.
    pub stopped: bool,
    /// True when `p_stat` is `SZOMB`.
    pub zombie: bool,
    /// `e_ppid`.
    pub parent: libc::pid_t,
    /// `e_pgid`.
    pub group: libc::pid_t,
    /// `e_ucred.cr_uid`, the effective user.
    pub uid: libc::uid_t,
    /// `e_pcred.p_ruid`, the real user.
    pub real_uid: libc::uid_t,
    /// `e_pcred.p_svuid`, the saved user.
    pub saved_uid: libc::uid_t,
    /// `p_siglist`, the pending signals.
    pub pending: u32,
    /// `p_sigmask`, the blocked signals.
    pub blocked: libc::sigset_t,
    /// `p_sigignore`, the ignored signals.
    pub ignored: libc::sigset_t,
    /// `p_sigcatch`, the caught signals.
    pub caught: libc::sigset_t,
}

/// The facts of a live process, or `None` when no process or only a
/// zombie has the PID.
///
/// # Errors
///
/// Returns [`ProcessError::PermissionDenied`] for a process that runs a
/// binary of another user for us (our real or saved user, a foreign
/// effective user), and [`ProcessError::Io`] for any other failure.
pub(super) fn facts(pid: Pid) -> Result<Option<ProcessFacts>, ProcessError> {
    let base = match bsd_info(pid)? {
        BsdInfo::Gone => return Ok(None),
        BsdInfo::Info(info) => {
            if info.pbi_status == libc::SZOMB {
                return Ok(None);
            }
            Base {
                parent: info.pbi_ppid.cast_signed(),
                group: info.pbi_pgid.cast_signed(),
                uid: info.pbi_uid,
                start: start_time(info.pbi_start_tvsec, info.pbi_start_tvusec)?,
                stopped: info.pbi_status == libc::SSTOP,
            }
        }
        BsdInfo::Denied => match kinfo(pid)? {
            None => return Ok(None),
            Some(facts) if facts.zombie => return Ok(None),
            Some(facts) => {
                let ours = current_uid();
                if facts.uid != ours && (facts.real_uid == ours || facts.saved_uid == ours) {
                    return Err(ProcessError::PermissionDenied { pid });
                }
                Base {
                    parent: facts.parent,
                    group: facts.group,
                    uid: facts.uid,
                    start: facts.start,
                    stopped: facts.stopped,
                }
            }
        },
    };
    let Some(executable) = executable(pid)? else {
        return Ok(None);
    };
    let Some(session) = session(pid)? else {
        return Ok(None);
    };
    Ok(Some(base.into_facts(pid, session, executable)?))
}

/// The facts of a child read right after its spawn.
///
/// A child the OS already ended, for example a binary it refused during
/// exec, is a zombie that no `inspect` describes. What the kernel keeps for
/// the zombie stands in for it then, with the spawned program as the
/// executable, the PID as the session (`SETSID`), and `stopped` false, so
/// the supervisor sees it never stood suspended and gets its status from
/// `discard` (spec 0010, K7).
///
/// # Errors
///
/// The errors of [`facts`], and [`ProcessError::Io`] when the child left no
/// trace at all.
pub(super) fn spawned_facts(pid: Pid, program: &Path) -> Result<ProcessFacts, ProcessError> {
    if let Some(facts) = facts(pid)? {
        return Ok(facts);
    }
    match kinfo(pid)? {
        Some(zombie) if zombie.zombie => {
            let base = Base {
                parent: zombie.parent,
                group: if zombie.group > 0 {
                    zombie.group
                } else {
                    pid.as_i32()
                },
                uid: zombie.uid,
                start: zombie.start,
                stopped: false,
            };
            base.into_facts(pid, pid, program.to_path_buf())
        }
        _ => Err(ProcessError::Io {
            op: "inspect",
            path: None,
            kind: io::ErrorKind::NotFound,
            message: "the child vanished right after its spawn".to_owned(),
        }),
    }
}

/// True when the kernel no longer describes `pid` as a live process: it is
/// gone, a zombie, or on its way to being one. For an unreaped child that
/// means it is exiting, even while `waitpid(WNOHANG)` does not see it yet.
pub(super) fn exiting(pid: Pid) -> bool {
    match bsd_info(pid) {
        Ok(BsdInfo::Gone) => true,
        Ok(BsdInfo::Info(info)) => info.pbi_status == libc::SZOMB,
        Ok(BsdInfo::Denied) | Err(_) => false,
    }
}

/// Reads `kinfo_proc`, or `None` when the kernel has no process with the
/// PID (a result of length 0).
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when `sysctl` fails or answers a length
/// that is neither 0 nor the size of the struct.
pub(super) fn kinfo(pid: Pid) -> Result<Option<KinfoFacts>, ProcessError> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        pid.as_i32(),
    ];
    // SAFETY: kinfo_proc is plain data of integers and pointers, for which
    // all zero bytes are a valid value.
    let mut info: sys::kinfo_proc = unsafe { mem::zeroed() };
    let mut len = size_of::<sys::kinfo_proc>();
    // SAFETY: the name array has the 4 entries passed, `info` is writable
    // for `len` bytes, and no new value is set.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            (&raw mut info).cast::<c_void>(),
            &raw mut len,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(io_error("sysctl kern.proc.pid"));
    }
    if len == 0 {
        return Ok(None);
    }
    if len != size_of::<sys::kinfo_proc>() {
        return Err(ProcessError::Io {
            op: "sysctl kern.proc.pid",
            path: None,
            kind: io::ErrorKind::InvalidData,
            message: format!(
                "the kernel answered {len} bytes, not the {} of kinfo_proc",
                size_of::<sys::kinfo_proc>()
            ),
        });
    }
    let proc = &info.kp_proc;
    let eproc = &info.kp_eproc;
    let status = u32::from(proc.p_stat.cast_unsigned());
    let seconds = u64::try_from(proc.p_starttime.tv_sec).map_err(|_| bad_start())?;
    let micros = u64::try_from(proc.p_starttime.tv_usec).map_err(|_| bad_start())?;
    Ok(Some(KinfoFacts {
        start: start_time(seconds, micros)?,
        stopped: status == libc::SSTOP,
        zombie: status == libc::SZOMB,
        parent: eproc.e_ppid,
        group: eproc.e_pgid,
        uid: eproc.e_ucred.cr_uid,
        real_uid: eproc.e_pcred.p_ruid,
        saved_uid: eproc.e_pcred.p_svuid,
        pending: proc.p_siglist.cast_unsigned(),
        blocked: proc.p_sigmask,
        ignored: proc.p_sigignore,
        caught: proc.p_sigcatch,
    }))
}

/// Tells whether `pid` holds `file` open, by device and inode.
///
/// # Errors
///
/// Returns [`ProcessError::NotFound`] when the process is gone, and
/// [`ProcessError::Io`] when the listing fails otherwise.
pub(super) fn holds_file(pid: Pid, file: FileIdentity) -> Result<HoldsFile, ProcessError> {
    let fds = match list_fds(pid) {
        Ok(fds) => fds,
        Err(Listing::Denied) => return Ok(HoldsFile::Unreadable),
        Err(Listing::Gone) => return Err(ProcessError::NotFound { pid }),
        Err(Listing::Failed(error)) => return Err(error),
    };
    let vnode = u32::try_from(libc::PROX_FDTYPE_VNODE).unwrap_or(u32::MAX);
    for fd in fds.iter().filter(|fd| fd.proc_fdtype == vnode) {
        match vnode_identity(pid, fd.proc_fd) {
            Ok(Some(identity)) if identity == file => return Ok(HoldsFile::Yes),
            Ok(_) => {}
            Err(Listing::Denied) => return Ok(HoldsFile::Unreadable),
            Err(Listing::Gone) => return Err(ProcessError::NotFound { pid }),
            Err(Listing::Failed(error)) => return Err(error),
        }
    }
    Ok(HoldsFile::No)
}

/// The open descriptor numbers of `pid`, ascending.
///
/// # Errors
///
/// Returns [`ProcessError::NotFound`] when the process is gone,
/// [`ProcessError::PermissionDenied`] for a process of another user, and
/// [`ProcessError::Io`] otherwise.
pub(super) fn fd_numbers(pid: Pid) -> Result<Vec<i32>, ProcessError> {
    let mut numbers: Vec<i32> = match list_fds(pid) {
        Ok(fds) => fds.iter().map(|fd| fd.proc_fd).collect(),
        Err(Listing::Denied) => return Err(ProcessError::PermissionDenied { pid }),
        Err(Listing::Gone) => return Err(ProcessError::NotFound { pid }),
        Err(Listing::Failed(error)) => return Err(error),
    };
    numbers.sort_unstable();
    Ok(numbers)
}

/// The live members of a process group, ascending, through
/// `proc_listpids(PROC_PGRP_ONLY)`, which answers a number of bytes.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when the list cannot be read.
pub(super) fn group_members(group: Pid) -> Result<Vec<Pid>, ProcessError> {
    let group_id = group.get();
    // SAFETY: a null buffer of size 0 only asks for the size needed.
    let needed = unsafe { libc::proc_listpids(sys::PROC_PGRP_ONLY, group_id, ptr::null_mut(), 0) };
    if needed < 0 {
        return Err(io_error("proc_listpids"));
    }
    // Room for a few more processes than were there a moment ago.
    let count = usize::try_from(needed).unwrap_or(0) / size_of::<libc::pid_t>() + 16;
    let mut pids: Vec<libc::pid_t> = vec![0; count];
    let bytes = c_int::try_from(count * size_of::<libc::pid_t>()).unwrap_or(c_int::MAX);
    // SAFETY: the buffer holds `bytes` bytes of pid_t values.
    let filled = unsafe {
        libc::proc_listpids(
            sys::PROC_PGRP_ONLY,
            group_id,
            pids.as_mut_ptr().cast::<c_void>(),
            bytes,
        )
    };
    if filled < 0 {
        return Err(io_error("proc_listpids"));
    }
    pids.truncate(usize::try_from(filled).unwrap_or(0) / size_of::<libc::pid_t>());
    let mut members = Vec::with_capacity(pids.len());
    for pid in pids {
        let Some(pid) = u32::try_from(pid).ok().and_then(|pid| Pid::new(pid).ok()) else {
            continue;
        };
        let live = match bsd_info(pid)? {
            BsdInfo::Gone => false,
            BsdInfo::Info(info) => info.pbi_status != libc::SZOMB,
            BsdInfo::Denied => true,
        };
        if live {
            members.push(pid);
        }
    }
    members.sort_unstable();
    members.dedup();
    Ok(members)
}

/// The facts both sources share, before the executable and session are
/// read.
struct Base {
    parent: libc::pid_t,
    group: libc::pid_t,
    uid: libc::uid_t,
    start: ProcessStart,
    stopped: bool,
}

impl Base {
    fn into_facts(
        self,
        pid: Pid,
        session: Pid,
        executable: PathBuf,
    ) -> Result<ProcessFacts, ProcessError> {
        let group = u32::try_from(self.group)
            .ok()
            .and_then(|group| Pid::new(group).ok())
            .ok_or_else(|| ProcessError::Io {
                op: "inspect",
                path: None,
                kind: io::ErrorKind::InvalidData,
                message: format!("the process group {} is not a process ID", self.group),
            })?;
        Ok(ProcessFacts {
            pid,
            parent: u32::try_from(self.parent)
                .ok()
                .and_then(|parent| Pid::new(parent).ok()),
            group,
            session,
            uid: UserId::Unix(self.uid),
            start: self.start,
            executable,
            stopped: self.stopped,
        })
    }
}

/// The answer of `PROC_PIDTBSDINFO`.
enum BsdInfo {
    Info(libc::proc_bsdinfo),
    /// `ESRCH`: no process, or a zombie.
    Gone,
    /// `EPERM`: a process of another user.
    Denied,
}

fn bsd_info(pid: Pid) -> Result<BsdInfo, ProcessError> {
    // SAFETY: proc_bsdinfo is plain integers, for which all zero bytes are
    // a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { mem::zeroed() };
    let size = c_int::try_from(size_of::<libc::proc_bsdinfo>()).unwrap_or(c_int::MAX);
    // SAFETY: `info` is writable for `size` bytes.
    let filled = unsafe {
        libc::proc_pidinfo(
            pid.as_i32(),
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    if filled == size {
        return Ok(BsdInfo::Info(info));
    }
    if filled > 0 {
        return Err(ProcessError::Io {
            op: "proc_pidinfo PROC_PIDTBSDINFO",
            path: None,
            kind: io::ErrorKind::InvalidData,
            message: format!("the kernel answered {filled} bytes, not {size}"),
        });
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(BsdInfo::Gone),
        Some(libc::EPERM) => Ok(BsdInfo::Denied),
        _ => Err(ProcessError::io(
            "proc_pidinfo PROC_PIDTBSDINFO",
            None,
            &error,
        )),
    }
}

/// The executable of a live process through `proc_pidpath`, or `None` when
/// it is gone. Any other error, `ENOENT` for an executable unlinked under a
/// live process included, is an error, never a different executable.
fn executable(pid: Pid) -> Result<Option<PathBuf>, ProcessError> {
    let capacity = usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).unwrap_or(4096);
    let mut buffer = vec![0_u8; capacity];
    let size = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: the buffer is writable for `size` bytes.
    let filled =
        unsafe { libc::proc_pidpath(pid.as_i32(), buffer.as_mut_ptr().cast::<c_void>(), size) };
    if filled <= 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(ProcessError::io("proc_pidpath", None, &error)),
        };
    }
    buffer.truncate(usize::try_from(filled).unwrap_or(0));
    Ok(Some(PathBuf::from(OsStr::from_bytes(&buffer))))
}

/// The session of a live process through `getsid`, or `None` when it is
/// gone.
fn session(pid: Pid) -> Result<Option<Pid>, ProcessError> {
    // SAFETY: getsid takes an integer and touches no memory of ours.
    let session = unsafe { libc::getsid(pid.as_i32()) };
    if session == -1 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(ProcessError::io("getsid", None, &error)),
        };
    }
    u32::try_from(session)
        .ok()
        .and_then(|session| Pid::new(session).ok())
        .map(Some)
        .ok_or_else(|| ProcessError::Io {
            op: "getsid",
            path: None,
            kind: io::ErrorKind::InvalidData,
            message: format!("the session {session} is not a process ID"),
        })
}

/// Why a descriptor listing has no answer.
enum Listing {
    /// `EPERM`: a process of another user.
    Denied,
    /// `ESRCH`: the process is gone.
    Gone,
    Failed(ProcessError),
}

fn listing_error(op: &'static str) -> Listing {
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EPERM) => Listing::Denied,
        Some(libc::ESRCH) => Listing::Gone,
        _ => Listing::Failed(ProcessError::io(op, None, &error)),
    }
}

fn list_fds(pid: Pid) -> Result<Vec<libc::proc_fdinfo>, Listing> {
    const OP: &str = "proc_pidinfo PROC_PIDLISTFDS";
    clear_errno();
    // SAFETY: a null buffer of size 0 only asks for the size needed.
    let needed =
        unsafe { libc::proc_pidinfo(pid.as_i32(), libc::PROC_PIDLISTFDS, 0, ptr::null_mut(), 0) };
    if needed <= 0 {
        if needed == 0 && io::Error::last_os_error().raw_os_error() == Some(0) {
            return Ok(Vec::new());
        }
        return Err(listing_error(OP));
    }
    let item = size_of::<libc::proc_fdinfo>();
    // Room for a few more descriptors than were open a moment ago.
    let count = usize::try_from(needed).unwrap_or(0) / item + 16;
    let mut fds = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        count
    ];
    let bytes = c_int::try_from(count * item).unwrap_or(c_int::MAX);
    clear_errno();
    // SAFETY: the buffer holds `bytes` bytes of proc_fdinfo values.
    let filled = unsafe {
        libc::proc_pidinfo(
            pid.as_i32(),
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast::<c_void>(),
            bytes,
        )
    };
    if filled <= 0 {
        if filled == 0 && io::Error::last_os_error().raw_os_error() == Some(0) {
            return Ok(Vec::new());
        }
        return Err(listing_error(OP));
    }
    fds.truncate(usize::try_from(filled).unwrap_or(0) / item);
    Ok(fds)
}

/// The device and inode behind one vnode descriptor, or `None` when the
/// descriptor closed in the meantime.
fn vnode_identity(pid: Pid, fd: i32) -> Result<Option<FileIdentity>, Listing> {
    // SAFETY: the struct is plain data, for which all zero bytes are a
    // valid value.
    let mut info: sys::vnode_fdinfowithpath = unsafe { mem::zeroed() };
    let size = c_int::try_from(size_of::<sys::vnode_fdinfowithpath>()).unwrap_or(c_int::MAX);
    // SAFETY: `info` is writable for `size` bytes.
    let filled = unsafe {
        libc::proc_pidfdinfo(
            pid.as_i32(),
            fd,
            sys::PROC_PIDFDVNODEPATHINFO,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    if filled == size {
        let stat = info.pvip.vip_vi.vi_stat;
        // `vst_dev` is the dev_t of stat(2), a signed 32 bit number that
        // `MetadataExt::dev` widens with its sign, so widen it the same way.
        let device = i64::from(stat.vst_dev.cast_signed()).cast_unsigned();
        return Ok(Some(FileIdentity {
            device,
            inode: stat.vst_ino,
        }));
    }
    if filled > 0 {
        return Err(Listing::Failed(ProcessError::Io {
            op: "proc_pidfdinfo PROC_PIDFDVNODEPATHINFO",
            path: None,
            kind: io::ErrorKind::InvalidData,
            message: format!("the kernel answered {filled} bytes, not {size}"),
        }));
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EBADF) => Ok(None),
        _ => Err(listing_error("proc_pidfdinfo PROC_PIDFDVNODEPATHINFO")),
    }
}

fn start_time(seconds: u64, micros: u64) -> Result<ProcessStart, ProcessError> {
    let micros = u32::try_from(micros).map_err(|_| bad_start())?;
    ProcessStart::new(seconds, micros).map_err(|_| bad_start())
}

fn bad_start() -> ProcessError {
    ProcessError::Io {
        op: "inspect",
        path: None,
        kind: io::ErrorKind::InvalidData,
        message: "the kernel answered a start time out of range".to_owned(),
    }
}

fn io_error(op: &'static str) -> ProcessError {
    ProcessError::io(op, None, &io::Error::last_os_error())
}

/// Sets this thread's `errno` to 0, so a call that answers 0 for an empty
/// list can be told from one that failed.
fn clear_errno() {
    // SAFETY: __error returns the address of this thread's errno, which
    // stays valid for the thread's life.
    unsafe { *libc::__error() = 0 };
}
