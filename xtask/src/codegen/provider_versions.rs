//! `providers/<package>/versions.json`, generated from
//! `runtimes/inventory.toml` (spec 0009, "Generator").
//!
//! The catalog never reads the inventory: this generator copies the fields
//! the catalog needs into each package, and `codegen --check` catches drift.
//! A package whose software has no inventory entry (the `testing/`
//! packages) keeps its hand written `versions.json`, untouched.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use stackly_core::provider::{VERSIONS_SCHEMA, manifest_software};
use toml::value::Datetime;

use super::{Generated, pretty_json};
use crate::runtime::compatibility::{CatalogRecord, Evidence};
use crate::runtime::inventory::{Artifact, Dependency, Inventory};

/// The provider packages, relative to the repository root.
pub const PROVIDERS_DIR: &str = "providers";
/// The manifest inside a package.
const MANIFEST_FILE: &str = "provider.yaml";
/// The generated catalog inside a package.
const VERSIONS_FILE: &str = "versions.json";

/// `versions.json`, schema `stackly.provider-versions/v1`. Field order is
/// the order the file is written in.
#[derive(Debug, Serialize)]
struct VersionsFile<'a> {
    schema: &'static str,
    software: &'a str,
    artifacts: Vec<CatalogArtifact<'a>>,
}

/// One artifact of the catalog, copied from one inventory entry. Fields
/// the inventory leaves out are not written.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogArtifact<'a> {
    id: &'a str,
    line: &'a str,
    version: &'a str,
    os: &'a str,
    arch: &'a str,
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_os: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    urls: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_root: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signing_policy: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executables: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocked: Option<Blocked<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unpacked_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dependencies: Option<Vec<CatalogDependency<'a>>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    compatibility: Vec<CatalogRecord>,
}

/// One library an available artifact links, copied from
/// `[[artifact.dependency]]`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogDependency<'a> {
    name: &'a str,
    kind: &'a str,
    install_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_url: Option<&'a str>,
}

/// Why a line has no managed artifact.
#[derive(Debug, Serialize)]
struct Blocked<'a> {
    reason: &'a str,
    owner: &'a str,
    since: String,
}

/// Every `versions.json` the inventory fills: one for each package in
/// `providers/` whose `metadata.software` has inventory entries.
pub fn generate(root: &Path) -> Result<Vec<Generated>> {
    let inventory = Inventory::load(root)?;
    let evidence = Evidence::load(root)?;
    generate_from(root, &inventory, &evidence)
}

fn generate_from(
    root: &Path,
    inventory: &Inventory,
    evidence: &Evidence,
) -> Result<Vec<Generated>> {
    let mut generated = Vec::new();
    for package in package_dirs(root)? {
        let manifest = Path::new(PROVIDERS_DIR).join(&package).join(MANIFEST_FILE);
        let text = fs::read_to_string(root.join(&manifest))
            .with_context(|| format!("cannot read {}", manifest.display()))?;
        let software = manifest_software(&text).map_err(|issue| {
            anyhow!(
                "{}: {issue}",
                Path::new(PROVIDERS_DIR).join(&package).display()
            )
        })?;
        let entries: Vec<(usize, &Artifact)> = inventory
            .artifacts
            .iter()
            .enumerate()
            .filter(|(_, artifact)| artifact.provider.as_deref() == Some(software.as_str()))
            .collect();
        if entries.is_empty() {
            continue;
        }
        let artifacts = entries
            .into_iter()
            .map(|(index, artifact)| catalog_artifact(index, artifact, evidence))
            .collect::<Result<Vec<_>>>()?;
        let file = VersionsFile {
            schema: VERSIONS_SCHEMA,
            software: software.as_str(),
            artifacts,
        };
        generated.push(Generated {
            path: Path::new(PROVIDERS_DIR).join(&package).join(VERSIONS_FILE),
            contents: pretty_json(&file)?,
        });
    }
    Ok(generated)
}

/// The package folders in `providers/`, sorted by name. Files there, such
/// as a README, are skipped.
fn package_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let dir = root.join(PROVIDERS_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut packages = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("cannot read {}", dir.display()))?;
        if entry.file_type()?.is_dir() {
            packages.push(PathBuf::from(entry.file_name()));
        }
    }
    packages.sort();
    Ok(packages)
}

/// Maps one inventory entry to a catalog artifact. Tooling only fields
/// (upstream checksum and signature, recipe, origin, distribution,
/// maintainer source) are left out; `stable`, `build`, the scan values,
/// and the dependencies are copied (spec 0009, fase 02), and an available
/// artifact gets the compatibility records of its sha256 (spec 0018).
fn catalog_artifact<'a>(
    index: usize,
    artifact: &'a Artifact,
    evidence: &Evidence,
) -> Result<CatalogArtifact<'a>> {
    let label = artifact.label(index);
    let blocked = match (
        artifact.blocked_reason.as_deref(),
        artifact.blocked_owner.as_deref(),
        artifact.blocked_since.as_ref(),
    ) {
        (None, None, None) => None,
        (Some(reason), Some(owner), Some(since)) => Some(Blocked {
            reason,
            owner,
            since: date_text(since).with_context(|| format!("{label}: blocked_since"))?,
        }),
        _ => bail!(
            "{label}: blocked_reason, blocked_owner, and blocked_since come together; run `cargo xtask runtime check`"
        ),
    };
    Ok(CatalogArtifact {
        id: required(&label, "id", &artifact.id)?,
        line: required(&label, "line", &artifact.line)?,
        version: required(&label, "version", &artifact.version)?,
        os: required(&label, "os", &artifact.os)?,
        arch: required(&label, "arch", &artifact.arch)?,
        status: required(&label, "status", &artifact.status)?,
        min_os: artifact.min_os.as_deref(),
        format: artifact.format.as_deref(),
        urls: artifact.urls.as_deref(),
        sha256: artifact.sha256.as_deref(),
        size: artifact.size,
        archive_root: artifact.archive_root.as_deref(),
        signing_policy: artifact.signing_policy.as_deref(),
        license: artifact.license.as_deref(),
        license_url: artifact.license_url.as_deref(),
        executables: artifact.executables.as_deref(),
        blocked,
        stable: artifact.stable,
        build: artifact.build,
        unpacked_size: artifact.unpacked_size,
        entry_count: artifact.entry_count,
        dependencies: catalog_dependencies(&label, artifact)?,
        compatibility: compatibility_records(&label, artifact, evidence)?,
    })
}

/// The dependencies of an available artifact, or `None` for any other
/// status, which `versions.json` refuses them on.
fn catalog_dependencies<'a>(
    label: &str,
    artifact: &'a Artifact,
) -> Result<Option<Vec<CatalogDependency<'a>>>> {
    if artifact.status.as_deref() != Some("available") || artifact.dependencies.is_empty() {
        return Ok(None);
    }
    artifact
        .dependencies
        .iter()
        .map(|dependency: &Dependency| {
            let field = |name: &str, value: &'a Option<String>| {
                required(&format!("{label}: dependency"), name, value)
            };
            Ok(CatalogDependency {
                name: field("name", &dependency.name)?,
                kind: field("kind", &dependency.kind)?,
                install_name: field("install_name", &dependency.install_name)?,
                version: dependency.version.as_deref(),
                license: dependency.license.as_deref(),
                source_url: dependency.source_url.as_deref(),
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// The compatibility records of an available artifact with a digest.
fn compatibility_records(
    label: &str,
    artifact: &Artifact,
    evidence: &Evidence,
) -> Result<Vec<CatalogRecord>> {
    match (artifact.status.as_deref(), artifact.sha256.as_deref()) {
        (Some("available"), Some(sha256)) if !sha256.is_empty() => evidence
            .records_for(sha256)
            .with_context(|| format!("{label}: compatibility records")),
        _ => Ok(Vec::new()),
    }
}

/// A field every catalog artifact carries.
fn required<'a>(label: &str, field: &str, value: &'a Option<String>) -> Result<&'a str> {
    value.as_deref().ok_or_else(|| {
        anyhow!("{label}: the inventory entry has no {field}; run `cargo xtask runtime check`")
    })
}

/// A TOML date written `YYYY-MM-DD`.
fn date_text(since: &Datetime) -> Result<String> {
    let date = since
        .date
        .ok_or_else(|| anyhow!("a time without a date is not a date"))?;
    Ok(format!(
        "{:04}-{:02}-{:02}",
        date.year, date.month, date.day
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `generate_from` with the evidence under `root`.
    fn generate_in(root: &Path, inventory: &Inventory) -> Result<Vec<Generated>> {
        generate_from(root, inventory, &Evidence::load(root)?)
    }

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask sits one directory below the repository root")
    }

    /// A repo with one package per `(folder, software)` pair.
    fn repo(packages: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (folder, software) in packages {
            let dir = root.path().join(PROVIDERS_DIR).join(folder);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join(MANIFEST_FILE),
                format!(
                    "metadata:\n  id: official/{folder}\n  software: {software}\n  displayName: X\n  revision: \"1\"\n  category: messaging\n"
                ),
            )
            .unwrap();
        }
        root
    }

    const NATS: &str = r#"
schema_version = 1

[[artifact]]
id = "nats-2.15.1-macos-aarch64"
provider = "nats"
line = "2.15"
version = "2.15.1"
build = 1
os = "macos"
arch = "aarch64"
min_os = "13.0"
status = "available"
origin = "upstream"
distribution = "upstream_download"
format = "tar_gz"
urls = ["https://example.com/nats.tar.gz"]
sha256 = "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd"
size = 6785953
archive_root = "nats-server-v2.15.1-darwin-arm64"
upstream_checksum = { kind = "sha256sums", url = "https://example.com/SHA256SUMS" }
upstream_signature = { kind = "none" }
signing_policy = "keep_or_adhoc"
license = "Apache-2.0"
license_url = "https://example.com/LICENSE"
executables = ["nats-server"]

[[artifact.dependency]]
name = "libSystem.B.dylib"
kind = "system"
install_name = "/usr/lib/libSystem.B.dylib"

[[artifact]]
id = "nats-2.14-macos-aarch64"
provider = "nats"
line = "2.14"
version = "2.14.0"
os = "macos"
arch = "aarch64"
status = "blocked"
blocked_reason = "no signed build yet"
blocked_owner = "@stackly"
blocked_since = 2026-10-01
license = "Apache-2.0"

[[artifact]]
id = "valkey-8.1.0-macos-aarch64"
provider = "valkey"
line = "8.1"
version = "8.1.0"
os = "macos"
arch = "aarch64"
status = "available"
"#;

    #[test]
    fn a_package_gets_its_inventory_entries_in_inventory_order() {
        let root = repo(&[("nats", "nats")]);
        let inventory = Inventory::parse(NATS).unwrap();

        let generated = generate_in(root.path(), &inventory).unwrap();

        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].path, Path::new("providers/nats/versions.json"));
        let expected = r#"{
  "schema": "stackly.provider-versions/v1",
  "software": "nats",
  "artifacts": [
    {
      "id": "nats-2.15.1-macos-aarch64",
      "line": "2.15",
      "version": "2.15.1",
      "os": "macos",
      "arch": "aarch64",
      "status": "available",
      "minOs": "13.0",
      "format": "tar_gz",
      "urls": [
        "https://example.com/nats.tar.gz"
      ],
      "sha256": "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd",
      "size": 6785953,
      "archiveRoot": "nats-server-v2.15.1-darwin-arm64",
      "signingPolicy": "keep_or_adhoc",
      "license": "Apache-2.0",
      "licenseUrl": "https://example.com/LICENSE",
      "executables": [
        "nats-server"
      ],
      "build": 1,
      "dependencies": [
        {
          "name": "libSystem.B.dylib",
          "kind": "system",
          "installName": "/usr/lib/libSystem.B.dylib"
        }
      ]
    },
    {
      "id": "nats-2.14-macos-aarch64",
      "line": "2.14",
      "version": "2.14.0",
      "os": "macos",
      "arch": "aarch64",
      "status": "blocked",
      "license": "Apache-2.0",
      "blocked": {
        "reason": "no signed build yet",
        "owner": "@stackly",
        "since": "2026-10-01"
      }
    }
  ]
}
"#;
        assert_eq!(generated[0].contents, expected);
    }

    #[test]
    fn a_package_without_inventory_entries_keeps_its_hand_written_catalog() {
        let root = repo(&[("fake", "fake"), ("nats", "nats")]);
        fs::write(root.path().join("providers/README.md"), "not a package\n").unwrap();
        let inventory = Inventory::parse(NATS).unwrap();

        let generated = generate_in(root.path(), &inventory).unwrap();

        let paths: Vec<_> = generated.iter().map(|file| file.path.clone()).collect();
        assert_eq!(paths, [PathBuf::from("providers/nats/versions.json")]);
    }

    #[test]
    fn a_broken_manifest_names_its_package() {
        let root = repo(&[("nats", "Nats")]);
        let inventory = Inventory::parse(NATS).unwrap();

        let err = generate_in(root.path(), &inventory)
            .unwrap_err()
            .to_string();

        assert!(
            err.starts_with("providers/nats: provider.yaml metadata.software: invalid_name"),
            "{err}"
        );
    }

    #[test]
    fn a_half_blocked_entry_is_refused() {
        let root = repo(&[("nats", "nats")]);
        let text = NATS.replace("blocked_owner = \"@stackly\"\n", "");
        let inventory = Inventory::parse(&text).unwrap();

        let err = generate_in(root.path(), &inventory)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("nats-2.14-macos-aarch64: blocked_reason"),
            "{err}"
        );
    }

    #[test]
    fn a_changed_inventory_entry_drifts_from_the_committed_catalog() {
        let real = Inventory::load(repo_root()).unwrap();
        let committed = generate_in(repo_root(), &real).unwrap();
        let mut changed = real.clone();
        let nats = changed
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.provider.as_deref() == Some("nats"))
            .expect("the inventory has a nats entry");
        nats.size = nats.size.map(|size| size + 1);

        let regenerated = generate_in(repo_root(), &changed).unwrap();

        let path = Path::new("providers/nats/versions.json");
        let find = |files: &[Generated]| {
            files
                .iter()
                .find(|file| file.path == path)
                .map(|file| file.contents.clone())
                .expect("nats has a generated catalog")
        };
        assert_eq!(
            fs::read_to_string(repo_root().join(path)).unwrap(),
            find(&committed),
            "providers/nats/versions.json drifted; run `cargo xtask codegen`"
        );
        assert_ne!(find(&regenerated), find(&committed));
    }

    #[test]
    fn an_available_artifact_gets_the_records_of_its_digest_and_the_others_get_none() {
        let root = repo(&[("nats", "nats")]);
        let digest = "ed0fe39222ec2e317c4d0e1a9cdc16c63ec523d94f9355d24a902a4db2655ffd";
        let run = root
            .path()
            .join("evidence/runtime/019a0000-0000-7000-8000-000000000001");
        let record = "019a0000-0000-7000-8000-000000000002";
        fs::create_dir_all(run.join(format!("artifacts/{record}"))).unwrap();
        fs::write(
            run.join(format!("artifacts/{record}/spike-result.json")),
            r#"{"steps":[{"name":"launch","status":"PASS"},{"name":"stop","status":"PASS"}],"closureProbes":[{"name":"dependencies","status":"PASS"}]}"#,
        )
        .unwrap();
        let host = r#"{"os":"macos","arch":"aarch64","osVersion":"27.0","osBuild":"26A428"}"#;
        fs::write(
            run.join("report.json"),
            format!(
                r#"{{"schema":"stackly.evidence-report/v1","runId":"019a0000-0000-7000-8000-000000000001","kind":"native","wrapperVersion":"0.1.0",
"context":{{"sourceCommit":"abc","sourceDirty":false,"host":{host},"toolchain":{{}}}},
"startedAt":"2026-10-17T09:00:00Z","finishedAt":"2026-10-17T09:49:41.233258Z","interrupted":false,
"summary":{{"total":1,"byStatus":{{"NOT_RUN":0,"PASS":1,"FAIL":0,"BLOCKED":0,"DEFERRED":0}},"byKind":{{"fixture":0,"native":1}}}},
"problems":[],"records":[{{"schema":"stackly.evidence/v1","id":"{record}","covers":["AC-078"],"kind":"native","method":"automated",
"test":{{"language":"rust","package":"xtask","target":"native_runtime","name":"nats_runs"}},"status":"PASS","expected":"x","actual":"nats 2.15.1 ready in 412 ms",
"subjects":[{{"provider":"nats","version":"2.15.1","digest":"{digest}","source":"managed"}}],
"startedAt":"2026-10-17T09:49:00Z","finishedAt":"2026-10-17T09:49:41.233258Z","evidencePath":"artifacts/{record}/"}}]}}"#
            ),
        )
        .unwrap();
        let inventory = Inventory::parse(NATS).unwrap();

        let generated = generate_in(root.path(), &inventory).unwrap();

        let value: serde_json::Value = serde_json::from_str(&generated[0].contents).unwrap();
        let artifacts = value["artifacts"].as_array().unwrap();
        assert_eq!(
            artifacts[0]["compatibility"],
            serde_json::json!([{
                "scope": "runtime", "result": "pass", "providerRevision": null,
                "os": "macos", "osVersion": "27.0", "osBuild": "26A428", "arch": "aarch64",
                "checks": ["dependencies", "launch_agent", "stop"],
                "knownLimitations": ["provider_integration_not_covered"],
                "summary": "nats 2.15.1 ready in 412 ms",
                "testedAt": "2026-10-17T09:49:41.233Z",
                "evidence": format!("runtime/019a0000-0000-7000-8000-000000000001/{record}")
            }])
        );
        assert!(artifacts[1].get("compatibility").is_none());
    }
}
