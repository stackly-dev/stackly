//! Maps the detail data of `stackly-core` to the DTOs of
//! `instance.get_detail` and `instance.get_data_usage` (spec 0021): the
//! facts of `detail::build` to `InstanceDetail`, the rendered connections
//! to `ConnectionString`, and a measured size to `DataUsage`. A value is
//! copied as it is; nothing is computed again here, and no secret value
//! exists on this side.

use stackly_core::instance::connection::{
    ConnectionPart as CorePart, ConnectionState as CoreState, ConnectionUnavailable,
    RenderedConnection,
};
use stackly_core::instance::detail::InstanceDetailData;
use stackly_domain::{Ownership, StartupPolicy};
use stackly_protocol::codes::instance;
use stackly_protocol::instance::connection::ConnectionPart;
use stackly_protocol::instance::{
    ConnectionAvailability, ConnectionString, ConnectionUnavailableReason, DataUsage,
    DependentOwnership, DetailDependent, DetailProvider, DetailRuntime, DetailSecret,
    InstanceDetail, StartupPolicyKind,
};
use stackly_protocol::runtime::InstallModeKind;
use stackly_protocol::secret::SecretStatus;
use stackly_protocol::{Count, ErrorBody, Timestamp};

use super::runtime::target;
use crate::detail::{MeasuredUsage, UsageFailure};
use crate::ipc::operation::dto::revision;

/// The wire detail of `data`.
#[must_use]
pub fn instance_detail(data: &InstanceDetailData) -> InstanceDetail {
    InstanceDetail {
        instance_id: data.instance_id.as_uuid(),
        revision: revision(data.revision),
        provider: DetailProvider {
            name: data.provider.key.provider().to_string(),
            revision: data.provider.key.revision().to_string(),
            manifest_sha256: data.provider.key.manifest_sha256().to_string(),
            display_name: data.provider.display_name.clone(),
            package_readable: data.provider.package_readable,
        },
        runtime: DetailRuntime {
            id: data.runtime.id.as_uuid(),
            version: data.runtime.version.clone(),
            target: target(data.runtime.target),
            install_mode: if data.runtime.managed {
                InstallModeKind::Managed
            } else {
                InstallModeKind::External
            },
            artifact_sha256: data
                .runtime
                .artifact_sha256
                .map(|digest| digest.to_string()),
        },
        data_path: data.data_path.to_string_lossy().into_owned(),
        startup: match data.startup {
            StartupPolicy::Manual => StartupPolicyKind::Manual,
            StartupPolicy::AtLogin => StartupPolicyKind::AtLogin,
        },
        secrets: data
            .secrets
            .iter()
            .map(|secret| DetailSecret {
                field: secret.field.as_str().to_owned(),
                title: secret.title.clone(),
                reference: secret.reference.to_string(),
                status: match secret.status {
                    stackly_domain::SecretStatus::Present => SecretStatus::Present,
                    stackly_domain::SecretStatus::Missing => SecretStatus::Missing,
                    stackly_domain::SecretStatus::Unknown => SecretStatus::Unknown,
                },
            })
            .collect(),
        dependents: data
            .dependents
            .iter()
            .map(|dependent| DetailDependent {
                environment_id: dependent.environment_id.as_uuid(),
                name: dependent.name.to_string(),
                ownership: match dependent.ownership {
                    Ownership::Dedicated => DependentOwnership::Dedicated,
                    Ownership::Shared => DependentOwnership::Shared,
                },
                lease_active: dependent.lease_active,
            })
            .collect(),
        connections: data.connections.iter().map(connection).collect(),
    }
}

/// The wire form of one rendered connection entry.
fn connection(rendered: &RenderedConnection) -> ConnectionString {
    let (state, reason, masked, parts) = match &rendered.state {
        CoreState::Available { masked, parts } => (
            ConnectionAvailability::Available,
            None,
            Some(masked.clone()),
            parts.iter().map(part).collect(),
        ),
        CoreState::Unavailable(why) => (
            ConnectionAvailability::Unavailable,
            Some(match why {
                ConnectionUnavailable::Retained => ConnectionUnavailableReason::Retained,
                ConnectionUnavailable::PackageUnavailable => {
                    ConnectionUnavailableReason::PackageUnavailable
                }
                ConnectionUnavailable::PortNotAssigned => {
                    ConnectionUnavailableReason::PortNotAssigned
                }
                _ => ConnectionUnavailableReason::Unrecognized,
            }),
            None,
            Vec::new(),
        ),
        _ => (ConnectionAvailability::Unrecognized, None, None, Vec::new()),
    };
    ConnectionString {
        name: rendered.name.clone(),
        title: rendered.title.clone(),
        state,
        reason,
        masked,
        parts,
        port_may_change: rendered.port_may_change,
    }
}

fn part(part: &CorePart) -> ConnectionPart {
    match part {
        CorePart::Text(text) => ConnectionPart::text(text.clone()),
        CorePart::Secret { field, reference } => {
            ConnectionPart::secret(field.as_str(), reference.to_string())
        }
        // A part kind this build cannot name has no text and no secret,
        // so a client reads it as unavailable (spec 0021, *Method IPC*).
        _ => ConnectionPart {
            kind: stackly_protocol::instance::connection::ConnectionPartKind::Unrecognized,
            text: None,
            secret: None,
        },
    }
}

/// The wire size of a measured folder.
#[must_use]
pub fn data_usage(usage: &MeasuredUsage) -> DataUsage {
    DataUsage {
        bytes: Count::saturating(usage.bytes),
        complete: usage.complete,
        scanned_at: Timestamp::from_jiff(usage.scanned_at),
    }
}

/// `instance.data_usage_unavailable` with the hint of its reason.
#[must_use]
pub fn usage_failure(failure: UsageFailure) -> ErrorBody {
    let body = ErrorBody::new(
        &instance::DATA_USAGE_UNAVAILABLE,
        "the data folder size can't be measured",
    )
    .with_detail("reason", failure.reason());
    match instance::hint(body.code(), body.details()) {
        Some(hint) => body.with_hint(hint),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_gives_the_code_its_reason_and_its_hint() {
        for (failure, reason, hint) in [
            (
                UsageFailure::Missing,
                "missing",
                "run stackly doctor; the data folder is missing",
            ),
            (UsageFailure::Io, "io", "try again"),
        ] {
            let body = usage_failure(failure);

            assert_eq!(body.code(), "instance.data_usage_unavailable");
            assert_eq!(body.hint(), Some(hint));
            assert_eq!(
                body.details()
                    .iter()
                    .find(|d| d.key() == "reason")
                    .map(|d| d.value()),
                Some(reason)
            );
        }
        assert!(usage_failure(UsageFailure::NotPrivate).hint().is_some());
    }
}
