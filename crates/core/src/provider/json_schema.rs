//! JSON Schemas of the provider package files, for editors (spec 0009,
//! AC-17).
//!
//! They are generated from the raw decode types with schemars, so a schema
//! cannot drift from the decoder; `cargo xtask codegen` writes them to
//! `schemas/provider/`. The schemas describe structure, names, and versions
//! only: the semantic rules (closed sets, references, interpolation, the
//! settings dialect) stay in the validator, which is the authority.

use schemars::Schema;
use schemars::generate::SchemaSettings;

use super::raw::{RawManifest, RawSettingsSchema, RawVersions};

/// `SoftwareName` and each part of `ProviderName` (VAL-001), at most 63
/// characters.
pub(crate) const NAME: &str = "^[a-z]([a-z0-9]*(-[a-z0-9]+)*)?$";
/// `ProviderName`, `<namespace>/<name>` (VAL-002), at most 127 characters.
pub(crate) const PROVIDER_NAME: &str =
    "^[a-z]([a-z0-9]*(-[a-z0-9]+)*)?/[a-z]([a-z0-9]*(-[a-z0-9]+)*)?$";
/// `RevisionLabel` (VAL-002): letters, digits, and `. + _ -`, starting
/// with a letter or digit, without `..`, at most 64 characters.
pub(crate) const REVISION_LABEL: &str = r"^[A-Za-z0-9][A-Za-z0-9+_-]*(\.[A-Za-z0-9+_-]+)*\.?$";
/// `RuntimeLine`: a major version or `major.minor`.
pub(crate) const RUNTIME_LINE: &str = r"^[0-9]+(\.[0-9]+)?$";
/// `ExactVersion`, at most 64 characters.
pub(crate) const EXACT_VERSION: &str = r"^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$";
/// A setting key, item field, config file name, or executable alias.
pub(crate) const MANIFEST_NAME: &str = "^[a-z][a-zA-Z0-9]{0,31}$";
/// A `connection` entry name (spec 0021).
pub(crate) const CONNECTION_NAME: &str = "^[a-z][a-z0-9_]{0,31}$";
/// An endpoint name: a manifest name that is also a domain `EndpointName`.
pub(crate) const ENDPOINT_NAME: &str = "^[a-z][a-z0-9]{0,31}$";
/// An env key; the validator also refuses `PATH`, `HOME`, `TMPDIR`, and
/// the `DYLD_` and `LD_` prefixes.
pub(crate) const ENV_KEY: &str = "^[A-Z][A-Z0-9_]{0,63}$";
/// A template path in `configFiles`.
pub(crate) const TEMPLATE_PATH: &str = r"^templates/[a-z0-9][a-z0-9._-]{0,63}\.tmpl$";
/// An artifact ID in `versions.json`.
pub(crate) const ARTIFACT_ID: &str = "^[A-Za-z0-9._-]+$";
/// A date written `YYYY-MM-DD`.
pub(crate) const DATE: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";

/// The three provider package schemas, each with the file stem it is
/// written under: `provider.v1alpha1` for `provider.yaml`,
/// `settings.v1alpha1` for `schemas/settings.schema.json`, and
/// `versions.v1` for `versions.json`. Every object with properties has
/// `additionalProperties: false`, like the decoder.
#[must_use]
pub fn json_schemas() -> Vec<(&'static str, Schema)> {
    let generator = || SchemaSettings::draft2020_12().into_generator();
    vec![
        (
            "provider.v1alpha1",
            generator().into_root_schema_for::<RawManifest>(),
        ),
        (
            "settings.v1alpha1",
            generator().into_root_schema_for::<RawSettingsSchema>(),
        ),
        (
            "versions.v1",
            generator().into_root_schema_for::<RawVersions>(),
        ),
    ]
}
