//! Maps platform and domain values to the DTOs of `stackly-protocol`.
//! Boundaries carry DTOs only, so this is the one place that knows both.

use std::path::Path;

use stackly_domain::{RegistryError, ResourceRef as DomainResource};
use stackly_platform::{
    CapabilityStatus, DirectoryLayout, PlatformCapability, PlatformInfo, UserId,
};
use stackly_protocol::codes::{internal, registry, resource};
use stackly_protocol::daemon::{
    Capabilities, DaemonInfo, DaemonRoots, PlatformCapabilityStatus, RegistryRecovery,
};
use stackly_protocol::events::EventKind;
use stackly_protocol::{
    CONTRACT_DIGEST, Call, ErrorBody, ProtocolVersion, ResourceKind, ResourceRef, Timestamp,
};
use stackly_storage::RecoveryReport;
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
        registry_schema: None,
    }
}

/// `DaemonHealth.recovery` from the report `open_registry` wrote (spec
/// 0005): the reason code, the report file, and both schema versions.
#[must_use]
pub fn registry_recovery(report: &RecoveryReport) -> RegistryRecovery {
    RegistryRecovery {
        reason: report.reason.code().to_owned(),
        report_path: path_text(&report.path),
        detected_at: Timestamp::from_jiff(report.detected_at),
        app_schema_version: report.app_schema_version,
        db_schema_version: report.db_schema_version,
    }
}

/// `registry.recovery_mode`: the answer to every method not allowed while
/// the registry is in recovery mode (spec 0006 AC-18). Nothing was read or
/// written.
#[must_use]
pub fn recovery_mode(recovery: &RegistryRecovery) -> ErrorBody {
    ErrorBody::new(
        &registry::RECOVERY_MODE,
        "the registry is in recovery mode, so this method is not available",
    )
    .with_detail("reason", recovery.reason.as_str())
    .with_detail("reportPath", recovery.report_path.as_str())
    .with_hint("read the recovery report, repair the registry, then restart stacklyd")
}

/// The wire form of a registry resource, for the kinds spec 0006 names.
/// A lease or an artifact has no `ResourceKind`, so it travels without
/// one; a journal step travels as its operation.
#[must_use]
pub fn resource_ref(resource: &DomainResource) -> Option<ResourceRef> {
    let (kind, id) = match resource {
        DomainResource::Runtime(id) => (ResourceKind::Runtime, id.to_string()),
        DomainResource::Instance(id) => (ResourceKind::Instance, id.to_string()),
        DomainResource::Environment(id) => (ResourceKind::Environment, id.to_string()),
        DomainResource::Operation(id) | DomainResource::OperationStep(id, _) => {
            (ResourceKind::Operation, id.to_string())
        }
        DomainResource::ProviderRevision(key) => (
            ResourceKind::Provider,
            format!(
                "{}@{}#{}",
                key.provider(),
                key.revision(),
                key.manifest_sha256()
            ),
        ),
        _ => return None,
    };
    Some(ResourceRef { kind, id })
}

/// The error body of a registry failure (spec 0005, *Kode error di wire*,
/// AC-17). Messages are fixed sentences: the detail text some variants
/// carry is for logs only and never goes on the wire.
#[must_use]
pub fn registry_error(error: &RegistryError) -> ErrorBody {
    let about = |body: ErrorBody, resource: &DomainResource| match resource_ref(resource) {
        Some(resource) => body.with_resource(resource),
        None => body,
    };
    match error {
        RegistryError::NotFound { resource: r } => about(
            ErrorBody::new(&resource::NOT_FOUND, "the resource does not exist"),
            r,
        ),
        RegistryError::RevisionConflict {
            resource: r,
            expected,
            actual,
        } => about(
            ErrorBody::new(
                &resource::REVISION_MISMATCH,
                "the resource changed since it was read",
            )
            .with_detail("expected", expected.to_string())
            .with_detail("actual", actual.to_string())
            .with_hint("refresh the resource, then try again"),
            r,
        ),
        RegistryError::MissingReference { resource: r } => about(
            ErrorBody::new(
                &registry::MISSING_REFERENCE,
                "the write refers to a resource that does not exist",
            ),
            r,
        ),
        RegistryError::DuplicateResource { resource: r } => about(
            ErrorBody::new(&registry::DUPLICATE_RESOURCE, "the resource already exists"),
            r,
        ),
        RegistryError::GenerationConflict { expected, actual } => ErrorBody::new(
            &registry::GENERATION_CONFLICT,
            "the registry kept changing while the request ran",
        )
        .with_detail("expected", expected.get().to_string())
        .with_detail("actual", actual.get().to_string()),
        RegistryError::NameTaken { name, existing } => {
            ErrorBody::new(&registry::NAME_TAKEN, "another instance has this name")
                .with_detail("name", name.as_str())
                .with_detail("existing", existing.to_string())
        }
        RegistryError::ShortKeyTaken { key, existing } => ErrorBody::new(
            &registry::SHORT_KEY_TAKEN,
            "another instance has the same short key",
        )
        .with_detail("key", key.to_string())
        .with_detail("existing", existing.to_string()),
        RegistryError::PathTaken {
            root,
            path,
            existing,
        } => ErrorBody::new(
            &registry::PATH_TAKEN,
            "another instance stores the same path",
        )
        .with_detail("root", root.to_string())
        .with_detail("path", path.as_str())
        .with_detail("existing", existing.to_string()),
        RegistryError::DuplicateRuntime { existing } => {
            ErrorBody::new(&registry::DUPLICATE_RUNTIME, "the runtime already exists")
                .with_detail("existing", existing.to_string())
        }
        RegistryError::DedicatedOwnerExists {
            instance,
            environment,
        } => ErrorBody::new(
            &registry::DEDICATED_OWNER_EXISTS,
            "another environment already owns the instance",
        )
        .with_detail("instance", instance.to_string())
        .with_detail("environment", environment.to_string()),
        RegistryError::ActiveLeaseExists {
            instance,
            environment,
        } => ErrorBody::new(
            &registry::ACTIVE_LEASE_EXISTS,
            "the environment already holds an active lease on the instance",
        )
        .with_detail("instance", instance.to_string())
        .with_detail("environment", environment.to_string()),
        RegistryError::SoftwareMismatch {
            instance,
            runtime,
            provider_revision,
        } => ErrorBody::new(
            &registry::SOFTWARE_MISMATCH,
            "the instance software does not match its runtime and provider revision",
        )
        .with_detail("instance", instance.as_str())
        .with_detail("runtime", runtime.as_str())
        .with_detail("providerRevision", provider_revision.as_str()),
        RegistryError::NotUtf8Path { field } => {
            ErrorBody::new(&registry::PATH_NOT_UTF8, "a path is not valid utf-8")
                .with_detail("field", *field)
        }
        RegistryError::Busy => ErrorBody::new(&registry::BUSY, "the registry is busy"),
        RegistryError::Unavailable => {
            ErrorBody::new(&registry::UNAVAILABLE, "the registry is not available")
        }
        RegistryError::NoSpace => {
            ErrorBody::new(&registry::NO_SPACE, "the disk holding the registry is full")
        }
        RegistryError::Io { .. } => ErrorBody::new(
            &registry::IO_ERROR,
            "reading or writing the registry failed",
        ),
        RegistryError::Corrupt { table, column, .. } => {
            ErrorBody::new(&registry::CORRUPT, "a stored registry value cannot be read")
                .with_detail("table", *table)
                .with_detail("column", *column)
        }
        RegistryError::Integrity { .. } => ErrorBody::new(
            &registry::INTEGRITY,
            "the registry refused a write it should have accepted or rejected earlier",
        ),
        // `operation.idempotency_key_reused` belongs to spec 0007, whose
        // `operation` area is not in the catalog yet. provisional: F7
        RegistryError::IdempotencyKeyTaken { .. }
        | RegistryError::InvalidRevision { .. }
        | RegistryError::InvalidQuery { .. } => ErrorBody::new(
            &internal::UNEXPECTED,
            "a registry call broke an invariant inside the daemon",
        ),
        _ => ErrorBody::new(
            &internal::UNEXPECTED,
            "a registry failure this build cannot name",
        ),
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
    use stackly_domain::{
        EnvironmentId, Generation, InstanceId, LeaseId, LogicalRoot, OperationId,
        ProviderRevisionKey, RelativePath, ResourceRevision, RuntimeId, StepSeq,
    };
    use stackly_protocol::ErrorCategory;
    use stackly_testkit::{FakePlatform, TestRoot};

    use super::*;

    /// Maps `error` and checks the code, that the body conforms to the
    /// catalog entry (category, `retryable`, `details` keys), and that the
    /// detail values are the ones given.
    fn maps_to(error: &RegistryError, code: &str, details: &[(&str, &str)]) -> ErrorBody {
        let body = registry_error(error);
        assert_eq!(body.code(), code, "{error:?}");
        let spec = body.spec().expect("the code is in the catalog");
        assert_eq!(body.category(), spec.category);
        assert_eq!(body.retryable(), spec.retryable);
        let keys: Vec<_> = body.details().iter().map(|d| d.key()).collect();
        let expected: Vec<_> = details.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, expected, "{code}");
        for (key, value) in details {
            assert_eq!(body.detail(key), Some(*value), "{code} {key}");
        }
        assert!(
            body.message().starts_with(|c: char| c.is_ascii_lowercase())
                && !body.message().ends_with('.'),
            "{}",
            body.message()
        );
        body
    }

    fn uuid_of(text: &str) -> uuid::Uuid {
        text.parse().expect("the test UUIDs are valid")
    }

    const RUNTIME: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d01";
    const INSTANCE: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d02";
    const ENVIRONMENT: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d03";
    const OPERATION: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d04";

    fn instance() -> InstanceId {
        InstanceId::from_uuid(uuid_of(INSTANCE)).expect("the test UUIDs are version 7")
    }

    fn environment() -> EnvironmentId {
        EnvironmentId::from_uuid(uuid_of(ENVIRONMENT)).expect("the test UUIDs are version 7")
    }

    fn resource_of(body: &ErrorBody) -> Option<(ResourceKind, &str)> {
        body.resource().map(|r| (r.kind, r.id.as_str()))
    }

    #[test]
    fn not_found_maps_to_resource_not_found_with_the_resource() {
        let error = RegistryError::NotFound {
            resource: DomainResource::Instance(instance()),
        };
        let body = maps_to(&error, "resource.not_found", &[]);
        assert_eq!(resource_of(&body), Some((ResourceKind::Instance, INSTANCE)));
        assert_eq!(body.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn revision_conflict_maps_to_resource_revision_mismatch_with_both_revisions() {
        let error = RegistryError::RevisionConflict {
            resource: DomainResource::Environment(environment()),
            expected: ResourceRevision::new(3).unwrap(),
            actual: ResourceRevision::new(4).unwrap(),
        };
        let body = maps_to(
            &error,
            "resource.revision_mismatch",
            &[("expected", "3"), ("actual", "4")],
        );
        assert_eq!(
            resource_of(&body),
            Some((ResourceKind::Environment, ENVIRONMENT))
        );
    }

    #[test]
    fn missing_reference_maps_to_registry_missing_reference_with_the_resource() {
        let error = RegistryError::MissingReference {
            resource: DomainResource::Runtime(RuntimeId::from_uuid(uuid_of(RUNTIME)).unwrap()),
        };
        let body = maps_to(&error, "registry.missing_reference", &[]);
        assert_eq!(resource_of(&body), Some((ResourceKind::Runtime, RUNTIME)));
    }

    #[test]
    fn duplicate_resource_maps_to_registry_duplicate_resource() {
        let error = RegistryError::DuplicateResource {
            resource: DomainResource::Operation(
                OperationId::from_uuid(uuid_of(OPERATION)).unwrap(),
            ),
        };
        let body = maps_to(&error, "registry.duplicate_resource", &[]);
        assert_eq!(
            resource_of(&body),
            Some((ResourceKind::Operation, OPERATION))
        );
    }

    #[test]
    fn generation_conflict_maps_to_registry_generation_conflict() {
        let error = RegistryError::GenerationConflict {
            expected: Generation::new(7),
            actual: Generation::new(9),
        };
        maps_to(
            &error,
            "registry.generation_conflict",
            &[("expected", "7"), ("actual", "9")],
        );
    }

    #[test]
    fn invalid_revision_maps_to_internal_unexpected() {
        let error = RegistryError::InvalidRevision {
            resource: DomainResource::Instance(instance()),
            expected: ResourceRevision::new(1).unwrap(),
            actual: ResourceRevision::new(2).unwrap(),
        };
        let body = maps_to(&error, "internal.unexpected", &[]);
        assert_eq!(body.resource(), None);
    }

    #[test]
    fn invalid_query_maps_to_internal_unexpected() {
        let error = RegistryError::InvalidQuery {
            reason: "limit is 0",
        };
        let body = maps_to(&error, "internal.unexpected", &[]);
        assert!(!body.message().contains("limit"), "{}", body.message());
    }

    #[test]
    fn not_utf8_path_maps_to_registry_path_not_utf8_with_the_field() {
        let error = RegistryError::NotUtf8Path {
            field: "external_root",
        };
        maps_to(
            &error,
            "registry.path_not_utf8",
            &[("field", "external_root")],
        );
    }

    #[test]
    fn name_taken_maps_to_registry_name_taken_with_the_name_and_holder() {
        let error = RegistryError::NameTaken {
            name: "main".parse().unwrap(),
            existing: instance(),
        };
        maps_to(
            &error,
            "registry.name_taken",
            &[("name", "main"), ("existing", INSTANCE)],
        );
    }

    #[test]
    fn idempotency_key_taken_maps_to_internal_unexpected_until_spec_0007_adds_its_code() {
        let error = RegistryError::IdempotencyKeyTaken {
            key: "key-1".parse().unwrap(),
            existing: OperationId::from_uuid(uuid_of(OPERATION)).unwrap(),
        };
        let body = maps_to(&error, "internal.unexpected", &[]);
        assert!(!body.message().contains("key-1"));
    }

    #[test]
    fn short_key_taken_maps_to_registry_short_key_taken() {
        let error = RegistryError::ShortKeyTaken {
            key: "0a1b2c3d".parse().unwrap(),
            existing: instance(),
        };
        maps_to(
            &error,
            "registry.short_key_taken",
            &[("key", "0a1b2c3d"), ("existing", INSTANCE)],
        );
    }

    #[test]
    fn path_taken_maps_to_registry_path_taken_with_root_path_and_holder() {
        let error = RegistryError::PathTaken {
            root: LogicalRoot::Data,
            path: RelativePath::new("instances/main/data").unwrap(),
            existing: instance(),
        };
        let root = LogicalRoot::Data.to_string();
        maps_to(
            &error,
            "registry.path_taken",
            &[
                ("root", root.as_str()),
                ("path", "instances/main/data"),
                ("existing", INSTANCE),
            ],
        );
    }

    #[test]
    fn duplicate_runtime_maps_to_registry_duplicate_runtime() {
        let error = RegistryError::DuplicateRuntime {
            existing: RuntimeId::from_uuid(uuid_of(RUNTIME)).unwrap(),
        };
        maps_to(
            &error,
            "registry.duplicate_runtime",
            &[("existing", RUNTIME)],
        );
    }

    #[test]
    fn software_mismatch_maps_to_registry_software_mismatch_with_all_three() {
        let error = RegistryError::SoftwareMismatch {
            instance: "postgresql".parse().unwrap(),
            runtime: "valkey".parse().unwrap(),
            provider_revision: "nats".parse().unwrap(),
        };
        let body = maps_to(
            &error,
            "registry.software_mismatch",
            &[
                ("instance", "postgresql"),
                ("runtime", "valkey"),
                ("providerRevision", "nats"),
            ],
        );
        assert_eq!(body.category(), ErrorCategory::InvalidInput);
    }

    #[test]
    fn dedicated_owner_exists_maps_to_registry_dedicated_owner_exists() {
        let error = RegistryError::DedicatedOwnerExists {
            instance: instance(),
            environment: environment(),
        };
        maps_to(
            &error,
            "registry.dedicated_owner_exists",
            &[("instance", INSTANCE), ("environment", ENVIRONMENT)],
        );
    }

    #[test]
    fn active_lease_exists_maps_to_registry_active_lease_exists() {
        let error = RegistryError::ActiveLeaseExists {
            instance: instance(),
            environment: environment(),
        };
        maps_to(
            &error,
            "registry.active_lease_exists",
            &[("instance", INSTANCE), ("environment", ENVIRONMENT)],
        );
    }

    #[test]
    fn integrity_maps_to_registry_integrity_without_the_detail() {
        let error = RegistryError::Integrity {
            detail: "UNIQUE constraint failed: instances.name".to_owned(),
        };
        let body = maps_to(&error, "registry.integrity", &[]);
        assert!(!body.message().contains("UNIQUE"), "{}", body.message());
        assert_eq!(body.category(), ErrorCategory::Internal);
    }

    #[test]
    fn corrupt_maps_to_registry_corrupt_with_table_and_column_only() {
        let error = RegistryError::Corrupt {
            table: "instances",
            column: "desired_state",
            detail: "unknown code `sleepy`".to_owned(),
        };
        let body = maps_to(
            &error,
            "registry.corrupt",
            &[("table", "instances"), ("column", "desired_state")],
        );
        assert!(!body.message().contains("sleepy"));
    }

    #[test]
    fn no_space_maps_to_registry_no_space() {
        let body = maps_to(&RegistryError::NoSpace, "registry.no_space", &[]);
        assert_eq!(body.category(), ErrorCategory::Failed);
    }

    #[test]
    fn busy_maps_to_registry_busy_which_is_not_retryable() {
        let body = maps_to(&RegistryError::Busy, "registry.busy", &[]);
        assert_eq!(body.category(), ErrorCategory::Unavailable);
        assert!(!body.retryable());
    }

    #[test]
    fn unavailable_maps_to_registry_unavailable() {
        maps_to(&RegistryError::Unavailable, "registry.unavailable", &[]);
    }

    #[test]
    fn io_maps_to_registry_io_error_without_the_detail() {
        let error = RegistryError::Io {
            detail: "disk I/O error at /Users/someone".to_owned(),
        };
        let body = maps_to(&error, "registry.io_error", &[]);
        assert!(!body.message().contains("/Users"));
    }

    #[test]
    fn resources_without_a_wire_kind_travel_without_one_and_a_step_as_its_operation() {
        let lease = DomainResource::Lease(LeaseId::generate());
        assert_eq!(resource_ref(&lease), None);
        let artifact = DomainResource::Artifact("a".repeat(64).parse().unwrap());
        assert_eq!(resource_ref(&artifact), None);
        let step = DomainResource::OperationStep(
            OperationId::from_uuid(uuid_of(OPERATION)).unwrap(),
            StepSeq::FIRST,
        );
        assert_eq!(
            resource_ref(&step),
            Some(ResourceRef {
                kind: ResourceKind::Operation,
                id: OPERATION.to_owned(),
            })
        );
    }

    #[test]
    fn a_provider_revision_travels_as_its_spec_0009_key() {
        let key = ProviderRevisionKey::new(
            "official/postgresql".parse().unwrap(),
            "r2".parse().unwrap(),
            "b".repeat(64).parse().unwrap(),
        );
        let resource = resource_ref(&DomainResource::ProviderRevision(Box::new(key)))
            .expect("a provider revision has a wire kind");
        assert_eq!(resource.kind, ResourceKind::Provider);
        assert_eq!(
            resource.id,
            format!("official/postgresql@r2#{}", "b".repeat(64))
        );
    }

    #[test]
    fn recovery_mode_names_the_reason_and_the_report() {
        let recovery = RegistryRecovery {
            reason: "integrity_failed".to_owned(),
            report_path: "/data/recovery/a.json".to_owned(),
            detected_at: Timestamp::UNIX_EPOCH,
            app_schema_version: 2,
            db_schema_version: Some(2),
        };
        let body = recovery_mode(&recovery);
        assert!(body.is(&registry::RECOVERY_MODE));
        assert_eq!(body.detail("reason"), Some("integrity_failed"));
        assert_eq!(body.detail("reportPath"), Some("/data/recovery/a.json"));
        assert_eq!(body.category(), ErrorCategory::Unavailable);
        assert!(!body.retryable());
    }

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
