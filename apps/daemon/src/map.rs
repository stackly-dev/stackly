//! Maps platform and domain values to the DTOs of `stackly-protocol`.
//! Boundaries carry DTOs only, so this is the one place that knows both.
//! [`catalog`] maps the catalog read model and [`redact`] hides secret values
//! in what the daemon sends.

pub mod catalog;
pub mod redact;

use std::path::Path;

use stackly_core::provider::{Issue, PackageError, PlanError, SettingsError};
use stackly_domain::{RegistryError, ResourceRef as DomainResource};
use stackly_platform::bundle::AppBundle;
use stackly_platform::{
    CapabilityStatus, DirectoryLayout, LaunchContext, PlatformCapability, PlatformInfo, UserId,
};
use stackly_protocol::codes::{
    PlanInvalidReason, RenderFailedReason, internal, manifest, operation, provider, registry,
    resource,
};
use stackly_protocol::daemon::{
    Capabilities, DaemonInfo, DaemonLaunch, DaemonRoots, LaunchContextKind,
    PlatformCapabilityStatus, RegistryRecovery,
};
use stackly_protocol::events::EventKind;
use stackly_protocol::{
    CONTRACT_DIGEST, Call, ErrorBody, ErrorItem, ProtocolVersion, ResourceKind, ResourceRef,
    Timestamp,
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
        secret_store: None,
        launch: None,
    }
}

/// `DaemonInfo.launch` from who started the daemon and the bundle it
/// runs from (spec 0015).
#[must_use]
pub fn daemon_launch(context: &LaunchContext, bundle: Option<&AppBundle>) -> DaemonLaunch {
    DaemonLaunch {
        context: match context {
            LaunchContext::Launchd { .. } => LaunchContextKind::Launchd,
            LaunchContext::Manual => LaunchContextKind::Manual,
        },
        label: context.label().map(str::to_owned),
        bundle_path: bundle.map(|bundle| path_text(bundle.root())),
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
        DomainResource::ProviderRevision(key) => (ResourceKind::Provider, key.to_string()),
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
        RegistryError::IdempotencyKeyTaken { existing, .. } => ErrorBody::new(
            &operation::IDEMPOTENCY_KEY_REUSED,
            "the idempotency key belongs to an operation another request started",
        )
        .with_detail("operationId", existing.to_string())
        .with_hint("send the request again with a new idempotency key"),
        RegistryError::InvalidRevision { .. } | RegistryError::InvalidQuery { .. } => {
            ErrorBody::new(
                &internal::UNEXPECTED,
                "a registry call broke an invariant inside the daemon",
            )
        }
        _ => ErrorBody::new(
            &internal::UNEXPECTED,
            "a registry failure this build cannot name",
        ),
    }
}

/// The item kind of one provider issue (spec 0009).
const ISSUE_ITEM: &str = "issue";

/// The `issue` item of one provider issue: its file, location, code, and
/// message, plus its reason when the code declares reasons (spec 0021).
/// Core promises an issue message never holds a settings text value or a
/// secret.
fn issue_item(issue: &Issue) -> ErrorItem {
    let item = ErrorItem::new(ISSUE_ITEM)
        .with_field("file", issue.file.as_str())
        .with_field("location", issue.location.to_string())
        .with_field("code", issue.code.as_code())
        .with_field("message", issue.message.as_str());
    match issue.reason {
        Some(reason) => item.with_field("reason", reason),
        None => item,
    }
}

/// `body` with the `issueCount` detail and one `issue` item per issue, at
/// most the catalog limit; the rest are counted in `omitted`.
fn with_issues(body: ErrorBody, issues: &[Issue]) -> ErrorBody {
    body.with_detail("issueCount", issues.len().to_string())
        .with_items(issues.iter().map(issue_item))
}

/// The error body of a provider package that cannot be read or is invalid
/// (spec 0009, *Kode error di wire*, AC-19): `manifest.invalid`.
#[must_use]
pub fn package_error(error: &PackageError) -> ErrorBody {
    with_issues(
        ErrorBody::new(&manifest::INVALID, "the provider package is invalid")
            .with_detail("package", error.package.as_str()),
        &error.issues,
    )
}

/// The error body of settings that do not match their schema (spec 0009,
/// AC-19): `provider.invalid_settings`. The issues name the fields, never
/// their values.
#[must_use]
pub fn settings_error(error: &SettingsError) -> ErrorBody {
    with_issues(
        ErrorBody::new(
            &provider::INVALID_SETTINGS,
            "the settings do not match the provider's settings schema",
        ),
        &error.issues,
    )
}

/// The error body of a plan that cannot be built (spec 0009, *Kode error
/// di wire*, AC-19). Messages are fixed sentences; the names a variant
/// carries go in `details`. An unresolved endpoint or an unavailable
/// secret is a bug in the daemon, so it becomes `internal.unexpected`.
#[must_use]
pub fn plan_error(error: &PlanError) -> ErrorBody {
    let plan_invalid = |position: &str, reason: PlanInvalidReason| {
        ErrorBody::new(
            &provider::PLAN_INVALID,
            "an argument or a path of the provider plan breaks a plan rule",
        )
        .with_detail("position", position)
        .with_detail("reason", reason.as_str())
    };
    let render_failed = |config_file: &str, reason: RenderFailedReason| {
        ErrorBody::new(
            &provider::RENDER_FAILED,
            "a rendered config file does not parse or is too large",
        )
        .with_detail("configFile", config_file)
        .with_detail("reason", reason.as_str())
    };
    let body = match error {
        PlanError::Unsupported { capability } => ErrorBody::new(
            &provider::UNSUPPORTED_CAPABILITY,
            "the provider does not support this action",
        )
        .with_detail("capability", capability.as_code()),
        PlanError::EndpointNotLoopback { endpoint } => ErrorBody::new(
            &provider::ENDPOINT_NOT_LOOPBACK,
            "an endpoint of the instance is not on a loopback address",
        )
        .with_detail("endpoint", endpoint.to_string()),
        PlanError::UnrepresentableValue { field } => ErrorBody::new(
            &provider::UNREPRESENTABLE_VALUE,
            "a value cannot be written in the format of its config file",
        )
        .with_detail("field", field.as_str()),
        PlanError::ArgumentTooLong { position } => {
            plan_invalid(position, PlanInvalidReason::ArgumentTooLong)
        }
        PlanError::PathNotUtf8 { position } => {
            plan_invalid(position, PlanInvalidReason::PathNotUtf8)
        }
        PlanError::PathEscapesScope { position } => {
            plan_invalid(position, PlanInvalidReason::PathEscapesScope)
        }
        PlanError::RenderedOutputInvalid { config_file } => {
            render_failed(config_file, RenderFailedReason::RenderedOutputInvalid)
        }
        PlanError::OutputTooLarge { config_file } => {
            render_failed(config_file, RenderFailedReason::OutputTooLarge)
        }
        PlanError::Settings(settings) => settings_error(settings),
        PlanError::EndpointUnresolved { .. } | PlanError::SecretUnavailable { .. } => {
            ErrorBody::new(
                &internal::UNEXPECTED,
                "building the provider plan broke an invariant inside the daemon",
            )
        }
        _ => ErrorBody::new(
            &internal::UNEXPECTED,
            "a provider plan failure this build cannot name",
        ),
    };
    match error.rule() {
        Some(rule) => body.with_rule(rule.id()),
        None => body,
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
    use std::collections::BTreeMap;

    use stackly_core::provider::{
        IssueCode, Location, NameCheck, PackageFiles, SettingsSchema, ValidatedPackage,
    };
    use stackly_domain::{
        Capability, EnvironmentId, Generation, InstanceId, LeaseId, LogicalRoot, OperationId,
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
        conforms(body, code, details)
    }

    /// Checks that `body` has `code`, conforms to its catalog entry
    /// (category, `retryable`, `details` keys, item kind, fields, and
    /// limit), and carries exactly the given details, in order.
    fn conforms(body: ErrorBody, code: &str, details: &[(&str, &str)]) -> ErrorBody {
        assert_eq!(body.code(), code, "{body:?}");
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
        match spec.items {
            None => assert!(body.items().is_empty(), "{code} allows no items"),
            Some(items) => {
                assert!(body.items().len() <= usize::from(items.max), "{code}");
                for item in body.items() {
                    assert_eq!(item.kind(), items.kind, "{code}");
                    for field in item.fields() {
                        assert!(items.fields.contains(&field.key()), "{code}: {field:?}");
                    }
                }
            }
        }
        body
    }

    /// Maps a plan error and checks it like [`conforms`].
    fn plan_maps_to(error: &PlanError, code: &str, details: &[(&str, &str)]) -> ErrorBody {
        conforms(plan_error(error), code, details)
    }

    /// The fake provider package from `providers/fake`, read into memory.
    fn fake_package() -> ValidatedPackage {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
        let files: BTreeMap<String, Vec<u8>> = [
            "provider.yaml",
            "versions.json",
            "schemas/settings.schema.json",
            "templates/fake.json.tmpl",
        ]
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(root.join(path)).expect("providers/fake holds this file");
            (path.to_owned(), bytes)
        })
        .collect();
        ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::FolderName)
            .expect("providers/fake is a valid package")
    }

    /// The settings error of the fake provider for `document`.
    fn settings_error_of(schema: &SettingsSchema, document: serde_json::Value) -> SettingsError {
        schema
            .validate(&document)
            .expect_err("the document breaks the schema")
    }

    /// A settings text value no error may echo.
    const SETTINGS_SENTINEL: &str = "settings-sentinel-0192f3a5";

    fn sentinel_settings_error() -> SettingsError {
        let package = fake_package();
        settings_error_of(
            package.settings_schema(),
            serde_json::json!({
                "greeting": SETTINGS_SENTINEL.repeat(20),
                "notASetting": SETTINGS_SENTINEL,
            }),
        )
    }

    fn assert_no_sentinel(body: &ErrorBody) {
        let json = serde_json::to_string(body).expect("an error body always encodes");
        assert!(!json.contains(SETTINGS_SENTINEL), "{json}");
    }

    #[test]
    fn a_package_error_maps_to_manifest_invalid_with_one_issue_item_per_issue() {
        let error = ValidatedPackage::from_files(
            PackageFiles::new("broken", BTreeMap::new()),
            NameCheck::Skip,
        )
        .expect_err("an empty package is invalid");
        let count = error.issues.len().to_string();
        let body = conforms(
            package_error(&error),
            "manifest.invalid",
            &[("package", "broken"), ("issueCount", count.as_str())],
        );
        assert_eq!(body.items().len(), error.issues.len());
        let first = &body.items()[0];
        assert_eq!(first.kind(), "issue");
        let keys: Vec<_> = first.fields().iter().map(|field| field.key()).collect();
        assert_eq!(keys, ["file", "location", "code", "message"]);
        assert_eq!(first.field("file"), Some(error.issues[0].file.as_str()));
        assert_eq!(first.field("code"), Some(error.issues[0].code.as_code()));
        assert_eq!(body.category(), ErrorCategory::InvalidInput);
    }

    #[test]
    fn an_issue_with_a_reason_carries_it_as_the_last_item_field() {
        let issue = stackly_core::provider::Issue::new(
            "provider.yaml",
            stackly_core::provider::Location::Field("connection[0].template".to_owned()),
            stackly_core::provider::IssueCode::ConnectionInvalid,
            "the template does not render to an absolute uri",
        )
        .with_reason("not_a_uri");

        let item = issue_item(&issue);

        let keys: Vec<_> = item.fields().iter().map(|field| field.key()).collect();
        assert_eq!(keys, ["file", "location", "code", "message", "reason"]);
        assert_eq!(item.field("reason"), Some("not_a_uri"));
    }

    #[test]
    fn more_than_100_issues_keep_100_items_and_count_the_rest_in_omitted() {
        let issues: Vec<_> = (0..101)
            .map(|index| {
                Issue::new(
                    "provider.yaml",
                    Location::Field(format!("lifecycle.start.args[{index}]")),
                    IssueCode::UnknownToken,
                    "unknown token",
                )
            })
            .collect();
        let body = with_issues(
            ErrorBody::new(&manifest::INVALID, "the provider package is invalid")
                .with_detail("package", "fake"),
            &issues,
        );
        let body = conforms(
            body,
            "manifest.invalid",
            &[("package", "fake"), ("issueCount", "101"), ("omitted", "1")],
        );
        assert_eq!(body.items().len(), 100);
        assert_eq!(
            body.items()[99].field("location"),
            Some("lifecycle.start.args[99]")
        );
    }

    #[test]
    fn an_issue_location_is_a_field_path_a_line_and_column_or_the_file() {
        let item = |location| {
            issue_item(&Issue::new(
                "versions.json",
                location,
                IssueCode::JsonSyntax,
                "m",
            ))
        };
        assert_eq!(
            item(Location::Position { line: 3, column: 7 }).field("location"),
            Some("3:7")
        );
        assert_eq!(
            item(Location::Field("metadata.id".to_owned())).field("location"),
            Some("metadata.id")
        );
        assert_eq!(item(Location::File).field("location"), Some("."));
    }

    #[test]
    fn a_settings_error_maps_to_provider_invalid_settings_without_the_values() {
        let error = sentinel_settings_error();
        let count = error.issues.len().to_string();
        let body = conforms(
            settings_error(&error),
            "provider.invalid_settings",
            &[("issueCount", count.as_str())],
        );
        assert_eq!(body.items().len(), error.issues.len());
        let files: Vec<_> = body.items().iter().map(|item| item.field("file")).collect();
        assert!(
            files.iter().all(|file| *file == Some("settings")),
            "{files:?}"
        );
        assert_no_sentinel(&body);
    }

    #[test]
    fn plan_unsupported_maps_to_provider_unsupported_capability_with_rule_br_016() {
        let error = PlanError::Unsupported {
            capability: Capability::Reload,
        };
        let code = Capability::Reload.as_code();
        let body = plan_maps_to(
            &error,
            "provider.unsupported_capability",
            &[("capability", code)],
        );
        assert_eq!(body.rule(), Some("BR-016"));
        assert_eq!(body.category(), ErrorCategory::Unsupported);
    }

    #[test]
    fn plan_endpoint_not_loopback_maps_to_provider_endpoint_not_loopback() {
        let error = PlanError::EndpointNotLoopback {
            endpoint: "client".parse().unwrap(),
        };
        let body = plan_maps_to(
            &error,
            "provider.endpoint_not_loopback",
            &[("endpoint", "client")],
        );
        assert_eq!(body.rule(), None);
    }

    #[test]
    fn plan_unrepresentable_value_maps_to_provider_unrepresentable_value() {
        let error = PlanError::UnrepresentableValue {
            field: "settings.greeting".to_owned(),
        };
        plan_maps_to(
            &error,
            "provider.unrepresentable_value",
            &[("field", "settings.greeting")],
        );
    }

    #[test]
    fn plan_argument_too_long_maps_to_provider_plan_invalid() {
        let error = PlanError::ArgumentTooLong {
            position: "lifecycle.start.args[2]".to_owned(),
        };
        plan_maps_to(
            &error,
            "provider.plan_invalid",
            &[
                ("position", "lifecycle.start.args[2]"),
                ("reason", "argument_too_long"),
            ],
        );
    }

    #[test]
    fn plan_path_not_utf8_maps_to_provider_plan_invalid() {
        let error = PlanError::PathNotUtf8 {
            position: "${instance.data}".to_owned(),
        };
        plan_maps_to(
            &error,
            "provider.plan_invalid",
            &[
                ("position", "${instance.data}"),
                ("reason", "path_not_utf8"),
            ],
        );
    }

    #[test]
    fn plan_path_escapes_scope_maps_to_provider_plan_invalid() {
        let error = PlanError::PathEscapesScope {
            position: "${instance.run}".to_owned(),
        };
        plan_maps_to(
            &error,
            "provider.plan_invalid",
            &[
                ("position", "${instance.run}"),
                ("reason", "path_escapes_scope"),
            ],
        );
    }

    #[test]
    fn plan_rendered_output_invalid_maps_to_provider_render_failed() {
        let error = PlanError::RenderedOutputInvalid {
            config_file: "fakeConfig".to_owned(),
        };
        let body = plan_maps_to(
            &error,
            "provider.render_failed",
            &[
                ("configFile", "fakeConfig"),
                ("reason", "rendered_output_invalid"),
            ],
        );
        assert_eq!(body.category(), ErrorCategory::Failed);
    }

    #[test]
    fn plan_output_too_large_maps_to_provider_render_failed() {
        let error = PlanError::OutputTooLarge {
            config_file: "fakeConfig".to_owned(),
        };
        plan_maps_to(
            &error,
            "provider.render_failed",
            &[("configFile", "fakeConfig"), ("reason", "output_too_large")],
        );
    }

    #[test]
    fn plan_settings_maps_to_provider_invalid_settings_without_the_values() {
        let settings = sentinel_settings_error();
        let count = settings.issues.len().to_string();
        let body = plan_maps_to(
            &PlanError::Settings(settings),
            "provider.invalid_settings",
            &[("issueCount", count.as_str())],
        );
        assert!(!body.items().is_empty());
        assert_no_sentinel(&body);
    }

    #[test]
    fn plan_endpoint_unresolved_maps_to_internal_unexpected() {
        let error = PlanError::EndpointUnresolved {
            endpoint: "client".parse().unwrap(),
        };
        let body = plan_maps_to(&error, "internal.unexpected", &[]);
        assert!(!body.message().contains("client"), "{}", body.message());
    }

    #[test]
    fn plan_secret_unavailable_maps_to_internal_unexpected() {
        let error = PlanError::SecretUnavailable {
            field: "authToken".parse().unwrap(),
        };
        let body = plan_maps_to(&error, "internal.unexpected", &[]);
        assert!(!body.message().contains("authToken"), "{}", body.message());
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
    fn idempotency_key_taken_maps_to_operation_idempotency_key_reused_with_the_first_operation() {
        let error = RegistryError::IdempotencyKeyTaken {
            key: "key-1".parse().unwrap(),
            existing: OperationId::from_uuid(uuid_of(OPERATION)).unwrap(),
        };
        let body = maps_to(
            &error,
            "operation.idempotency_key_reused",
            &[("operationId", OPERATION)],
        );
        assert_eq!(body.category(), ErrorCategory::InvalidInput);
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
        let resource = resource_ref(&DomainResource::ProviderRevision(Box::new(key.clone())))
            .expect("a provider revision has a wire kind");
        assert_eq!(resource.kind, ResourceKind::Provider);
        assert_eq!(
            resource.id,
            format!("official/postgresql@r2#{}", "b".repeat(64))
        );
        assert_eq!(resource.id.parse::<ProviderRevisionKey>().unwrap(), key);
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
