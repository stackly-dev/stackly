//! The login session adapter (spec 0031, AC-1): the boot UUID from
//! `kern.bootsessionuuid` and the audit session ID of the login from
//! `getaudit_addr`.

use std::mem;

use stackly_domain::BootId;
use stackly_platform::{LoginSession, LoginSessionError, LoginSessionInfo};

use crate::process::boot::boot_id;

/// `struct au_mask` of `<bsm/audit.h>`.
#[repr(C)]
struct AuMask {
    success: u32,
    failure: u32,
}

/// `struct au_tid_addr` of `<bsm/audit.h>`.
#[repr(C)]
struct AuTidAddr {
    port: i32,
    kind: u32,
    addr: [u32; 4],
}

/// `struct auditinfo_addr` of `<bsm/audit.h>`.
#[repr(C)]
struct AuditInfoAddr {
    auid: u32,
    mask: AuMask,
    termid: AuTidAddr,
    asid: i32,
    flags: u64,
}

unsafe extern "C" {
    fn getaudit_addr(info: *mut AuditInfoAddr, length: libc::c_int) -> libc::c_int;
}

/// Reads the login session of this process from the kernel.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacosLoginSessionInfo;

impl MacosLoginSessionInfo {
    /// Creates the adapter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl LoginSessionInfo for MacosLoginSessionInfo {
    fn current(&self) -> Result<LoginSession, LoginSessionError> {
        let boot = read_boot()?;
        let audit = read_audit_session()?;
        LoginSession::new(boot, audit)
    }
}

fn read_boot() -> Result<BootId, LoginSessionError> {
    boot_id().map_err(|_| LoginSessionError::Unavailable {
        reason: "the boot session UUID cannot be read",
    })
}

/// Returns the audit session ID of this process, which is positive for a
/// login session.
fn read_audit_session() -> Result<u32, LoginSessionError> {
    let mut info = AuditInfoAddr {
        auid: 0,
        mask: AuMask {
            success: 0,
            failure: 0,
        },
        termid: AuTidAddr {
            port: 0,
            kind: 0,
            addr: [0; 4],
        },
        asid: 0,
        flags: 0,
    };
    let length = libc::c_int::try_from(mem::size_of::<AuditInfoAddr>()).map_err(|_| {
        LoginSessionError::Unavailable {
            reason: "the audit record size does not fit",
        }
    })?;
    // SAFETY: `info` is a writable `repr(C)` copy of `struct auditinfo_addr`
    // and `length` is its exact size, so the kernel writes inside it.
    let result = unsafe { getaudit_addr(&raw mut info, length) };
    if result != 0 {
        return Err(LoginSessionError::Unavailable {
            reason: "getaudit_addr failed",
        });
    }
    u32::try_from(info.asid).map_err(|_| LoginSessionError::Unavailable {
        reason: "the audit session is not assigned",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_reports_a_session_when_it_has_one() {
        // A test run from a launchd or a login context has an audit session;
        // one run in a bare sandbox may not, which is the documented error.
        match MacosLoginSessionInfo::new().current() {
            Ok(session) => {
                assert!(session.audit_session >= 1);
                assert!(session.id.as_str().len() <= 47);
                assert!(
                    session
                        .id
                        .as_str()
                        .starts_with(&session.boot_id.to_string())
                );
            }
            Err(error) => assert!(matches!(error, LoginSessionError::Unavailable { .. })),
        }
    }
}
