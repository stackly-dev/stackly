//! What the metrics sampler reads (spec 0028, *Port platform*): every PID
//! through `proc_listallpids`, and the counters of one process through
//! `proc_pid_rusage(RUSAGE_INFO_V4)`.
//!
//! The CPU times of `rusage_info_v4` are mach time units, not nanoseconds:
//! on Apple Silicon `mach_timebase_info` answers 125/3 (spec 0028
//! rationale, *Hasil spike*), so every time is converted before it leaves
//! the adapter. Disk bytes count only I/O that reached the disk.

use std::io;
use std::mem::{self, size_of};
use std::ptr;
use std::sync::OnceLock;
use std::time::Duration;

use libc::{c_int, c_void};
use stackly_domain::{Pid, ProcessCounters, ProcessStart};
use stackly_platform::ProcessError;

use super::inspect::{BsdInfo, bsd_info, start_time};
use super::sys;

/// Extra room past the count the kernel gave, for processes that start
/// between the two calls.
const LIST_SLACK: usize = 64;
/// How many times the list grows before it gives up.
const LIST_TRIES: usize = 4;

/// Every live PID on the system, in no particular order. PID 0, the
/// kernel, is left out.
///
/// # Errors
///
/// Returns [`ProcessError::Io`] when the list cannot be read.
pub(super) fn list_pids() -> Result<Vec<Pid>, ProcessError> {
    // SAFETY: a null buffer of size 0 only asks for the number of PIDs.
    let hint = unsafe { libc::proc_listallpids(ptr::null_mut(), 0) };
    if hint < 0 {
        return Err(io_error("proc_listallpids"));
    }
    let mut capacity = usize::try_from(hint).unwrap_or(0) + LIST_SLACK;
    for _ in 0..LIST_TRIES {
        let mut pids: Vec<libc::pid_t> = vec![0; capacity];
        let bytes = c_int::try_from(capacity * size_of::<libc::pid_t>()).unwrap_or(c_int::MAX);
        // SAFETY: the buffer holds `bytes` bytes of pid_t values.
        let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), bytes) };
        if filled < 0 {
            return Err(io_error("proc_listallpids"));
        }
        let filled = usize::try_from(filled).unwrap_or(0);
        if filled < capacity {
            pids.truncate(filled);
            return Ok(pids
                .into_iter()
                .filter_map(|pid| u32::try_from(pid).ok().and_then(|pid| Pid::new(pid).ok()))
                .collect());
        }
        // A full buffer may have cut the list short: grow and ask again.
        capacity *= 2;
    }
    Err(ProcessError::Io {
        op: "proc_listallpids",
        path: None,
        kind: io::ErrorKind::Other,
        message: "the process list kept growing past the buffer".to_owned(),
    })
}

/// The counters of `pid` while it still started at `expected`. The start
/// is read before and after the counters, so a PID reused in between is
/// refused rather than measured as the old process.
///
/// # Errors
///
/// Returns [`ProcessError::NotFound`] for no live process,
/// [`ProcessError::IdentityMismatch`] for another start time,
/// [`ProcessError::PermissionDenied`] for `EPERM`, and
/// [`ProcessError::Io`] otherwise.
pub(super) fn counters(pid: Pid, expected: ProcessStart) -> Result<ProcessCounters, ProcessError> {
    check_start(pid, expected)?;
    // SAFETY: rusage_info_v4 is plain integers, for which all zero bytes
    // are a valid value.
    let mut info: libc::rusage_info_v4 = unsafe { mem::zeroed() };
    // SAFETY: `info` is a writable rusage_info_v4, the struct the
    // RUSAGE_INFO_V4 flavor fills; the kernel writes no more than its size.
    let result = unsafe {
        libc::proc_pid_rusage(
            pid.as_i32(),
            libc::RUSAGE_INFO_V4,
            (&raw mut info).cast::<libc::rusage_info_t>(),
        )
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        return Err(match error.raw_os_error() {
            Some(libc::ESRCH) => ProcessError::NotFound { pid },
            Some(libc::EPERM) => ProcessError::PermissionDenied { pid },
            _ => ProcessError::io("proc_pid_rusage", None, &error),
        });
    }
    check_start(pid, expected)?;
    let timebase = timebase()?;
    Ok(ProcessCounters {
        cpu_user: timebase.duration(info.ri_user_time),
        cpu_system: timebase.duration(info.ri_system_time),
        footprint: info.ri_phys_footprint,
        disk_read: info.ri_diskio_bytesread,
        disk_write: info.ri_diskio_byteswritten,
    })
}

/// Fails unless a live process with `pid` started at `expected`.
fn check_start(pid: Pid, expected: ProcessStart) -> Result<(), ProcessError> {
    match bsd_info(pid)? {
        BsdInfo::Gone => Err(ProcessError::NotFound { pid }),
        BsdInfo::Denied => Err(ProcessError::PermissionDenied { pid }),
        BsdInfo::Info(info) if info.pbi_status == libc::SZOMB => {
            Err(ProcessError::NotFound { pid })
        }
        BsdInfo::Info(info) => {
            let start = start_time(info.pbi_start_tvsec, info.pbi_start_tvusec)?;
            if start == expected {
                Ok(())
            } else {
                Err(ProcessError::IdentityMismatch { pid })
            }
        }
    }
}

/// The ratio from mach time units to nanoseconds.
#[derive(Debug, Clone, Copy)]
struct Timebase {
    numer: u64,
    denom: u64,
}

impl Timebase {
    fn duration(self, ticks: u64) -> Duration {
        let nanos = u128::from(ticks) * u128::from(self.numer) / u128::from(self.denom);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

/// The timebase, read once per daemon: it is fixed for the life of the
/// boot.
fn timebase() -> Result<Timebase, ProcessError> {
    static TIMEBASE: OnceLock<Option<Timebase>> = OnceLock::new();
    let read = TIMEBASE.get_or_init(|| {
        let mut info = sys::mach_timebase_info::default();
        // SAFETY: `info` is a writable mach_timebase_info.
        let result = unsafe { sys::mach_timebase_info(&raw mut info) };
        (result == 0 && info.numer != 0 && info.denom != 0).then_some(Timebase {
            numer: u64::from(info.numer),
            denom: u64::from(info.denom),
        })
    });
    read.ok_or_else(|| ProcessError::Io {
        op: "mach_timebase_info",
        path: None,
        kind: io::ErrorKind::Other,
        message: "the kernel answered no timebase".to_owned(),
    })
}

fn io_error(op: &'static str) -> ProcessError {
    ProcessError::io(op, None, &io::Error::last_os_error())
}
