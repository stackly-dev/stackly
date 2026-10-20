//! Declarations the process adapter needs that libc 0.2.190 does not have
//! (spec 0010, *Deklarasi lokal*).
//!
//! Each one names the macOS SDK header it comes from. The structs are
//! written out whole, `#[repr(C)]`, and `const` asserts pin their size and
//! the offset of every field the adapter reads to the values of the macOS
//! 27 SDK, measured with `offsetof` in a C program against it. When the
//! workspace's libc gains these symbols, the declarations go and the
//! asserts stay.
#![allow(
    non_camel_case_types,
    dead_code,
    reason = "the SDK names and layouts need every field; the adapter reads a few"
)]

use std::mem::{offset_of, size_of};

use libc::{c_char, c_int, c_short, c_uchar, c_uint, c_ushort, c_void, gid_t, pid_t, uid_t};

/// `sys/spawn.h`: puts the child in a new session (macOS 10.15 and
/// later).
pub(crate) const POSIX_SPAWN_SETSID: c_int = 0x0400;
/// `sys/proc_info.h`: the `proc_pidfdinfo` flavor that answers a
/// [`vnode_fdinfowithpath`].
pub(crate) const PROC_PIDFDVNODEPATHINFO: c_int = 2;
/// `sys/proc_info.h`: the `proc_listpids` type that lists one process
/// group.
pub(crate) const PROC_PGRP_ONLY: u32 = 2;
/// `sys/fcntl.h`: no `SIGPIPE` for a write on this descriptor that finds
/// the pipe closed, just `EPIPE`.
pub(crate) const F_SETNOSIGPIPE: c_int = 73;

/// `sys/proc_info.h`: `struct proc_fileinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct proc_fileinfo {
    pub(crate) fi_openflags: u32,
    pub(crate) fi_status: u32,
    pub(crate) fi_offset: i64,
    pub(crate) fi_type: i32,
    pub(crate) fi_guardflags: u32,
}

/// `sys/proc_info.h`: `struct vnode_fdinfowithpath`, which
/// `PROC_PIDFDVNODEPATHINFO` fills.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct vnode_fdinfowithpath {
    pub(crate) pfi: proc_fileinfo,
    pub(crate) pvip: libc::vnode_info_path,
}

/// `sys/vm.h`: `struct vmspace`, opaque padding in `eproc`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct vmspace {
    pub(crate) dummy: i32,
    pub(crate) dummy2: *mut c_char,
    pub(crate) dummy3: [i32; 5],
    pub(crate) dummy4: [*mut c_char; 3],
}

/// `sys/sysctl.h`: `struct _pcred`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct _pcred {
    pub(crate) pc_lock: [c_char; 72],
    pub(crate) pc_ucred: *mut c_void,
    pub(crate) p_ruid: uid_t,
    pub(crate) p_svuid: uid_t,
    pub(crate) p_rgid: gid_t,
    pub(crate) p_svgid: gid_t,
    pub(crate) p_refcnt: c_int,
}

/// `sys/sysctl.h`: `struct _ucred`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct _ucred {
    pub(crate) cr_ref: i32,
    pub(crate) cr_uid: uid_t,
    pub(crate) cr_ngroups: c_short,
    pub(crate) cr_groups: [gid_t; 16],
}

/// `sys/proc.h`: `struct extern_proc`.
///
/// Its first member is a union of two run queue pointers and the start
/// time, both 16 bytes with 8 byte alignment; the adapter only reads the
/// start time, so the union is written as that member.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct extern_proc {
    pub(crate) p_starttime: libc::timeval,
    pub(crate) p_vmspace: *mut c_void,
    pub(crate) p_sigacts: *mut c_void,
    pub(crate) p_flag: c_int,
    pub(crate) p_stat: c_char,
    pub(crate) p_pid: pid_t,
    pub(crate) p_oppid: pid_t,
    pub(crate) p_dupfd: c_int,
    pub(crate) user_stack: *mut c_char,
    pub(crate) exit_thread: *mut c_void,
    pub(crate) p_debugger: c_int,
    pub(crate) sigwait: c_int,
    pub(crate) p_estcpu: c_uint,
    pub(crate) p_cpticks: c_int,
    pub(crate) p_pctcpu: u32,
    pub(crate) p_wchan: *mut c_void,
    pub(crate) p_wmesg: *mut c_char,
    pub(crate) p_swtime: c_uint,
    pub(crate) p_slptime: c_uint,
    pub(crate) p_realtimer: libc::itimerval,
    pub(crate) p_rtime: libc::timeval,
    pub(crate) p_uticks: u64,
    pub(crate) p_sticks: u64,
    pub(crate) p_iticks: u64,
    pub(crate) p_traceflag: c_int,
    pub(crate) p_tracep: *mut c_void,
    pub(crate) p_siglist: c_int,
    pub(crate) p_textvp: *mut c_void,
    pub(crate) p_holdcnt: c_int,
    pub(crate) p_sigmask: libc::sigset_t,
    pub(crate) p_sigignore: libc::sigset_t,
    pub(crate) p_sigcatch: libc::sigset_t,
    pub(crate) p_priority: c_uchar,
    pub(crate) p_usrpri: c_uchar,
    pub(crate) p_nice: c_char,
    pub(crate) p_comm: [c_char; 17],
    pub(crate) p_pgrp: *mut c_void,
    pub(crate) p_addr: *mut c_void,
    pub(crate) p_xstat: c_ushort,
    pub(crate) p_acflag: c_ushort,
    pub(crate) p_ru: *mut c_void,
}

/// `sys/sysctl.h`: `struct eproc`, the second half of `kinfo_proc`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct eproc {
    pub(crate) e_paddr: *mut c_void,
    pub(crate) e_sess: *mut c_void,
    pub(crate) e_pcred: _pcred,
    pub(crate) e_ucred: _ucred,
    pub(crate) e_vm: vmspace,
    pub(crate) e_ppid: pid_t,
    pub(crate) e_pgid: pid_t,
    pub(crate) e_jobc: c_short,
    pub(crate) e_tdev: libc::dev_t,
    pub(crate) e_tpgid: pid_t,
    pub(crate) e_tsess: *mut c_void,
    pub(crate) e_wmesg: [c_char; 8],
    pub(crate) e_xsize: i32,
    pub(crate) e_xrssize: c_short,
    pub(crate) e_xccount: c_short,
    pub(crate) e_xswrss: c_short,
    pub(crate) e_flag: i32,
    pub(crate) e_login: [c_char; 12],
    pub(crate) e_spare: [i32; 4],
}

/// `sys/sysctl.h`: `struct kinfo_proc`, which
/// `sysctl {CTL_KERN, KERN_PROC, KERN_PROC_PID, pid}` fills.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct kinfo_proc {
    pub(crate) kp_proc: extern_proc,
    pub(crate) kp_eproc: eproc,
}

// The macOS 27 SDK layout, measured with offsetof (spec 0010, *Deklarasi
// lokal*). A mismatch fails the build instead of reading the wrong bytes.
const _: () = {
    assert!(size_of::<proc_fileinfo>() == 24);
    assert!(size_of::<vnode_fdinfowithpath>() == 1200);
    assert!(offset_of!(vnode_fdinfowithpath, pvip) == 24);
    assert!(size_of::<libc::vnode_info_path>() == 1176);
    assert!(size_of::<libc::proc_bsdinfo>() == 136);

    assert!(size_of::<kinfo_proc>() == 648);
    assert!(size_of::<extern_proc>() == 296);
    assert!(size_of::<eproc>() == 352);
    assert!(size_of::<_pcred>() == 104);
    assert!(size_of::<_ucred>() == 76);
    assert!(size_of::<vmspace>() == 64);
    assert!(size_of::<libc::timeval>() == 16);
    assert!(offset_of!(libc::timeval, tv_usec) == 8);

    assert!(offset_of!(kinfo_proc, kp_proc.p_starttime) == 0);
    assert!(offset_of!(kinfo_proc, kp_proc.p_stat) == 36);
    assert!(offset_of!(kinfo_proc, kp_proc.p_siglist) == 208);
    assert!(offset_of!(kinfo_proc, kp_proc.p_sigmask) == 228);
    assert!(offset_of!(kinfo_proc, kp_proc.p_sigignore) == 232);
    assert!(offset_of!(kinfo_proc, kp_proc.p_sigcatch) == 236);
    assert!(offset_of!(kinfo_proc, kp_eproc) == 296);
    assert!(offset_of!(kinfo_proc, kp_eproc.e_pcred.p_ruid) == 392);
    assert!(offset_of!(kinfo_proc, kp_eproc.e_pcred.p_svuid) == 396);
    assert!(offset_of!(kinfo_proc, kp_eproc.e_ucred.cr_uid) == 420);
    assert!(offset_of!(kinfo_proc, kp_eproc.e_ppid) == 560);
    assert!(offset_of!(kinfo_proc, kp_eproc.e_pgid) == 564);
};
