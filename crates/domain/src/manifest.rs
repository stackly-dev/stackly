//! The `instance-manifest.json` that every instance folder carries, so the
//! folder explains itself without the registry.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{ValueError, ValueKind};
use crate::id::{InstanceId, OperationId, RuntimeId};
use crate::provider::ProviderRevisionKey;
use crate::target::{Arch, Os, Target};
use crate::text::serde_as_text;
use crate::value::{
    DisplayName, ExactVersion, InstanceName, RuntimeLine, Sha256Digest, SoftwareName,
};

/// The `schema` value of the instance manifest this version reads and
/// writes.
pub const INSTANCE_MANIFEST_SCHEMA: &str = "stackly.instance-manifest/v1";

/// The `schema` field of a v1 manifest. It accepts only
/// [`INSTANCE_MANIFEST_SCHEMA`], so a manifest of another schema never
/// decodes as v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SchemaV1;

impl fmt::Display for SchemaV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(INSTANCE_MANIFEST_SCHEMA)
    }
}

impl FromStr for SchemaV1 {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == INSTANCE_MANIFEST_SCHEMA {
            Ok(Self)
        } else {
            Err(ValueError::new(
                ValueKind::InstanceManifest,
                "schema must be stackly.instance-manifest/v1",
            ))
        }
    }
}

/// How a runtime got onto the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallKind {
    /// Stackly downloaded, verified, and installed the runtime itself.
    Managed,
    /// The runtime lives in a folder the user owns, and Stackly only
    /// registered it (BR-019).
    External,
}

impl InstallKind {
    /// Every install kind, in code order.
    pub const ALL: [Self; 2] = [Self::Managed, Self::External];

    /// Returns the stable code, `managed` or `external`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::External => "external",
        }
    }
}

impl fmt::Display for InstallKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for InstallKind {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_code() == s)
            .ok_or(ValueError::new(
                ValueKind::InstallKind,
                "must be managed or external",
            ))
    }
}

serde_as_text! {
    SchemaV1 => "the schema stackly.instance-manifest/v1",
    InstallKind => "an install mode: managed or external",
}

/// The runtime an instance runs on, as its manifest records it.
///
/// A managed runtime always has the digest of its artifact, and an external
/// runtime never has one. In JSON this is
/// `{ id, version, line, os, arch, installMode, artifactSha256 }`, with
/// `artifactSha256` null for an external runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RuntimeRecord", into = "RuntimeRecord")]
pub struct ManifestRuntime {
    id: RuntimeId,
    version: ExactVersion,
    line: RuntimeLine,
    target: Target,
    /// `Some` for a managed runtime, `None` for an external one.
    artifact_sha256: Option<Sha256Digest>,
}

impl ManifestRuntime {
    /// Records a managed runtime, installed from the artifact with the given
    /// digest.
    #[must_use]
    pub const fn managed(
        id: RuntimeId,
        version: ExactVersion,
        line: RuntimeLine,
        target: Target,
        artifact_sha256: Sha256Digest,
    ) -> Self {
        Self {
            id,
            version,
            line,
            target,
            artifact_sha256: Some(artifact_sha256),
        }
    }

    /// Records an external runtime, which has no artifact.
    #[must_use]
    pub const fn external(
        id: RuntimeId,
        version: ExactVersion,
        line: RuntimeLine,
        target: Target,
    ) -> Self {
        Self {
            id,
            version,
            line,
            target,
            artifact_sha256: None,
        }
    }

    /// Returns the runtime's ID.
    #[must_use]
    pub const fn id(&self) -> RuntimeId {
        self.id
    }

    /// Returns the runtime's exact version.
    #[must_use]
    pub const fn version(&self) -> &ExactVersion {
        &self.version
    }

    /// Returns the runtime's line.
    #[must_use]
    pub const fn line(&self) -> &RuntimeLine {
        &self.line
    }

    /// Returns the OS and architecture the runtime is built for.
    #[must_use]
    pub const fn target(&self) -> Target {
        self.target
    }

    /// Returns whether the runtime is managed or external.
    #[must_use]
    pub const fn install_kind(&self) -> InstallKind {
        match self.artifact_sha256 {
            Some(_) => InstallKind::Managed,
            None => InstallKind::External,
        }
    }

    /// Returns the digest of the installed artifact, or `None` for an
    /// external runtime.
    #[must_use]
    pub const fn artifact_sha256(&self) -> Option<&Sha256Digest> {
        self.artifact_sha256.as_ref()
    }
}

/// The JSON form of [`ManifestRuntime`].
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRecord {
    id: RuntimeId,
    version: ExactVersion,
    line: RuntimeLine,
    os: Os,
    arch: Arch,
    install_mode: InstallKind,
    #[serde(default)]
    artifact_sha256: Option<Sha256Digest>,
}

impl From<ManifestRuntime> for RuntimeRecord {
    fn from(runtime: ManifestRuntime) -> Self {
        Self {
            install_mode: runtime.install_kind(),
            id: runtime.id,
            version: runtime.version,
            line: runtime.line,
            os: runtime.target.os(),
            arch: runtime.target.arch(),
            artifact_sha256: runtime.artifact_sha256,
        }
    }
}

impl TryFrom<RuntimeRecord> for ManifestRuntime {
    type Error = ValueError;

    fn try_from(record: RuntimeRecord) -> Result<Self, Self::Error> {
        let target = Target::new(record.os, record.arch);
        match (record.install_mode, record.artifact_sha256) {
            (InstallKind::Managed, Some(digest)) => Ok(Self::managed(
                record.id,
                record.version,
                record.line,
                target,
                digest,
            )),
            (InstallKind::External, None) => Ok(Self::external(
                record.id,
                record.version,
                record.line,
                target,
            )),
            _ => Err(ValueError::new(
                ValueKind::InstanceManifest,
                "artifactSha256 must be set for a managed runtime and null for an external one",
            )),
        }
    }
}

/// The data compatibility record of a manifest: the line and version that
/// initialized the data, and when initialization succeeded.
///
/// The line and version change only through a data migration (BR-009,
/// BR-013).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestData {
    initialized_line: RuntimeLine,
    initialized_version: ExactVersion,
    #[serde(default)]
    initialized_at: Option<Timestamp>,
}

impl ManifestData {
    /// Returns the runtime line that initialized the data.
    #[must_use]
    pub const fn initialized_line(&self) -> &RuntimeLine {
        &self.initialized_line
    }

    /// Returns the exact runtime version that initialized the data.
    #[must_use]
    pub const fn initialized_version(&self) -> &ExactVersion {
        &self.initialized_version
    }

    /// Returns when initialization succeeded, or `None` while it has not.
    #[must_use]
    pub const fn initialized_at(&self) -> Option<Timestamp> {
        self.initialized_at
    }
}

/// Marks a manifest whose instance folder was moved to `retained/`.
///
/// It must be present in `retained/` and absent in `instances/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedMarker {
    retained_at: Timestamp,
    by_operation: OperationId,
}

impl RetainedMarker {
    /// Creates the marker for an instance retained at `retained_at` by the
    /// operation `by_operation`.
    #[must_use]
    pub const fn new(retained_at: Timestamp, by_operation: OperationId) -> Self {
        Self {
            retained_at,
            by_operation,
        }
    }

    /// Returns when the instance was retained.
    #[must_use]
    pub const fn retained_at(&self) -> Timestamp {
        self.retained_at
    }

    /// Returns the operation that retained the instance.
    #[must_use]
    pub const fn by_operation(&self) -> OperationId {
        self.by_operation
    }
}

/// The values a new [`InstanceManifest`] starts from, when the instance is
/// created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInstanceManifest {
    /// The new instance's ID, created by the daemon.
    pub instance_id: InstanceId,
    /// The instance name from the create input.
    pub name: InstanceName,
    /// The optional display name from the create input.
    pub display_name: Option<DisplayName>,
    /// The software of the provider revision.
    pub software: SoftwareName,
    /// The provider revision the instance uses.
    pub provider: ProviderRevisionKey,
    /// The runtime the instance runs on. Its line and version also become
    /// the data's initialized line and version.
    pub runtime: ManifestRuntime,
    /// The time of the create, from the platform clock.
    pub created_at: Timestamp,
    /// The create operation, the base of its compensation (BR-023).
    pub created_by_operation: OperationId,
    /// The version of the Stackly daemon that writes the manifest.
    pub stackly_version: String,
}

/// The `instance-manifest.json` at the root of every instance folder,
/// schema [`INSTANCE_MANIFEST_SCHEMA`].
///
/// The registry stays the source of truth; the manifest mirrors enough of it
/// that a folder can be recognized without the registry. It holds no secret,
/// endpoint, desired state, config, or path: paths follow from the layout
/// and the ID. JSON is camelCase, and decoding ignores unknown fields, so
/// new optional fields can join v1. Encoding and decoding JSON happen in
/// `stackly-core`.
///
/// ```
/// use stackly_domain::{
///     Arch, InstanceId, InstanceManifest, ManifestRuntime, NewInstanceManifest, OperationId,
///     Os, ProviderRevisionKey, RuntimeId, Target,
/// };
///
/// let digest = "cb95f16d56a9ab7b938a03732ddf4ab691346124747fbb811ac6c09928f1fa6b".parse()?;
/// let manifest = InstanceManifest::new(NewInstanceManifest {
///     instance_id: InstanceId::generate(),
///     name: "orders-db".parse()?,
///     display_name: None,
///     software: "postgresql".parse()?,
///     provider: ProviderRevisionKey::new("official/postgresql".parse()?, "1".parse()?, digest),
///     runtime: ManifestRuntime::external(
///         RuntimeId::generate(),
///         "17.6".parse()?,
///         "17".parse()?,
///         Target::new(Os::Macos, Arch::Aarch64),
///     ),
///     created_at: "2026-10-18T09:30:00Z".parse().expect("valid timestamp"),
///     created_by_operation: OperationId::generate(),
///     stackly_version: "0.1.0".to_owned(),
/// });
/// assert_eq!(manifest.data().initialized_line().as_str(), "17");
/// assert_eq!(manifest.data().initialized_at(), None);
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceManifest {
    schema: SchemaV1,
    instance_id: InstanceId,
    name: InstanceName,
    #[serde(default)]
    display_name: Option<DisplayName>,
    software: SoftwareName,
    provider: ProviderRevisionKey,
    runtime: ManifestRuntime,
    data: ManifestData,
    created_at: Timestamp,
    created_by_operation: OperationId,
    updated_at: Timestamp,
    stackly_version: String,
    #[serde(default)]
    retained: Option<RetainedMarker>,
}

impl InstanceManifest {
    /// Creates the manifest of a new instance. The data starts with the
    /// runtime's line and version and no `initializedAt`, `updatedAt`
    /// equals `createdAt`, and the instance is not retained.
    #[must_use]
    pub fn new(new: NewInstanceManifest) -> Self {
        let data = ManifestData {
            initialized_line: new.runtime.line.clone(),
            initialized_version: new.runtime.version.clone(),
            initialized_at: None,
        };
        Self {
            schema: SchemaV1,
            instance_id: new.instance_id,
            name: new.name,
            display_name: new.display_name,
            software: new.software,
            provider: new.provider,
            runtime: new.runtime,
            data,
            created_at: new.created_at,
            created_by_operation: new.created_by_operation,
            updated_at: new.created_at,
            stackly_version: new.stackly_version,
            retained: None,
        }
    }

    /// Returns the instance ID, which must equal the folder name.
    #[must_use]
    pub const fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    /// Returns the instance name.
    #[must_use]
    pub const fn name(&self) -> &InstanceName {
        &self.name
    }

    /// Returns the display name, if any.
    #[must_use]
    pub const fn display_name(&self) -> Option<&DisplayName> {
        self.display_name.as_ref()
    }

    /// Returns the software.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.software
    }

    /// Returns the provider revision.
    #[must_use]
    pub const fn provider(&self) -> &ProviderRevisionKey {
        &self.provider
    }

    /// Returns the runtime the instance runs on.
    #[must_use]
    pub const fn runtime(&self) -> &ManifestRuntime {
        &self.runtime
    }

    /// Returns the data compatibility record.
    #[must_use]
    pub const fn data(&self) -> &ManifestData {
        &self.data
    }

    /// Returns when the instance was created.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Returns the operation that created the instance.
    #[must_use]
    pub const fn created_by_operation(&self) -> OperationId {
        self.created_by_operation
    }

    /// Returns when the manifest was last written.
    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    /// Returns the version of the Stackly daemon that last wrote the
    /// manifest.
    #[must_use]
    pub fn stackly_version(&self) -> &str {
        &self.stackly_version
    }

    /// Returns the retained marker, or `None` for an active instance.
    #[must_use]
    pub const fn retained(&self) -> Option<&RetainedMarker> {
        self.retained.as_ref()
    }

    /// Records that data initialization succeeded at `at`.
    pub fn record_initialized(&mut self, at: Timestamp) {
        self.data.initialized_at = Some(at);
    }

    /// Records that the instance was retained, before its folder moves to
    /// `retained/`.
    pub fn record_retained(&mut self, marker: RetainedMarker) {
        self.retained = Some(marker);
    }

    /// Records a write of the manifest: the time from the platform clock and
    /// the version of the daemon that writes it. Call it before every
    /// rewrite.
    pub fn record_write(&mut self, at: Timestamp, stackly_version: String) {
        self.updated_at = at;
        self.stackly_version = stackly_version;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "da578821d15e9c0f59eb8e4672962c6f032074e802684576debc6dfc7f86815f";

    fn at(text: &str) -> Timestamp {
        text.parse().expect("test timestamps are valid RFC 3339")
    }

    fn runtime(kind: InstallKind) -> ManifestRuntime {
        let id = RuntimeId::generate();
        let version: ExactVersion = "17.6".parse().expect("17.6 is an exact version");
        let line: RuntimeLine = "17".parse().expect("17 is a runtime line");
        let target = Target::new(Os::Macos, Arch::Aarch64);
        match kind {
            InstallKind::Managed => ManifestRuntime::managed(
                id,
                version,
                line,
                target,
                DIGEST.parse().expect("DIGEST is 64 hex characters"),
            ),
            InstallKind::External => ManifestRuntime::external(id, version, line, target),
        }
    }

    fn manifest(kind: InstallKind) -> InstanceManifest {
        InstanceManifest::new(NewInstanceManifest {
            instance_id: InstanceId::generate(),
            name: "orders-db".parse().expect("orders-db is an instance name"),
            display_name: None,
            software: "postgresql".parse().expect("postgresql is a software name"),
            provider: ProviderRevisionKey::new(
                "official/postgresql"
                    .parse()
                    .expect("official/postgresql is a provider name"),
                "1".parse().expect("1 is a revision label"),
                DIGEST.parse().expect("DIGEST is 64 hex characters"),
            ),
            runtime: runtime(kind),
            created_at: at("2026-10-18T09:30:00Z"),
            created_by_operation: OperationId::generate(),
            stackly_version: "0.1.0".to_owned(),
        })
    }

    #[test]
    fn new_manifest_starts_uninitialized_active_and_unchanged_since_create() {
        let manifest = manifest(InstallKind::Managed);

        assert_eq!(manifest.updated_at(), manifest.created_at());
        assert_eq!(
            manifest.data().initialized_line(),
            manifest.runtime().line()
        );
        assert_eq!(
            manifest.data().initialized_version(),
            manifest.runtime().version()
        );
        assert_eq!(manifest.data().initialized_at(), None);
        assert_eq!(manifest.retained(), None);
        assert_eq!(manifest.display_name(), None);
    }

    #[test]
    fn schema_is_the_first_field_and_absent_values_are_written_as_null() {
        let manifest = manifest(InstallKind::External);
        let text = String::from_utf8(serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert!(
            text.starts_with("{\n  \"schema\": \"stackly.instance-manifest/v1\","),
            "{text}"
        );
        assert_eq!(json["displayName"], serde_json::Value::Null);
        assert_eq!(json["data"]["initializedAt"], serde_json::Value::Null);
        assert_eq!(json["retained"], serde_json::Value::Null);
        assert_eq!(json["runtime"]["installMode"], "external");
        assert_eq!(json["runtime"]["artifactSha256"], serde_json::Value::Null);
        assert_eq!(json["createdAt"], "2026-10-18T09:30:00Z");
    }

    #[test]
    fn manifest_round_trips_through_json() {
        for kind in InstallKind::ALL {
            let mut manifest = manifest(kind);
            manifest.record_initialized(at("2026-10-18T09:30:05Z"));
            manifest.record_retained(RetainedMarker::new(
                at("2026-10-18T10:00:00Z"),
                OperationId::generate(),
            ));
            let json = serde_json::to_vec(&manifest).unwrap();

            assert_eq!(
                serde_json::from_slice::<InstanceManifest>(&json).unwrap(),
                manifest
            );
            assert_eq!(manifest.runtime().install_kind(), kind);
        }
    }

    #[test]
    fn recording_changes_only_the_recorded_fields() {
        let mut manifest = manifest(InstallKind::Managed);
        let before = manifest.clone();
        let retained = RetainedMarker::new(at("2026-10-18T10:00:00Z"), OperationId::generate());

        manifest.record_initialized(at("2026-10-18T09:30:05Z"));
        manifest.record_retained(retained);
        manifest.record_write(at("2026-10-18T10:00:00Z"), "0.2.0".to_owned());

        assert_eq!(
            manifest.data().initialized_at(),
            Some(at("2026-10-18T09:30:05Z"))
        );
        assert_eq!(manifest.retained(), Some(&retained));
        assert_eq!(manifest.updated_at(), at("2026-10-18T10:00:00Z"));
        assert_eq!(manifest.stackly_version(), "0.2.0");
        assert_eq!(manifest.created_at(), before.created_at());
        assert_eq!(manifest.name(), before.name());
        assert_eq!(manifest.runtime(), before.runtime());
    }

    #[test]
    fn install_mode_and_artifact_digest_must_agree() {
        let mut managed = serde_json::to_value(manifest(InstallKind::Managed)).unwrap();
        managed["runtime"]["artifactSha256"] = serde_json::Value::Null;
        let mut external = serde_json::to_value(manifest(InstallKind::External)).unwrap();
        external["runtime"]["artifactSha256"] = DIGEST.into();

        for json in [managed, external] {
            let err = serde_json::from_value::<InstanceManifest>(json).unwrap_err();
            assert!(
                err.to_string().contains("artifactSha256 must be set"),
                "{err}"
            );
        }
    }

    #[test]
    fn only_the_v1_schema_decodes() {
        let mut json = serde_json::to_value(manifest(InstallKind::Managed)).unwrap();
        json["schema"] = "stackly.instance-manifest/v2".into();

        let err = serde_json::from_value::<InstanceManifest>(json).unwrap_err();
        assert!(err.to_string().contains(INSTANCE_MANIFEST_SCHEMA), "{err}");
        assert_eq!(SchemaV1.to_string().parse::<SchemaV1>().unwrap(), SchemaV1);
    }

    #[test]
    fn install_kinds_round_trip_through_their_codes() {
        for kind in InstallKind::ALL {
            assert_eq!(kind.as_code().parse::<InstallKind>().unwrap(), kind);
        }
        assert_eq!(
            "Managed".parse::<InstallKind>().unwrap_err().kind(),
            ValueKind::InstallKind
        );
    }
}
