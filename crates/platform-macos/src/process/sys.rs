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
/// `sys/proc_info.h`: the `proc_listpids` type that lists the processes of
/// one effective user.
pub(crate) const PROC_UID_ONLY: u32 = 4;
/// `sys/proc_info.h`: the `proc_pidfdinfo` flavor that answers a
/// [`socket_fdinfo`].
pub(crate) const PROC_PIDFDSOCKETINFO: c_int = 3;
/// `sys/proc_info.h`: `soi_kind` of an IP socket other than TCP, whose
/// `soi_proto` is `pri_in`.
pub(crate) const SOCKINFO_IN: c_int = 1;
/// `sys/proc_info.h`: `soi_kind` of a TCP socket, whose `soi_proto` is
/// `pri_tcp`.
pub(crate) const SOCKINFO_TCP: c_int = 2;
/// `sys/proc_info.h`: `tcpsi_state` of a socket listening for connections.
pub(crate) const TSI_S_LISTEN: c_int = 1;
/// `sys/proc_info.h`: `insi_vflag` bit of a socket with an IPv4 address.
pub(crate) const INI_IPV4: u8 = 0x1;
/// `sys/proc_info.h`: `insi_vflag` bit of a socket with an IPv6 address.
pub(crate) const INI_IPV6: u8 = 0x2;

/// `mach/mach_time.h`: `struct mach_timebase_info`, the ratio that turns
/// mach time units into nanoseconds. libc declares it only as deprecated.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct mach_timebase_info {
    pub(crate) numer: u32,
    pub(crate) denom: u32,
}

unsafe extern "C" {
    /// `mach/mach_time.h`: fills the timebase ratio; answers 0
    /// (`KERN_SUCCESS`) on success.
    pub(crate) fn mach_timebase_info(info: *mut mach_timebase_info) -> c_int;
}

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

/// `sys/proc_info.h`: `struct in4in6_addr`, an IPv4 address in the last
/// four bytes of the 16 an IPv6 address takes.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct in4in6_addr {
    pub(crate) i46a_pad32: [u32; 3],
    pub(crate) i46a_addr4: libc::in_addr,
}

/// `sys/proc_info.h`: the anonymous union of `insi_faddr` and
/// `insi_laddr`: the IPv4 form or the IPv6 form of one address.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union in_sockinfo_addr {
    pub(crate) ina_46: in4in6_addr,
    pub(crate) ina_6: libc::in6_addr,
}

/// `sys/proc_info.h`: the anonymous struct `insi_v4`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct in_sockinfo_v4 {
    pub(crate) in4_tos: c_uchar,
}

/// `sys/proc_info.h`: the anonymous struct `insi_v6`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct in_sockinfo_v6 {
    pub(crate) in6_hlim: u8,
    pub(crate) in6_cksum: c_int,
    pub(crate) in6_ifindex: c_ushort,
    pub(crate) in6_hops: c_short,
}

/// `sys/proc_info.h`: `struct in_sockinfo`, an IP socket. `insi_lport`
/// holds the local port in network byte order.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct in_sockinfo {
    pub(crate) insi_fport: c_int,
    pub(crate) insi_lport: c_int,
    pub(crate) insi_gencnt: u64,
    pub(crate) insi_flags: u32,
    pub(crate) insi_flow: u32,
    pub(crate) insi_vflag: u8,
    pub(crate) insi_ip_ttl: u8,
    pub(crate) rfu_1: u32,
    pub(crate) insi_faddr: in_sockinfo_addr,
    pub(crate) insi_laddr: in_sockinfo_addr,
    pub(crate) insi_v4: in_sockinfo_v4,
    pub(crate) insi_v6: in_sockinfo_v6,
}

/// `sys/proc_info.h`: `struct tcp_sockinfo`, a TCP socket.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct tcp_sockinfo {
    pub(crate) tcpsi_ini: in_sockinfo,
    pub(crate) tcpsi_state: c_int,
    pub(crate) tcpsi_timer: [c_int; 4],
    pub(crate) tcpsi_mss: c_int,
    pub(crate) tcpsi_flags: u32,
    pub(crate) rfu_1: u32,
    pub(crate) tcpsi_tp: u64,
}

/// `sys/proc_info.h`: the anonymous union `soi_proto`. Only the IP members
/// are written out; `soi_size` gives it the size of its largest member,
/// `struct un_sockinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union soi_proto {
    pub(crate) pri_in: in_sockinfo,
    pub(crate) pri_tcp: tcp_sockinfo,
    pub(crate) soi_size: [u64; 66],
}

/// `sys/proc_info.h`: `struct sockbuf_info`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockbuf_info {
    pub(crate) sbi_cc: u32,
    pub(crate) sbi_hiwat: u32,
    pub(crate) sbi_mbcnt: u32,
    pub(crate) sbi_mbmax: u32,
    pub(crate) sbi_lowat: u32,
    pub(crate) sbi_flags: c_short,
    pub(crate) sbi_timeo: c_short,
}

/// `sys/proc_info.h`: `struct socket_info`. `soi_kind` names the member of
/// `soi_proto` the kernel filled.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct socket_info {
    pub(crate) soi_stat: libc::vinfo_stat,
    pub(crate) soi_so: u64,
    pub(crate) soi_pcb: u64,
    pub(crate) soi_type: c_int,
    pub(crate) soi_protocol: c_int,
    pub(crate) soi_family: c_int,
    pub(crate) soi_options: c_short,
    pub(crate) soi_linger: c_short,
    pub(crate) soi_state: c_short,
    pub(crate) soi_qlen: c_short,
    pub(crate) soi_incqlen: c_short,
    pub(crate) soi_qlimit: c_short,
    pub(crate) soi_timeo: c_short,
    pub(crate) soi_error: c_ushort,
    pub(crate) soi_oobmark: u32,
    pub(crate) soi_rcv: sockbuf_info,
    pub(crate) soi_snd: sockbuf_info,
    pub(crate) soi_kind: c_int,
    pub(crate) rfu_1: u32,
    pub(crate) soi_proto: soi_proto,
}

/// `sys/proc_info.h`: `struct socket_fdinfo`, which
/// `PROC_PIDFDSOCKETINFO` fills.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct socket_fdinfo {
    pub(crate) pfi: proc_fileinfo,
    pub(crate) psi: socket_info,
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
    assert!(size_of::<mach_timebase_info>() == 8);
    assert!(size_of::<libc::rusage_info_v4>() == 296);
    assert!(offset_of!(libc::rusage_info_v4, ri_user_time) == 16);
    assert!(offset_of!(libc::rusage_info_v4, ri_system_time) == 24);
    assert!(offset_of!(libc::rusage_info_v4, ri_phys_footprint) == 72);
    assert!(offset_of!(libc::rusage_info_v4, ri_diskio_bytesread) == 144);
    assert!(offset_of!(libc::rusage_info_v4, ri_diskio_byteswritten) == 152);
    assert!(size_of::<vnode_fdinfowithpath>() == 1200);
    assert!(offset_of!(vnode_fdinfowithpath, pvip) == 24);
    assert!(size_of::<libc::vnode_info_path>() == 1176);
    assert!(size_of::<libc::proc_bsdinfo>() == 136);

    assert!(size_of::<socket_fdinfo>() == 792);
    assert!(offset_of!(socket_fdinfo, psi) == 24);
    assert!(size_of::<socket_info>() == 768);
    assert!(size_of::<libc::vinfo_stat>() == 136);
    assert!(size_of::<sockbuf_info>() == 24);
    assert!(offset_of!(socket_info, soi_so) == 136);
    assert!(offset_of!(socket_info, soi_type) == 152);
    assert!(offset_of!(socket_info, soi_protocol) == 156);
    assert!(offset_of!(socket_info, soi_family) == 160);
    assert!(offset_of!(socket_info, soi_options) == 164);
    assert!(offset_of!(socket_info, soi_oobmark) == 180);
    assert!(offset_of!(socket_info, soi_rcv) == 184);
    assert!(offset_of!(socket_info, soi_snd) == 208);
    assert!(offset_of!(socket_info, soi_kind) == 232);
    assert!(offset_of!(socket_info, soi_proto) == 240);
    assert!(size_of::<soi_proto>() == 528);
    assert!(size_of::<in_sockinfo>() == 80);
    assert!(offset_of!(in_sockinfo, insi_lport) == 4);
    assert!(offset_of!(in_sockinfo, insi_gencnt) == 8);
    assert!(offset_of!(in_sockinfo, insi_flags) == 16);
    assert!(offset_of!(in_sockinfo, insi_flow) == 20);
    assert!(offset_of!(in_sockinfo, insi_vflag) == 24);
    assert!(offset_of!(in_sockinfo, insi_ip_ttl) == 25);
    assert!(offset_of!(in_sockinfo, rfu_1) == 28);
    assert!(offset_of!(in_sockinfo, insi_faddr) == 32);
    assert!(offset_of!(in_sockinfo, insi_laddr) == 48);
    assert!(offset_of!(in_sockinfo, insi_v4) == 64);
    assert!(offset_of!(in_sockinfo, insi_v6) == 68);
    assert!(size_of::<in4in6_addr>() == 16);
    assert!(offset_of!(in4in6_addr, i46a_addr4) == 12);
    assert!(size_of::<tcp_sockinfo>() == 120);
    assert!(offset_of!(tcp_sockinfo, tcpsi_state) == 80);
    assert!(offset_of!(tcp_sockinfo, tcpsi_timer) == 84);
    assert!(offset_of!(tcp_sockinfo, tcpsi_mss) == 100);
    assert!(offset_of!(tcp_sockinfo, tcpsi_tp) == 112);

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
