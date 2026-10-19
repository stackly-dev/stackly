//! Maps platform and domain values to the DTOs of `stackly-protocol`.
//! Boundaries carry DTOs only, so this is the one place that knows both.

use std::path::Path;

use stackly_platform::{
    CapabilityStatus, DirectoryLayout, PlatformCapability, PlatformInfo, UserId,
};
use stackly_protocol::daemon::{Capabilities, DaemonInfo, DaemonRoots, PlatformCapabilityStatus};
use stackly_protocol::events::EventKind;
use stackly_protocol::{CONTRACT_DIGEST, Call, ProtocolVersion, Timestamp};
use uuid::Uuid;

/// What the daemon decided at start, before it serves anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct StartFacts {
    /// The server instance ID of this run.
    pub server_instance_id: Uuid,
    /// When the daemon started.
    pub started_at: Timestamp,
    /// Whether `STACKLY_ROOT` was set.
    pub root_override: bool,
    /// The daemon's process ID.
    pub pid: u32,
}

impl StartFacts {
    /// Bundles the facts.
    #[must_use]
    pub const fn new(
        server_instance_id: Uuid,
        started_at: Timestamp,
        root_override: bool,
        pid: u32,
    ) -> Self {
        Self {
            server_instance_id,
            started_at,
            root_override,
            pid,
        }
    }
}

/// A path as wire text: the resolved (not canonical) form, converted
/// lossily; Stackly never creates paths that are not UTF-8.
#[must_use]
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The uid of a Unix user, or `None` for another kind of account.
#[must_use]
pub const fn unix_uid(user: &UserId) -> Option<u32> {
    match user {
        UserId::Unix(uid) => Some(*uid),
        _ => None,
    }
}

/// The reply of `daemon.get_info`.
#[must_use]
pub fn daemon_info(
    facts: &StartFacts,
    layout: &DirectoryLayout,
    uid: Option<u32>,
    version: &str,
) -> DaemonInfo {
    let roots = layout.roots();
    DaemonInfo {
        version: version.to_owned(),
        protocol: ProtocolVersion::CURRENT,
        contract_digest: CONTRACT_DIGEST.to_owned(),
        server_instance_id: facts.server_instance_id,
        pid: facts.pid,
        uid,
        started_at: facts.started_at,
        roots: DaemonRoots {
            data: path_text(roots.data()),
            logs: path_text(roots.logs()),
            cache: path_text(roots.cache()),
            runtime: path_text(roots.runtime()),
            root_override: facts.root_override,
        },
        socket_path: path_text(&layout.daemon_socket()),
    }
}

/// The reply of `daemon.get_capabilities`: every platform capability with
/// its code, the methods this daemon knows, and the event kinds it sends.
#[must_use]
pub fn capabilities(info: &dyn PlatformInfo) -> Capabilities {
    let statuses = info.capabilities();
    Capabilities {
        platform: PlatformCapability::ALL
            .iter()
            .map(|capability| {
                let (supported, reason) = match statuses.status(*capability) {
                    CapabilityStatus::Supported => (true, None),
                    CapabilityStatus::NotSupported { reason } => {
                        (false, (!reason.is_empty()).then(|| reason.to_owned()))
                    }
                };
                PlatformCapabilityStatus {
                    capability: capability.as_code().to_owned(),
                    supported,
                    reason,
                }
            })
            .collect(),
        methods: Call::METHODS
            .iter()
            .map(|method| (*method).to_owned())
            .collect(),
        event_kinds: EventKind::KINDS
            .iter()
            .map(|kind| (*kind).to_owned())
            .collect(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use stackly_testkit::{FakePlatform, TestRoot};

    use super::*;

    #[test]
    fn capabilities_name_every_platform_capability_and_every_method_and_event_kind() {
        let root = TestRoot::new().unwrap();
        let platform = FakePlatform::new(&root).platform();

        let capabilities = capabilities(platform.info());

        let codes: Vec<_> = capabilities
            .platform
            .iter()
            .map(|status| status.capability.as_str())
            .collect();
        let expected: Vec<_> = PlatformCapability::ALL
            .iter()
            .map(|capability| capability.as_code())
            .collect();
        assert_eq!(codes, expected);
        assert_eq!(capabilities.methods, Call::METHODS);
        assert_eq!(capabilities.event_kinds, EventKind::KINDS);
        for status in &capabilities.platform {
            assert_eq!(status.supported, status.reason.is_none(), "{status:?}");
        }
    }

    #[test]
    fn daemon_info_reports_the_resolved_roots_and_the_socket() {
        let root = TestRoot::new().unwrap();
        let layout = DirectoryLayout::new(root.roots());
        let facts = StartFacts::new(Uuid::now_v7(), Timestamp::UNIX_EPOCH, true, 4242);

        let info = daemon_info(&facts, &layout, Some(501), "0.1.0");

        assert_eq!(info.roots.data, path_text(root.roots().data()));
        assert_eq!(info.roots.runtime, path_text(root.roots().runtime()));
        assert_eq!(info.socket_path, path_text(&layout.daemon_socket()));
        assert!(info.roots.root_override);
        assert_eq!((info.pid, info.uid), (4242, Some(501)));
        assert_eq!(info.contract_digest, CONTRACT_DIGEST);
        assert_eq!(info.protocol, ProtocolVersion::CURRENT);
    }
}
