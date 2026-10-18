//! The provider engine (spec 0009): reads a provider package through the
//! `Filesystem` port, validates `stackly/v1alpha1` strictly, and builds
//! execution plans as pure data.
//!
//! The engine only reads, validates, and plans. It never runs a process,
//! writes a file, or resolves a secret; every side effect happens in the
//! executor of spec 0010, from a validated plan.

mod digest;
mod interpolate;
mod issue;
mod manifest;
mod names;
mod package;
mod plan;
mod preflight;
mod raw;
mod secret;
mod settings;
mod template;
mod timeouts;
mod validate;
mod versions;

pub use interpolate::{EndpointTransport, InstanceDir};
pub use issue::{Issue, IssueCode, Location, MAX_ISSUES};
pub use manifest::{Category, EndpointPurpose, EndpointSpec, Manifest, StopSignal};
pub use names::{InvalidSettingName, SettingName};
pub use package::{
    MANIFEST_MAX_LEN, NameCheck, PACKAGE_MAX_LEN, PackageError, PackageFiles,
    SETTINGS_SCHEMA_MAX_LEN, TEMPLATE_MAX_LEN, TEMPLATES_MAX, VERSIONS_MAX_LEN, read_package,
};
pub use plan::{
    ARGUMENT_MAX_LEN, CHILD_PATH, Command, ExecutablePath, FileWrite, HealthPlan, InitPlan,
    InitStep, PlanContext, PlanError, PlanSecret, ProbePlan, ProbeSpec, ProbeTarget, ReloadAction,
    ReloadPlan, RenderedContent, ResolvedEndpoint, RunPlan, RunSpec, RuntimeContext,
    SecretResolveError, Segment, Stdin, StopPlan,
};
pub use preflight::{DATA_MARKER_MAX_LEN, MarkerRead, PreflightOutcome};
pub use secret::{SecretRef, SecretRefError};
pub use settings::{
    ApplyClass, ArrayRules, BooleanRules, FieldKind, IntegerRules, ItemField, Pattern,
    SETTINGS_DOCUMENT_MAX_LEN, ScalarKind, ScalarValue, SettingClass, SettingField, SettingValue,
    SettingsError, SettingsItem, SettingsSchema, SettingsValues, TextRules, Visibility,
};
pub use template::{OUTPUT_MAX_LEN, TemplateFormat};
pub use validate::ValidatedPackage;
