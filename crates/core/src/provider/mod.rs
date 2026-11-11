//! The provider engine (spec 0009): reads a provider package through the
//! `Filesystem` port, validates `stackly/v1alpha1` strictly, and builds
//! execution plans as pure data.
//!
//! The engine only reads, validates, and plans. It never runs a process,
//! writes a file, or resolves a secret; every side effect happens in the
//! executor of spec 0010, from a validated plan.

pub mod check;
mod connection;
mod digest;
#[doc(hidden)]
pub mod fuzzing;
mod interpolate;
mod issue;
#[cfg(feature = "json-schema")]
mod json_schema;
mod manifest;
mod names;
mod open_with;
mod package;
pub(crate) mod permission_facts;
mod plan;
mod preflight;
pub mod probe;
mod raw;
mod remedies;
pub mod scenarios;
mod settings;
mod template;
#[cfg(feature = "test-support")]
pub mod testing;
mod timeouts;
mod validate;
mod version_check;
mod versions;

pub use check::{
    ApiVersionStatus, ApiVersionSupport, Finding, FindingOrigin, PackageCheck,
    SUPPORTED_API_VERSIONS, Severity, check_package,
};
pub use connection::{
    CONNECTION_ENTRIES_MAX, CONNECTION_TEMPLATE_MAX, CONNECTION_TITLE_MAX, ConnectionSpec,
};
pub(crate) use connection::{RenderFailure, RenderedEntry, RenderedPiece, render_entries};
pub use interpolate::{EndpointTransport, InstanceDir};
pub use issue::{Issue, IssueCode, Location, MAX_ISSUES};
#[cfg(feature = "json-schema")]
pub use json_schema::json_schemas;
pub use manifest::{Category, EndpointPurpose, EndpointSpec, Manifest, StopSignal};
pub(crate) use names::{COMPONENT_RULE, is_component};
pub use names::{InvalidSettingName, SettingName};
pub use open_with::{
    OPEN_WITH_ARGS_MAX, OPEN_WITH_BUNDLE_IDS_MAX, OPEN_WITH_ENTRIES_MAX, OPEN_WITH_ENV_MAX,
    OPEN_WITH_TITLE_MAX, OPEN_WITH_URL_MAX, OPEN_WITH_VALUE_MAX, OpenWithKind, OpenWithSpec,
};
pub(crate) use open_with::{RenderedOpenEntry, RenderedOpenValue, render_open_entries};
pub use package::{
    MANIFEST_MAX_LEN, NameCheck, PACKAGE_MAX_LEN, PackageError, PackageFiles,
    SETTINGS_SCHEMA_MAX_LEN, TEMPLATE_MAX_LEN, TEMPLATES_MAX, VERSIONS_MAX_LEN, read_package,
};
pub use plan::{
    ARGUMENT_MAX_LEN, CHILD_PATH, Command, EnvValue, ExecutablePath, FileWrite, HealthPlan,
    InitPlan, InitStep, NativeProbeSpec, PlanContext, PlanError, PlanSecret, ProbePlan, ProbeSpec,
    ProbeTarget, ReloadAction, ReloadPlan, RenderedContent, ResolvedEndpoint, RunPlan, RunSpec,
    RuntimeContext, Segment, Stdin, StopPlan,
};
pub use preflight::{DATA_MARKER_MAX_LEN, MarkerRead, PreflightOutcome};
pub use remedies::{
    REMEDIES_MAX, REMEDY_COMMAND_MAX, REMEDY_PATH_MAX, REMEDY_STEPS_MAX, REMEDY_TEXT_MAX,
    REMEDY_TITLE_MAX, REMEDY_WHEN_CODES_MAX, REMEDY_WHEN_TEXT_MAX, RemedySpec, RemedySubject,
    RenderedRemedy, RenderedRemedyStep, render_remedy,
};
pub use scenarios::{Expect, Scenario, Scenarios, read_scenarios};
pub use settings::{
    ApplyClass, ArrayRules, BooleanRules, FieldKind, IntegerRules, ItemField, Pattern,
    SETTINGS_DOCUMENT_MAX_LEN, ScalarKind, ScalarValue, SettingClass, SettingField, SettingValue,
    SettingsError, SettingsIssueKind, SettingsItem, SettingsSchema, SettingsValues, TextRules,
    Visibility,
};
pub use template::{OUTPUT_MAX_LEN, TemplateFormat};
pub use timeouts::ManifestTimings;
pub use validate::ValidatedPackage;
pub use version_check::{
    VERSION_CHECK_ARGS_MAX, VERSION_CHECK_TIMEOUT_DEFAULT, VERSION_CHECK_TIMEOUT_MAX_SECONDS,
    VersionCheck,
};
pub use versions::{
    ArtifactDependency, ArtifactStatus, DependencyKind, SigningPolicy, UnpackedSize,
    VERSIONS_SCHEMA, VersionArtifact, VersionCatalog, manifest_software,
};
