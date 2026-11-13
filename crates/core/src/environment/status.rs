//! The status of a registered environment (spec 0044, *Status
//! environment* and *State manifest*): computed from the registry snapshot
//! and the instances, never from the user's file, except the manifest
//! state, which only `env.get` with `checkManifest` asks for.

use stackly_domain::{
    Environment, InstanceLifecycle, InstanceName, MemberAlias, Ownership, ProviderName,
    ReadinessState, RuntimeSelector,
};

use super::plan::{DetachedMember, InstanceView};
use super::validate::validate;
use crate::catalog::Catalog;

/// How many of an environment's services are ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EnvironmentStatus {
    /// Every service is ready, and there is at least one.
    Ready,
    /// Some services are ready.
    PartiallyReady,
    /// No service is ready.
    Down,
}

impl EnvironmentStatus {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::PartiallyReady => "partially_ready",
            Self::Down => "down",
        }
    }
}

/// Why a service is not ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotReadyReason {
    /// No instance exists for it yet.
    NotCreated,
    /// The instance is stopped.
    Stopped,
    /// The instance is starting.
    Starting,
    /// The instance is stopping.
    Stopping,
    /// The instance failed.
    Failed,
    /// It runs but is not ready.
    NotReady,
    /// Its state is unknown or it is retained.
    Unavailable,
}

impl NotReadyReason {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::NotCreated => "not_created",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
            Self::NotReady => "not_ready",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One service of the snapshot and its instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    /// The alias.
    pub alias: MemberAlias,
    /// The provider.
    pub provider: ProviderName,
    /// Its ownership.
    pub ownership: Ownership,
    /// The selector the manifest wrote, if any.
    pub selector: Option<RuntimeSelector>,
    /// The instance a shared service borrows.
    pub instance_ref: Option<InstanceName>,
    /// Its instance, when it exists.
    pub instance: Option<InstanceView>,
    /// Why it is not ready, with a redacted detail; none when ready.
    pub not_ready: Option<(NotReadyReason, Option<String>)>,
}

/// An environment's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentView {
    /// The status.
    pub status: EnvironmentStatus,
    /// The services whose instance runs and is ready.
    pub ready_count: usize,
    /// The services of the snapshot.
    pub service_count: usize,
    /// One per service of the snapshot, in snapshot order.
    pub services: Vec<ServiceStatus>,
    /// Members the snapshot no longer declares.
    pub detached: Vec<DetachedMember>,
}

/// Why an instance is not ready, by the *Lifecycle ke aksi* table; none
/// when it runs and is ready.
#[must_use]
pub fn not_ready(view: &InstanceView) -> Option<(NotReadyReason, Option<String>)> {
    use InstanceLifecycle as L;
    let reason = match view.lifecycle {
        L::Running if view.readiness == ReadinessState::Ready => return None,
        L::Running => NotReadyReason::NotReady,
        L::Starting | L::Initializing | L::Recovering => NotReadyReason::Starting,
        L::Stopped | L::Uninitialized => NotReadyReason::Stopped,
        L::Failed => NotReadyReason::Failed,
        L::Stopping => NotReadyReason::Stopping,
        L::Unknown | L::Retained => NotReadyReason::Unavailable,
    };
    Some((reason, view.detail.clone()))
}

/// The status of `environment` over `instances`, every instance with the
/// retained ones: a retained member is `unavailable`, never `not_created`.
#[must_use]
pub fn environment_view(environment: &Environment, instances: &[InstanceView]) -> EnvironmentView {
    let by_id = |id| instances.iter().find(|view| view.id == id);
    let services: Vec<ServiceStatus> = environment
        .declared()
        .iter()
        .map(|declared| {
            let member = environment
                .members()
                .iter()
                .find(|member| member.alias() == declared.alias());
            let instance = match (member, declared.instance_ref()) {
                (Some(member), _) => by_id(member.instance_id()),
                // A retained instance gives its name up.
                (None, Some(name)) => instances.iter().find(|view| {
                    &view.name == name && view.lifecycle != InstanceLifecycle::Retained
                }),
                (None, None) => None,
            };
            let not_ready = match instance {
                None => Some((NotReadyReason::NotCreated, None)),
                Some(view) => not_ready(view),
            };
            ServiceStatus {
                alias: declared.alias().clone(),
                provider: declared.provider().clone(),
                ownership: declared.ownership(),
                selector: declared.selector().cloned(),
                instance_ref: declared.instance_ref().cloned(),
                instance: instance.cloned(),
                not_ready,
            }
        })
        .collect();
    let service_count = services.len();
    let ready_count = services.iter().filter(|s| s.not_ready.is_none()).count();
    let status = if service_count > 0 && ready_count == service_count {
        EnvironmentStatus::Ready
    } else if ready_count == 0 {
        EnvironmentStatus::Down
    } else {
        EnvironmentStatus::PartiallyReady
    };
    let detached = environment
        .members()
        .iter()
        .filter(|member| {
            !environment
                .declared()
                .iter()
                .any(|declared| declared.alias() == member.alias())
        })
        .map(|member| DetachedMember {
            alias: member.alias().clone(),
            instance_id: member.instance_id(),
            instance_name: by_id(member.instance_id()).map(|view| view.name.clone()),
            ownership: member.ownership(),
        })
        .collect();
    EnvironmentView {
        status,
        ready_count,
        service_count,
        services,
        detached,
    }
}

/// How the file on disk compares to the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ManifestState {
    /// The file matches the snapshot.
    InSync,
    /// The file changed since import.
    Changed,
    /// No location, or the file is gone.
    Missing,
    /// The file exists and can't be read.
    Unreadable,
    /// The file has error findings.
    Invalid,
    /// The file declares another name.
    NameChanged,
}

impl ManifestState {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::InSync => "in_sync",
            Self::Changed => "changed",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::Invalid => "invalid",
            Self::NameChanged => "name_changed",
        }
    }
}

/// What reading the manifest file gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestRead {
    /// No location, or no file there.
    Missing,
    /// The file exists but the limited read failed.
    Unreadable,
    /// The bytes.
    Bytes(Vec<u8>),
}

/// *State manifest*: the first state that matches, with the number of
/// error findings for `invalid`. Without a catalog, validation is skipped.
#[must_use]
pub fn manifest_state(
    environment: &Environment,
    read: &ManifestRead,
    catalog: Option<&Catalog>,
) -> (ManifestState, usize) {
    let bytes = match read {
        ManifestRead::Missing => return (ManifestState::Missing, 0),
        ManifestRead::Unreadable => return (ManifestState::Unreadable, 0),
        ManifestRead::Bytes(bytes) => bytes,
    };
    let validation = catalog.map(|catalog| validate(bytes, catalog));
    if let Some(validation) = &validation
        && validation.findings.has_errors()
    {
        return (ManifestState::Invalid, validation.findings.count());
    }
    let name = validation
        .as_ref()
        .and_then(|validation| validation.name.clone());
    if let Some(name) = name
        && &name != environment.name()
    {
        return (ManifestState::NameChanged, 0);
    }
    let digest = super::validate::manifest_digest(bytes);
    if environment.manifest_sha256() != Some(&digest) {
        return (ManifestState::Changed, 0);
    }
    (ManifestState::InSync, 0)
}
