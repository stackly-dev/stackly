//! The `closure` step of the spike (spec 0003, AC-7, AC-12, AC-18).
//!
//! The `dependencies` probe compares the bundled and own dependencies of
//! the inventory with the non system libraries the static audit found. For
//! a maintainer artifact the `manifest` probe then compares
//! `STACKLY-ARTIFACT.json`, `LICENSES/`, and the archive report with the
//! inventory entry. Both probes are always recorded before the step fails.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use super::plan::PlanArtifact;
use super::result::{ArchiveReport, Probe, StepStatus};

/// The build record at the root of a maintainer artifact.
pub const MANIFEST_FILE: &str = "STACKLY-ARTIFACT.json";
/// The `schema` value of that record.
pub const MANIFEST_SCHEMA: &str = "stackly.runtime-artifact/v1";
/// The folder of license texts in a maintainer artifact.
const LICENSES_DIR: &str = "LICENSES";
/// The largest `STACKLY-ARTIFACT.json` the probe reads: 1 MiB.
const MANIFEST_MAX_SIZE: u64 = 1024 * 1024;

/// The probes of the closure step: `dependencies`, then `manifest` for a
/// maintainer artifact. `archive` is the report of the extracted artifact.
pub fn probes(
    artifact: &PlanArtifact,
    root: &Path,
    non_system: &BTreeSet<String>,
    archive: Option<&ArchiveReport>,
) -> Vec<Probe> {
    let mut out = vec![dependencies(artifact, non_system)];
    if artifact.origin == "maintainer" {
        out.push(manifest(artifact, root, archive));
    }
    out
}

/// The reason the closure step fails, naming every failed probe, or `None`
/// when they all passed.
pub fn failure(probes: &[Probe]) -> Option<String> {
    let reasons: Vec<String> = probes
        .iter()
        .filter(|probe| probe.status == StepStatus::Fail)
        .map(|probe| match probe.name.as_str() {
            "dependencies" => format!(
                "the inventory dependencies differ from the audit: {}",
                probe.detail
            ),
            "manifest" => format!("the manifest does not match the entry: {}", probe.detail),
            other => format!("probe {other} failed: {}", probe.detail),
        })
        .collect();
    (!reasons.is_empty()).then(|| reasons.join("; "))
}

/// AC-7: the bundled and own dependencies of the inventory are exactly the
/// non system libraries the static audit found.
fn dependencies(artifact: &PlanArtifact, non_system: &BTreeSet<String>) -> Probe {
    let listed: BTreeSet<String> = artifact
        .dependencies
        .iter()
        .filter(|dependency| matches!(dependency.kind.as_str(), "bundled" | "own"))
        .map(|dependency| dependency.name.clone())
        .collect();
    let missing: Vec<&String> = non_system.difference(&listed).collect();
    let extra: Vec<&String> = listed.difference(non_system).collect();
    if missing.is_empty() && extra.is_empty() {
        probe(
            "dependencies",
            StepStatus::Pass,
            format!("{} non system libraries, as listed", listed.len()),
        )
    } else {
        probe(
            "dependencies",
            StepStatus::Fail,
            format!("not in the inventory: {missing:?}; listed but not found: {extra:?}"),
        )
    }
}

/// AC-12 and AC-18: `STACKLY-ARTIFACT.json` matches the entry, every
/// license text is in `LICENSES/`, and the archive holds only regular files
/// and folders. Every mismatch names its field or file in the detail.
fn manifest(artifact: &PlanArtifact, root: &Path, archive: Option<&ArchiveReport>) -> Probe {
    let mut problems = Vec::new();
    let mut licenses = 0;
    match read_manifest(root) {
        Ok(manifest) => {
            compare_fields(artifact, &manifest, &mut problems);
            let dependencies = list(&manifest, "dependencies", &mut problems);
            let vendored = list(&manifest, "vendored", &mut problems);
            compare_bundled(artifact, dependencies, &mut problems);
            licenses = check_licenses(root, artifact, dependencies, vendored, &mut problems);
        }
        Err(problem) => problems.push(problem),
    }
    check_archive(archive, &mut problems);
    if problems.is_empty() {
        probe(
            "manifest",
            StepStatus::Pass,
            format!(
                "{MANIFEST_FILE} matches the entry, {licenses} license texts, only regular files and folders"
            ),
        )
    } else {
        probe("manifest", StepStatus::Fail, problems.join("; "))
    }
}

fn probe(name: &str, status: StepStatus, detail: String) -> Probe {
    Probe {
        name: name.to_owned(),
        status,
        detail,
    }
}

/// `STACKLY-ARTIFACT.json` at the artifact root, which must be a JSON
/// object.
fn read_manifest(root: &Path) -> Result<Value, String> {
    let path = root.join(MANIFEST_FILE);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(format!("{MANIFEST_FILE} is missing"));
        }
        Err(err) => return Err(format!("cannot read {MANIFEST_FILE}: {err}")),
    };
    if !meta.is_file() {
        return Err(format!("{MANIFEST_FILE} is not a regular file"));
    }
    if meta.len() > MANIFEST_MAX_SIZE {
        return Err(format!(
            "{MANIFEST_FILE} is larger than {MANIFEST_MAX_SIZE} bytes"
        ));
    }
    let bytes = fs::read(&path).map_err(|err| format!("cannot read {MANIFEST_FILE}: {err}"))?;
    match serde_json::from_slice(&bytes) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(_) => Err(format!("{MANIFEST_FILE} is not a JSON object")),
        Err(err) => Err(format!("{MANIFEST_FILE} is not JSON: {err}")),
    }
}

/// `schema`, the identity fields, the source, the recipe, and the
/// deployment target. A value missing on either side is a mismatch.
fn compare_fields(artifact: &PlanArtifact, manifest: &Value, problems: &mut Vec<String>) {
    let text = |pointer: &str| manifest.pointer(pointer).and_then(Value::as_str);
    if text("/schema") != Some(MANIFEST_SCHEMA) {
        problems.push(format!(
            "schema is {}, not \"{MANIFEST_SCHEMA}\"",
            shown(text("/schema"))
        ));
    }
    let pairs = [
        ("provider", "/provider", Some(artifact.provider.as_str())),
        ("version", "/version", Some(artifact.version.as_str())),
        ("source.url", "/source/url", artifact.source_url.as_deref()),
        (
            "source.sha256",
            "/source/sha256",
            artifact.source_sha256.as_deref(),
        ),
        ("recipe", "/recipe", artifact.recipe.as_deref()),
    ];
    for (field, pointer, expected) in pairs {
        same(field, text(pointer), expected, "", problems);
    }
    same(
        "deploymentTarget",
        text("/deploymentTarget"),
        artifact.min_os.as_deref(),
        " min_os",
        problems,
    );
    let build = manifest.get("build").and_then(Value::as_i64);
    if build.is_none() || build != artifact.build {
        problems.push(format!(
            "build is {}, the entry says {}",
            build.map_or_else(|| "missing".to_owned(), |build| build.to_string()),
            artifact
                .build
                .map_or_else(|| "nothing".to_owned(), |build| build.to_string())
        ));
    }
}

/// Records a mismatch of `field` unless `found` is present and equal to
/// `expected`. `entry_field` names the entry field when it differs, such as
/// ` min_os`.
fn same(
    field: &str,
    found: Option<&str>,
    expected: Option<&str>,
    entry_field: &str,
    problems: &mut Vec<String>,
) {
    if found.is_none() || found != expected {
        problems.push(format!(
            "{field} is {}, the entry{entry_field} says {}",
            shown(found),
            expected.map_or_else(|| "nothing".to_owned(), |value| format!("\"{value}\""))
        ));
    }
}

fn shown(value: Option<&str>) -> String {
    value.map_or_else(|| "missing".to_owned(), |value| format!("\"{value}\""))
}

/// The array `field` of the manifest; anything else is a problem.
fn list<'a>(manifest: &'a Value, field: &str, problems: &mut Vec<String>) -> &'a [Value] {
    match manifest.get(field) {
        Some(Value::Array(items)) => items,
        _ => {
            problems.push(format!("{field} is not a list"));
            &[]
        }
    }
}

/// The library file names of one manifest dependency, when they are a list
/// of strings.
fn libraries(dependency: &Value) -> Option<Vec<&str>> {
    dependency
        .get("libraries")?
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect()
}

/// The union of the manifest's `libraries` is exactly the set of `bundled`
/// names of the entry, and each bundled row has the version, license, and
/// source URL of the manifest dependency that owns its file.
fn compare_bundled(artifact: &PlanArtifact, dependencies: &[Value], problems: &mut Vec<String>) {
    let mut owners: Vec<(&str, &Value)> = Vec::new();
    for (index, dependency) in dependencies.iter().enumerate() {
        match libraries(dependency) {
            Some(files) => owners.extend(files.into_iter().map(|file| (file, dependency))),
            None => problems.push(format!(
                "dependencies[{index}].libraries is not a list of file names"
            )),
        }
    }
    let shipped: BTreeSet<&str> = owners.iter().map(|(file, _)| *file).collect();
    let bundled: BTreeSet<&str> = artifact
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == "bundled")
        .map(|dependency| dependency.name.as_str())
        .collect();
    let unlisted: Vec<&&str> = shipped.difference(&bundled).collect();
    let unshipped: Vec<&&str> = bundled.difference(&shipped).collect();
    if !unlisted.is_empty() || !unshipped.is_empty() {
        problems.push(format!(
            "dependencies[].libraries differ from the bundled rows: not bundled in the entry {unlisted:?}; not in the manifest {unshipped:?}"
        ));
    }
    for row in artifact
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == "bundled")
    {
        let found: Vec<&Value> = owners
            .iter()
            .filter(|(file, _)| *file == row.name)
            .map(|(_, owner)| *owner)
            .collect();
        let owner = match found.as_slice() {
            [owner] => *owner,
            [] => continue,
            _ => {
                problems.push(format!(
                    "{} is in the libraries of more than one manifest dependency",
                    row.name
                ));
                continue;
            }
        };
        let owner_name = owner.get("name").and_then(Value::as_str).unwrap_or("?");
        for (field, key, expected) in [
            ("version", "version", row.version.as_deref()),
            ("license", "license", row.license.as_deref()),
            ("source_url", "sourceUrl", row.source_url.as_deref()),
        ] {
            let found = owner.get(key).and_then(Value::as_str);
            if found.is_none() || found != expected {
                problems.push(format!(
                    "bundled {} {field} is {}, the manifest dependency {owner_name} says {}",
                    row.name,
                    expected.map_or_else(|| "missing".to_owned(), |value| format!("\"{value}\"")),
                    shown(found)
                ));
            }
        }
    }
}

/// `LICENSES/<provider>.txt`, then one text per manifest dependency and
/// vendored component; returns how many texts the artifact must carry.
fn check_licenses(
    root: &Path,
    artifact: &PlanArtifact,
    dependencies: &[Value],
    vendored: &[Value],
    problems: &mut Vec<String>,
) -> usize {
    let mut names = vec![("provider".to_owned(), Some(artifact.provider.as_str()))];
    for (field, items) in [("dependencies", dependencies), ("vendored", vendored)] {
        for (index, item) in items.iter().enumerate() {
            names.push((
                format!("{field}[{index}].name"),
                item.get("name").and_then(Value::as_str),
            ));
        }
    }
    for (field, name) in &names {
        match name {
            Some(name) if is_plain_name(name) => {
                let relative = format!("{LICENSES_DIR}/{name}.txt");
                if !fs::symlink_metadata(root.join(&relative)).is_ok_and(|meta| meta.is_file()) {
                    problems.push(format!("{relative} is missing"));
                }
            }
            Some(name) => problems.push(format!("{field} `{name}` is not a plain file name")),
            None => problems.push(format!("{field} is missing")),
        }
    }
    names.len()
}

/// True for one non empty path part that is not `.` or `..`.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.contains(char::is_control)
}

/// A maintainer archive has no links, no setuid or setgid files, and no
/// special files (AC-12).
fn check_archive(archive: Option<&ArchiveReport>, problems: &mut Vec<String>) {
    let Some(archive) = archive else {
        problems.push("the archive report is missing".to_owned());
        return;
    };
    if archive.symlinks_internal > 0 {
        problems.push(format!(
            "archive symlinksInternal is {}, not 0",
            archive.symlinks_internal
        ));
    }
    for (field, paths) in [
        ("symlinksEscaping", &archive.symlinks_escaping),
        ("setuid", &archive.setuid),
        ("specialFiles", &archive.special_files),
    ] {
        if !paths.is_empty() {
            problems.push(format!("archive {field} lists {}", paths.join(", ")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::spike::plan::PlanDependency;
    use serde_json::json;

    const RECIPE: &str =
        "stackly-dev/runtimes@0123456789abcdef0123456789abcdef01234567:recipes/postgresql.sh";
    const SOURCE_URL: &str = "https://ftp.postgresql.org/pub/source/v18.6/postgresql-18.6.tar.bz2";
    const OPENSSL_URL: &str =
        "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz";
    const ZSTD_URL: &str =
        "https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.gz";

    fn dependency(name: &str, kind: &str, owner: Option<(&str, &str, &str)>) -> PlanDependency {
        PlanDependency {
            name: name.to_owned(),
            kind: kind.to_owned(),
            install_name: format!("@rpath/{name}"),
            version: owner.map(|(version, _, _)| version.to_owned()),
            license: owner.map(|(_, license, _)| license.to_owned()),
            source_url: owner.map(|(_, _, url)| url.to_owned()),
        }
    }

    /// A PostgreSQL 18 maintainer entry with OpenSSL and zstd bundled.
    fn maintainer() -> PlanArtifact {
        let openssl = Some(("3.5.9", "Apache-2.0", OPENSSL_URL));
        PlanArtifact {
            id: "postgresql-18.6-r1-macos-aarch64".to_owned(),
            provider: "postgresql".to_owned(),
            line: "18".to_owned(),
            version: "18.6".to_owned(),
            origin: "maintainer".to_owned(),
            format: "tar_gz".to_owned(),
            archive_root: Some("postgresql-18.6-r1-macos-aarch64".to_owned()),
            executables: vec!["bin/postgres".to_owned()],
            signing_policy: "adhoc_pipeline".to_owned(),
            sha256: "a".repeat(64),
            size: 1,
            urls: Vec::new(),
            dependencies: vec![
                dependency("libssl.3.dylib", "bundled", openssl),
                dependency("libcrypto.3.dylib", "bundled", openssl),
                dependency(
                    "libzstd.1.dylib",
                    "bundled",
                    Some(("1.5.7", "BSD-3-Clause", ZSTD_URL)),
                ),
                dependency("libpq.5.dylib", "own", None),
                PlanDependency {
                    name: "libz.1.dylib".to_owned(),
                    kind: "system".to_owned(),
                    install_name: "/usr/lib/libz.1.dylib".to_owned(),
                    version: None,
                    license: None,
                    source_url: None,
                },
            ],
            build: Some(1),
            source_url: Some(SOURCE_URL.to_owned()),
            source_sha256: Some("c".repeat(64)),
            recipe: Some(RECIPE.to_owned()),
            min_os: Some("27.0".to_owned()),
        }
    }

    /// The `STACKLY-ARTIFACT.json` `build.sh` writes for [`maintainer`].
    fn manifest_json() -> Value {
        json!({
            "schema": MANIFEST_SCHEMA,
            "provider": "postgresql",
            "version": "18.6",
            "build": 1,
            "source": { "url": SOURCE_URL, "sha256": "c".repeat(64) },
            "recipe": RECIPE,
            "toolchain": { "xcode": "Xcode 27.0", "meson": "1.11.2" },
            "deploymentTarget": "27.0",
            "dependencies": [
                {
                    "name": "openssl",
                    "version": "3.5.9",
                    "license": "Apache-2.0",
                    "sourceUrl": OPENSSL_URL,
                    "sourceSha256": "d".repeat(64),
                    "libraries": ["libssl.3.dylib", "libcrypto.3.dylib"]
                },
                {
                    "name": "zstd",
                    "version": "1.5.7",
                    "license": "BSD-3-Clause",
                    "sourceUrl": ZSTD_URL,
                    "sourceSha256": "e".repeat(64),
                    "libraries": ["libzstd.1.dylib"]
                }
            ],
            "vendored": [],
            "builtAt": "2026-10-18T09:00:00Z"
        })
    }

    /// An artifact root holding `manifest` and the license texts it needs.
    fn artifact_root(manifest: &Value) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(MANIFEST_FILE),
            serde_json::to_vec_pretty(manifest).unwrap(),
        )
        .unwrap();
        fs::create_dir(dir.path().join(LICENSES_DIR)).unwrap();
        for name in ["postgresql", "openssl", "zstd"] {
            fs::write(
                dir.path().join(LICENSES_DIR).join(format!("{name}.txt")),
                "license text",
            )
            .unwrap();
        }
        dir
    }

    /// The non system libraries the static audit of [`maintainer`] finds.
    fn audited() -> BTreeSet<String> {
        [
            "libssl.3.dylib",
            "libcrypto.3.dylib",
            "libzstd.1.dylib",
            "libpq.5.dylib",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    /// The `manifest` probe of [`maintainer`] against `manifest`.
    fn manifest_probe(manifest: &Value) -> Probe {
        let root = artifact_root(manifest);
        manifest_probe_in(root.path(), &maintainer(), &ArchiveReport::default())
    }

    fn manifest_probe_in(root: &Path, artifact: &PlanArtifact, archive: &ArchiveReport) -> Probe {
        let probes = probes(artifact, root, &audited(), Some(archive));
        assert_eq!(probes.len(), 2, "{probes:#?}");
        assert_eq!(probes[0].name, "dependencies");
        probes[1].clone()
    }

    fn assert_fails_with(probe: &Probe, expected: &str) {
        assert_eq!(probe.name, "manifest");
        assert_eq!(probe.status, StepStatus::Fail, "{probe:#?}");
        assert!(
            probe.detail.contains(expected),
            "`{}` lacks `{expected}`",
            probe.detail
        );
    }

    #[test]
    fn a_matching_manifest_passes() {
        let probe = manifest_probe(&manifest_json());
        assert_eq!(probe.status, StepStatus::Pass, "{probe:#?}");
        assert_eq!(
            probe.detail,
            "STACKLY-ARTIFACT.json matches the entry, 3 license texts, only regular files and folders"
        );
    }

    #[test]
    fn each_field_that_differs_from_the_entry_is_named() {
        for (pointer, value, expected) in [
            (
                "/provider",
                json!("valkey"),
                "provider is \"valkey\", the entry says \"postgresql\"",
            ),
            (
                "/version",
                json!("18.5"),
                "version is \"18.5\", the entry says \"18.6\"",
            ),
            ("/build", json!(2), "build is 2, the entry says 1"),
            ("/build", json!("1"), "build is missing, the entry says 1"),
            (
                "/source/url",
                json!("https://example.org/postgresql-18.6.tar.bz2"),
                "source.url is \"https://example.org/postgresql-18.6.tar.bz2\"",
            ),
            (
                "/source/sha256",
                json!("f".repeat(64)),
                "source.sha256 is \"ffff",
            ),
            (
                "/recipe",
                json!(
                    "stackly-dev/runtimes@1111111111111111111111111111111111111111:recipes/postgresql.sh"
                ),
                "recipe is \"stackly-dev/runtimes@1111",
            ),
            (
                "/deploymentTarget",
                json!("26.0"),
                "deploymentTarget is \"26.0\", the entry min_os says \"27.0\"",
            ),
            (
                "/schema",
                json!("stackly.runtime-artifact/v2"),
                "schema is \"stackly.runtime-artifact/v2\", not \"stackly.runtime-artifact/v1\"",
            ),
        ] {
            let mut manifest = manifest_json();
            *manifest.pointer_mut(pointer).unwrap() = value;
            assert_fails_with(&manifest_probe(&manifest), expected);
        }
    }

    #[test]
    fn a_missing_field_is_named() {
        let mut manifest = manifest_json();
        manifest.as_object_mut().unwrap().remove("recipe");
        assert_fails_with(
            &manifest_probe(&manifest),
            &format!("recipe is missing, the entry says \"{RECIPE}\""),
        );
    }

    #[test]
    fn a_missing_or_unreadable_manifest_fails() {
        let root = artifact_root(&manifest_json());
        fs::remove_file(root.path().join(MANIFEST_FILE)).unwrap();
        let probe = manifest_probe_in(root.path(), &maintainer(), &ArchiveReport::default());
        assert_fails_with(&probe, "STACKLY-ARTIFACT.json is missing");

        fs::write(root.path().join(MANIFEST_FILE), "not json").unwrap();
        let probe = manifest_probe_in(root.path(), &maintainer(), &ArchiveReport::default());
        assert_fails_with(&probe, "STACKLY-ARTIFACT.json is not JSON");

        fs::write(root.path().join(MANIFEST_FILE), "[]").unwrap();
        let probe = manifest_probe_in(root.path(), &maintainer(), &ArchiveReport::default());
        assert_fails_with(&probe, "STACKLY-ARTIFACT.json is not a JSON object");
    }

    #[test]
    fn a_missing_license_text_is_named() {
        for name in ["postgresql", "openssl", "zstd"] {
            let root = artifact_root(&manifest_json());
            fs::remove_file(root.path().join(LICENSES_DIR).join(format!("{name}.txt"))).unwrap();
            let probe = manifest_probe_in(root.path(), &maintainer(), &ArchiveReport::default());
            assert_fails_with(&probe, &format!("LICENSES/{name}.txt is missing"));
        }
    }

    #[test]
    fn every_vendored_component_needs_its_license_text() {
        let mut manifest = manifest_json();
        manifest["vendored"] = json!([{ "name": "lua", "license": "MIT" }]);
        assert_fails_with(&manifest_probe(&manifest), "LICENSES/lua.txt is missing");

        manifest["vendored"] = json!([{ "name": "../../etc/passwd", "license": "MIT" }]);
        assert_fails_with(
            &manifest_probe(&manifest),
            "vendored[0].name `../../etc/passwd` is not a plain file name",
        );

        manifest.as_object_mut().unwrap().remove("vendored");
        assert_fails_with(&manifest_probe(&manifest), "vendored is not a list");
    }

    #[test]
    fn the_bundled_rows_must_be_exactly_the_manifest_libraries() {
        let mut manifest = manifest_json();
        manifest["dependencies"][1]["libraries"] = json!(["libzstd.1.dylib", "liblz4.1.dylib"]);
        assert_fails_with(
            &manifest_probe(&manifest),
            "dependencies[].libraries differ from the bundled rows: not bundled in the entry [\"liblz4.1.dylib\"]; not in the manifest []",
        );

        let mut manifest = manifest_json();
        manifest["dependencies"][1]["libraries"] = json!([]);
        assert_fails_with(
            &manifest_probe(&manifest),
            "not in the manifest [\"libzstd.1.dylib\"]",
        );
    }

    #[test]
    fn a_bundled_row_carries_the_version_license_and_source_of_its_owner() {
        for (key, value, expected) in [
            (
                "version",
                json!("3.5.8"),
                "bundled libssl.3.dylib version is \"3.5.9\", the manifest dependency openssl says \"3.5.8\"",
            ),
            (
                "license",
                json!("OpenSSL"),
                "bundled libssl.3.dylib license is \"Apache-2.0\"",
            ),
            (
                "sourceUrl",
                json!("https://example.org/openssl.tar.gz"),
                "bundled libcrypto.3.dylib source_url is",
            ),
        ] {
            let mut manifest = manifest_json();
            manifest["dependencies"][0][key] = value;
            assert_fails_with(&manifest_probe(&manifest), expected);
        }
    }

    #[test]
    fn an_archive_with_a_link_setuid_or_special_file_fails() {
        let root = artifact_root(&manifest_json());
        let cases = [
            (
                ArchiveReport {
                    symlinks_internal: 1,
                    ..ArchiveReport::default()
                },
                "archive symlinksInternal is 1, not 0",
            ),
            (
                ArchiveReport {
                    symlinks_escaping: vec!["lib/x -> /opt/x".to_owned()],
                    ..ArchiveReport::default()
                },
                "archive symlinksEscaping lists lib/x -> /opt/x",
            ),
            (
                ArchiveReport {
                    setuid: vec!["bin/postgres".to_owned()],
                    ..ArchiveReport::default()
                },
                "archive setuid lists bin/postgres",
            ),
            (
                ArchiveReport {
                    special_files: vec!["run/fifo".to_owned()],
                    ..ArchiveReport::default()
                },
                "archive specialFiles lists run/fifo",
            ),
        ];
        for (report, expected) in cases {
            let probe = manifest_probe_in(root.path(), &maintainer(), &report);
            assert_fails_with(&probe, expected);
        }
        let probes = probes(&maintainer(), root.path(), &audited(), None);
        assert_fails_with(&probes[1], "the archive report is missing");
    }

    #[test]
    fn an_upstream_artifact_has_no_manifest_probe() {
        let mut upstream = maintainer();
        upstream.origin = "upstream".to_owned();
        let root = tempfile::tempdir().unwrap();
        let probes = probes(&upstream, root.path(), &audited(), None);
        assert_eq!(
            probes.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["dependencies"]
        );
        assert_eq!(failure(&probes), None);
    }

    #[test]
    fn both_probes_are_recorded_before_the_step_fails() {
        let mut manifest = manifest_json();
        manifest["provider"] = json!("valkey");
        let root = artifact_root(&manifest);
        let mut found = audited();
        found.insert("libicuuc.78.dylib".to_owned());
        let probes = probes(
            &maintainer(),
            root.path(),
            &found,
            Some(&ArchiveReport::default()),
        );
        assert_eq!(
            probes
                .iter()
                .map(|p| (p.name.as_str(), p.status))
                .collect::<Vec<_>>(),
            [
                ("dependencies", StepStatus::Fail),
                ("manifest", StepStatus::Fail)
            ]
        );
        let reason = failure(&probes).unwrap();
        assert!(
            reason.starts_with(
                "the inventory dependencies differ from the audit: not in the inventory: [\"libicuuc.78.dylib\"]"
            ),
            "{reason}"
        );
        assert!(
            reason.contains("; the manifest does not match the entry: provider is \"valkey\""),
            "{reason}"
        );
    }
}
