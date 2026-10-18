use std::path::{Path, PathBuf};

use serde_json::Value;
use stackly_domain::{INSTANCE_MANIFEST_SCHEMA, InstanceId, InstanceManifest};
use stackly_platform::{Filesystem, InstancePaths, Platform, PlatformError};

use crate::error::{CoreError, ManifestError};
use crate::layout::PreparedLayout;

/// The largest `instance-manifest.json` that is written or read, 64 KiB.
pub const INSTANCE_MANIFEST_MAX_LEN: u64 = 64 * 1024;

/// Writes the manifest of an instance to `paths.manifest` atomically, as
/// pretty JSON with a trailing newline.
///
/// The manifest is written as given. Before a rewrite, call
/// [`InstanceManifest::record_write`] with the platform clock and
/// [`daemon_info`](crate::daemon_info)'s version, so `updatedAt` and
/// `stacklyVersion` stay true.
///
/// # Errors
///
/// Returns [`ManifestError::TooLarge`] (through [`CoreError::Manifest`])
/// when the encoded manifest is over [`INSTANCE_MANIFEST_MAX_LEN`], and
/// [`CoreError::Platform`] when the write fails; the old file is then
/// intact.
pub fn write_instance_manifest(
    platform: &Platform,
    paths: &InstancePaths,
    manifest: &InstanceManifest,
) -> Result<(), CoreError> {
    let bytes = encode_manifest(manifest)?;
    platform.fs().write_file_atomic(&paths.manifest, &bytes)?;
    Ok(())
}

/// Reads and decodes the manifest at `paths.manifest`.
///
/// The `schema` field is checked first, and a manifest of another schema is
/// not decoded any further. Unknown fields are ignored. The file is never
/// followed through a symlink, and a FIFO or device is never read.
///
/// # Errors
///
/// Returns [`ManifestError::UnsupportedSchema`] or
/// [`ManifestError::Malformed`] (through [`CoreError::Manifest`]), and
/// [`CoreError::Platform`] when the file is missing, not a regular file
/// ([`PlatformError::NotARegularFile`]), over
/// [`INSTANCE_MANIFEST_MAX_LEN`] ([`PlatformError::FileTooLarge`]), or
/// cannot be read.
pub fn read_instance_manifest(
    platform: &Platform,
    paths: &InstancePaths,
) -> Result<InstanceManifest, CoreError> {
    let bytes = platform
        .fs()
        .read_file_limited(&paths.manifest, INSTANCE_MANIFEST_MAX_LEN)?;
    Ok(decode_manifest(&bytes)?)
}

/// Encodes a manifest as pretty JSON plus a trailing newline, within the
/// size limit.
pub(crate) fn encode_manifest(manifest: &InstanceManifest) -> Result<Vec<u8>, ManifestError> {
    let mut bytes = serde_json::to_vec_pretty(manifest)
        .expect("an instance manifest always encodes: its fields are text, numbers, and objects");
    bytes.push(b'\n');
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > INSTANCE_MANIFEST_MAX_LEN {
        return Err(ManifestError::TooLarge {
            len: bytes.len(),
            max: INSTANCE_MANIFEST_MAX_LEN,
        });
    }
    Ok(bytes)
}

/// Decodes a manifest: the `schema` field first, then, only for v1, the
/// whole manifest.
pub(crate) fn decode_manifest(bytes: &[u8]) -> Result<InstanceManifest, ManifestError> {
    let malformed = |err: serde_json::Error| ManifestError::Malformed {
        reason: err.to_string(),
    };
    let value: Value = serde_json::from_slice(bytes).map_err(malformed)?;
    match value.get("schema") {
        Some(Value::String(schema)) if schema == INSTANCE_MANIFEST_SCHEMA => {}
        Some(Value::String(schema)) => {
            return Err(ManifestError::UnsupportedSchema {
                schema: schema.clone(),
            });
        }
        _ => {
            return Err(ManifestError::Malformed {
                reason: "the schema field is missing or not a string".to_owned(),
            });
        }
    }
    serde_json::from_value(value).map_err(malformed)
}

/// Creates the folder of a new instance under `instances/<id>/`, with its
/// manifest, and returns its paths.
///
/// The steps run in order: refuse a short key that an instance in `known`
/// already uses, refuse a manifest that is not new (one with
/// `data.initializedAt` or a retained marker), refuse an existing
/// `instances/<id>` or `retained/<id>` (BR-008), create the instance
/// folder, write `instance-manifest.json` atomically, then create `data/`,
/// `config/`, `config-history/`, `temp/`, and `logs/instances/<id>/`. Every
/// folder is created new with mode 0700, never replaced. The run folder
/// under the runtime root is not created here: the supervisor calls
/// [`ensure_instance_run_dir`](crate::ensure_instance_run_dir) before each
/// start, because the OS may empty the runtime root.
///
/// Nothing is ever deleted. When a step fails after something was created,
/// the error lists exactly the paths this call created (BR-023).
///
/// # Errors
///
/// Returns [`CoreError::ShortKeyTaken`] (the caller creates a new ID),
/// [`ManifestError::NotNew`] or [`ManifestError::TooLarge`] through
/// [`CoreError::Manifest`], [`CoreError::AlreadyExists`] for an existing
/// instance folder, [`CoreError::Partial`] when a step fails after the
/// instance folder was created, and [`CoreError::Platform`] when a port
/// fails before anything was created.
pub fn create_instance_layout(
    platform: &Platform,
    prepared: &PreparedLayout,
    manifest: &InstanceManifest,
    known: &[InstanceId],
) -> Result<InstancePaths, CoreError> {
    let fs = platform.fs();
    let layout = prepared.layout();
    let id = manifest.instance_id();
    let key = id.short_key();
    if known.iter().any(|other| other.short_key() == key) {
        return Err(CoreError::ShortKeyTaken { key });
    }
    if manifest.data().initialized_at().is_some() {
        return Err(ManifestError::NotNew {
            reason: "data.initializedAt is set",
        }
        .into());
    }
    if manifest.retained().is_some() {
        return Err(ManifestError::NotNew {
            reason: "the retained marker is set",
        }
        .into());
    }
    let bytes = encode_manifest(manifest)?;

    let paths = layout.instance_paths(id);
    for existing in [&paths.root, &layout.retained_instance_paths(id).root] {
        if fs.symlink_metadata(existing)?.is_some() {
            return Err(CoreError::AlreadyExists {
                path: existing.clone(),
            });
        }
    }

    let mut creation = Creation {
        fs,
        created: Vec::new(),
    };
    creation.dir(&paths.root)?;
    creation.file(&paths.manifest, &bytes)?;
    for dir in [
        &paths.data,
        &paths.config,
        &paths.config_history,
        &paths.temp,
        &paths.logs,
    ] {
        creation.dir(dir)?;
    }
    Ok(paths)
}

/// The paths one create call made so far, in order.
struct Creation<'a> {
    fs: &'a dyn Filesystem,
    created: Vec<PathBuf>,
}

impl Creation<'_> {
    fn dir(&mut self, path: &Path) -> Result<(), CoreError> {
        let result = self.fs.create_dir_private(path);
        self.record(path, result)
    }

    fn file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
        let result = self.fs.write_file_atomic(path, bytes);
        self.record(path, result)
    }

    /// Notes a created path, or turns a failure into the error that says
    /// what this call created.
    fn record(&mut self, path: &Path, result: Result<(), PlatformError>) -> Result<(), CoreError> {
        match result {
            Ok(()) => {
                self.created.push(path.to_path_buf());
                Ok(())
            }
            // Nothing created yet: the instance folder itself failed, and an
            // existing one means another create got there first (BR-008).
            Err(PlatformError::AlreadyExists { path }) if self.created.is_empty() => {
                Err(CoreError::AlreadyExists { path })
            }
            Err(source) if self.created.is_empty() => Err(CoreError::Platform(source)),
            Err(source) => Err(CoreError::Partial {
                created: std::mem::take(&mut self.created),
                source,
            }),
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use stackly_domain::{RetainedMarker, Rule};
    use stackly_testkit::evidence::{Kind, evidence};
    use stackly_testkit::{Fault, FsOp, Mutation, TestRoot};

    use super::*;
    use crate::test_support::{manifest_for, prepared, same_short_key, temp_files};

    const V1: &str = include_str!("../../domain/tests/fixtures/instance-manifest-v1.json");
    const V1_UNKNOWN_FIELDS: &str =
        include_str!("../../domain/tests/fixtures/instance-manifest-v1-unknown-fields.json");
    const V2: &str = include_str!("../../domain/tests/fixtures/instance-manifest-v2.json");

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    #[test]
    fn create_writes_the_manifest_before_the_other_folders() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let before = fake.fs().mutations().len();

        let paths = create_instance_layout(&platform, &prepared, &manifest, &[]).unwrap();

        assert_eq!(
            paths,
            prepared.layout().instance_paths(manifest.instance_id())
        );
        let created: Vec<(FsOp, PathBuf)> = fake.fs().mutations()[before..]
            .iter()
            .map(|Mutation { op, path, .. }| (*op, path.clone()))
            .collect();
        let dir = |path: &PathBuf| (FsOp::CreateDirPrivate, path.clone());
        assert_eq!(
            created,
            vec![
                dir(&paths.root),
                (FsOp::WriteFileAtomic, paths.manifest.clone()),
                dir(&paths.data),
                dir(&paths.config),
                dir(&paths.config_history),
                dir(&paths.temp),
                dir(&paths.logs),
            ]
        );
        for dir in [
            &paths.root,
            &paths.data,
            &paths.config,
            &paths.config_history,
            &paths.temp,
            &paths.logs,
        ] {
            assert_eq!(mode(dir), 0o700, "{}", dir.display());
        }
        assert_eq!(mode(&paths.manifest), 0o600);
        assert!(!paths.run.exists());
        let written = read_instance_manifest(&platform, &paths).unwrap();
        assert_eq!(written, manifest);
        assert_eq!(written.data().initialized_at(), None);
        assert_eq!(fake.fs().violations(), 0);
    }

    #[test]
    fn create_refuses_an_existing_instance_folder() {
        let ev = evidence!(
            covers = ["BR-008"],
            kind = Kind::Fixture,
            expected = "an existing instances/<id> or retained/<id> folder, even a symlink, is refused with AlreadyExists (BR-008) before anything is written",
        );
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let layout = prepared.layout();
        let active = manifest_for(&platform, InstanceId::generate());
        let retained = manifest_for(&platform, InstanceId::generate());
        let linked = manifest_for(&platform, InstanceId::generate());
        std::fs::create_dir(layout.instance_paths(active.instance_id()).root).unwrap();
        std::fs::create_dir(layout.retained_instance_paths(retained.instance_id()).root).unwrap();
        std::os::unix::fs::symlink(
            root.path(),
            layout.instance_paths(linked.instance_id()).root,
        )
        .unwrap();
        let before = fake.fs().mutations();

        for (manifest, existing) in [
            (&active, layout.instance_paths(active.instance_id()).root),
            (
                &retained,
                layout.retained_instance_paths(retained.instance_id()).root,
            ),
            (&linked, layout.instance_paths(linked.instance_id()).root),
        ] {
            let result = create_instance_layout(&platform, &prepared, manifest, &[]);

            let err = result.unwrap_err();
            assert_eq!(err, CoreError::AlreadyExists { path: existing });
            assert_eq!(err.rule(), Some(Rule::Br008));
        }
        assert_eq!(fake.fs().mutations(), before);
        assert!(!layout.instance_paths(retained.instance_id()).root.exists());
        ev.pass();
    }

    #[test]
    fn create_refuses_a_short_key_a_known_instance_uses() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let other = same_short_key(manifest.instance_id());
        let before = fake.fs().mutations();

        for known in [
            vec![InstanceId::generate(), other],
            vec![manifest.instance_id()],
        ] {
            let result = create_instance_layout(&platform, &prepared, &manifest, &known);

            assert_eq!(
                result,
                Err(CoreError::ShortKeyTaken {
                    key: manifest.instance_id().short_key()
                })
            );
        }
        assert_eq!(fake.fs().mutations(), before);
        let unrelated = [InstanceId::generate(), InstanceId::generate()];
        create_instance_layout(&platform, &prepared, &manifest, &unrelated).unwrap();
    }

    #[test]
    fn failed_create_reports_only_its_own_paths_and_deletes_nothing() {
        let ev = evidence!(
            covers = ["BR-023"],
            kind = Kind::Fixture,
            expected = "a create that fails at config/ returns Partial with exactly the instance folder, its manifest, and data/, deletes nothing, and leaves other instances intact",
        );
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let neighbour = manifest_for(&platform, InstanceId::generate());
        let neighbour_paths =
            create_instance_layout(&platform, &prepared, &neighbour, &[]).unwrap();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let paths = prepared.layout().instance_paths(manifest.instance_id());
        fake.fs()
            .fail_when(FsOp::CreateDirPrivate, "config", Fault::NoSpace);

        let result =
            create_instance_layout(&platform, &prepared, &manifest, &[neighbour.instance_id()]);

        let err = result.unwrap_err();
        assert_eq!(
            err,
            CoreError::Partial {
                created: vec![
                    paths.root.clone(),
                    paths.manifest.clone(),
                    paths.data.clone()
                ],
                source: PlatformError::NoSpace {
                    path: paths.config.clone()
                },
            }
        );
        assert_eq!(err.rule(), Some(Rule::Br023));
        for kept in [&paths.root, &paths.manifest, &paths.data] {
            assert!(kept.exists(), "{}", kept.display());
        }
        assert!(!paths.config.exists());
        assert_eq!(read_instance_manifest(&platform, &paths).unwrap(), manifest);
        assert_eq!(
            read_instance_manifest(&platform, &neighbour_paths).unwrap(),
            neighbour
        );
        assert!(neighbour_paths.logs.is_dir());
        assert!(temp_files(&paths.root).is_empty());
        ev.pass();
    }

    #[test]
    fn a_failed_manifest_write_reports_only_the_instance_folder() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let paths = prepared.layout().instance_paths(manifest.instance_id());
        fake.fs().fail_next(FsOp::WriteFileAtomic, Fault::Io);

        let result = create_instance_layout(&platform, &prepared, &manifest, &[]);

        assert!(
            matches!(
                &result,
                Err(CoreError::Partial { created, source: PlatformError::Io { .. } })
                    if created == &vec![paths.root.clone()]
            ),
            "{result:?}"
        );
        assert!(!paths.manifest.exists());
        assert!(temp_files(&paths.root).is_empty());
    }

    #[test]
    fn a_create_that_fails_before_creating_anything_reports_the_platform_error() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let before = fake.fs().mutations();
        fake.fs()
            .fail_next(FsOp::CreateDirPrivate, Fault::PermissionDenied);

        let result = create_instance_layout(&platform, &prepared, &manifest, &[]);

        assert!(
            matches!(
                result,
                Err(CoreError::Platform(PlatformError::PermissionDenied { .. }))
            ),
            "{result:?}"
        );
        assert_eq!(fake.fs().mutations(), before);
    }

    #[test]
    fn create_refuses_a_manifest_that_is_not_new() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let mut initialized = manifest_for(&platform, InstanceId::generate());
        initialized.record_initialized(platform.clock().now());
        let mut retained = manifest_for(&platform, InstanceId::generate());
        retained.record_retained(RetainedMarker::new(
            platform.clock().now(),
            stackly_domain::OperationId::generate(),
        ));
        let before = fake.fs().mutations();

        for (manifest, reason) in [
            (&initialized, "data.initializedAt is set"),
            (&retained, "the retained marker is set"),
        ] {
            let result = create_instance_layout(&platform, &prepared, manifest, &[]);

            assert_eq!(
                result,
                Err(CoreError::Manifest(ManifestError::NotNew { reason }))
            );
        }
        assert_eq!(fake.fs().mutations(), before);
    }

    #[test]
    fn manifest_is_pretty_json_with_a_trailing_newline_and_schema_first() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let paths = create_instance_layout(&platform, &prepared, &manifest, &[]).unwrap();

        let text = std::fs::read_to_string(&paths.manifest).unwrap();

        let mut expected = serde_json::to_string_pretty(&manifest).unwrap();
        expected.push('\n');
        assert_eq!(text, expected);
        assert!(
            text.starts_with("{\n  \"schema\": \"stackly.instance-manifest/v1\",\n"),
            "{text}"
        );
    }

    #[test]
    fn a_rewritten_manifest_reads_back_with_its_new_fields() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let mut manifest = manifest_for(&platform, InstanceId::generate());
        let paths = create_instance_layout(&platform, &prepared, &manifest, &[]).unwrap();
        fake.clock().set("2026-01-01T00:00:05Z".parse().unwrap());

        manifest.record_initialized(platform.clock().now());
        manifest.record_write(
            platform.clock().now(),
            crate::daemon_info().version.to_owned(),
        );
        write_instance_manifest(&platform, &paths, &manifest).unwrap();

        let read = read_instance_manifest(&platform, &paths).unwrap();
        assert_eq!(read, manifest);
        assert_eq!(read.data().initialized_at(), Some(platform.clock().now()));
        assert_eq!(read.updated_at(), platform.clock().now());
        assert_ne!(read.updated_at(), read.created_at());
        assert!(temp_files(&paths.root).is_empty());
    }

    #[test]
    fn a_failed_rewrite_keeps_the_old_manifest_and_leaves_no_temporary_file() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let paths = create_instance_layout(&platform, &prepared, &manifest, &[]).unwrap();
        let mut renamed = manifest.clone();
        renamed.record_initialized(platform.clock().now());
        fake.fs().fail_next(FsOp::WriteFileAtomic, Fault::NoSpace);

        let result = write_instance_manifest(&platform, &paths, &renamed);

        assert!(
            matches!(
                result,
                Err(CoreError::Platform(PlatformError::NoSpace { .. }))
            ),
            "{result:?}"
        );
        assert_eq!(read_instance_manifest(&platform, &paths).unwrap(), manifest);
        assert!(temp_files(&paths.root).is_empty());
    }

    #[test]
    fn a_manifest_over_64_kib_is_neither_written_nor_read() {
        let root = TestRoot::new().unwrap();
        let (fake, prepared) = prepared(&root);
        let platform = fake.platform();
        let manifest = manifest_for(&platform, InstanceId::generate());
        let paths = create_instance_layout(&platform, &prepared, &manifest, &[]).unwrap();
        let mut huge = manifest.clone();
        huge.record_write(platform.clock().now(), "9".repeat(64 * 1024));
        let before = fake.fs().mutations();

        let written = write_instance_manifest(&platform, &paths, &huge);

        assert!(
            matches!(
                written,
                Err(CoreError::Manifest(ManifestError::TooLarge { max, .. }))
                    if max == INSTANCE_MANIFEST_MAX_LEN
            ),
            "{written:?}"
        );
        assert_eq!(fake.fs().mutations(), before);
        let mut padded = std::fs::read(&paths.manifest).unwrap();
        padded.resize(64 * 1024 + 1, b' ');
        std::fs::write(&paths.manifest, &padded).unwrap();
        let read = read_instance_manifest(&platform, &paths);
        assert!(
            matches!(
                read,
                Err(CoreError::Platform(PlatformError::FileTooLarge { max, .. }))
                    if max == INSTANCE_MANIFEST_MAX_LEN
            ),
            "{read:?}"
        );
        padded.truncate(64 * 1024);
        std::fs::write(&paths.manifest, &padded).unwrap();
        assert_eq!(read_instance_manifest(&platform, &paths).unwrap(), manifest);
    }

    #[test]
    fn decode_reads_the_shared_v1_fixtures() {
        let full = decode_manifest(V1.as_bytes()).unwrap();
        let extra = decode_manifest(V1_UNKNOWN_FIELDS.as_bytes()).unwrap();

        assert_eq!(
            full.instance_id().to_string(),
            "0199f2a4-6b1e-7c3d-9e4f-5a6b7c8d9e0f"
        );
        assert!(full.retained().is_some());
        assert_eq!(extra.name().as_str(), "cache");
        assert_eq!(extra.runtime().artifact_sha256(), None);
        let reencoded = encode_manifest(&full).unwrap();
        assert_eq!(decode_manifest(&reencoded).unwrap(), full);
    }

    #[test]
    fn decode_stops_at_another_schema_before_decoding_the_rest() {
        let result = decode_manifest(V2.as_bytes());

        assert_eq!(
            result,
            Err(ManifestError::UnsupportedSchema {
                schema: "stackly.instance-manifest/v2".to_owned()
            })
        );
    }

    #[test]
    fn decode_reports_bytes_that_are_not_a_v1_manifest_as_malformed() {
        let mut no_name: Value = serde_json::from_str(V1).unwrap();
        no_name.as_object_mut().unwrap().remove("name").unwrap();
        let no_name = serde_json::to_vec(&no_name).unwrap();

        for bytes in [
            &b"{ not json"[..],
            b"[]",
            b"{\"schema\": 1}",
            b"{\"name\": \"orders-db\"}",
            &no_name,
        ] {
            let result = decode_manifest(bytes);

            assert!(
                matches!(result, Err(ManifestError::Malformed { .. })),
                "{}: {result:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }
}
