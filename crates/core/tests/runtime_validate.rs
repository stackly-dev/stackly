//! `runtime::validate` and `runtime::launch` on a staged tree (spec 0018,
//! AC-10 to AC-12): architecture, macOS minimum, closure, signing policy,
//! quarantine, and the one run of the version check, over `FakePlatform`.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use stackly_core::prepare_layout;
use stackly_core::provider::{NameCheck, PackageFiles, SigningPolicy, ValidatedPackage};
use stackly_core::runtime::archive::{ExtractReport, ExtractSpec, extract};
use stackly_core::runtime::samples::{
    MachOBuilder, MemoryFile, TarBuilder, sample_limits, universal,
};
use stackly_core::runtime::validate::{
    Marker, ValidateError, ValidateInput, ValidateReport, VersionCheckSpec, finish_tree, validate,
};
use stackly_domain::{ArtifactFormat, ExactVersion, OsVersion};
use stackly_platform::{
    CapturedOutput, ChildRole, EntryPath, ExitStatus, FileKind, Platform, ProcessError, StopFlag,
};
use stackly_testkit::{FakePlatform, ProcessOp, SigningCall, TestRoot};

const VERSION: &str = "2.15.1";

fn entry(path: &str) -> EntryPath {
    EntryPath::new(path).expect("a valid entry path")
}

/// `providers/fake` with a version check added, taken out of its package.
fn version_check(pattern: &str, timeout_seconds: u64) -> VersionCheckSpec {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../providers/fake");
    let mut files = BTreeMap::new();
    for path in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        files.insert(
            path.to_owned(),
            std::fs::read(dir.join(path)).expect("package file"),
        );
    }
    for item in std::fs::read_dir(dir.join("templates")).expect("templates") {
        let item = item.expect("entry");
        let name = item.file_name().into_string().expect("name");
        files.insert(
            format!("templates/{name}"),
            std::fs::read(item.path()).expect("template"),
        );
    }
    let manifest = String::from_utf8(files["provider.yaml"].clone()).expect("utf-8");
    // The package declares its own check; this test swaps in its own.
    let own_check = "  versionCheck:\n    args: [\"version\"]\n    pattern: '^stackly-fake-server ([0-9]+\\.[0-9]+\\.[0-9]+)$'\n    timeoutSeconds: 10\n";
    assert!(
        manifest.contains(own_check),
        "the fake package declares its version check"
    );
    let patched = manifest.replace(own_check, "").replace(
        "  executionMode: foreground\n",
        &format!(
            "  executionMode: foreground\n  versionCheck:\n    args: [\"version\"]\n    pattern: '{pattern}'\n    timeoutSeconds: {timeout_seconds}\n"
        ),
    );
    files.insert("provider.yaml".to_owned(), patched.into_bytes());
    let package =
        ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::FolderName)
            .expect("the patched package validates");
    VersionCheckSpec::from_check(
        package
            .version_check()
            .expect("the manifest declares a version check"),
    )
}

struct Rig {
    _root: TestRoot,
    fake: FakePlatform,
    platform: Platform,
    tree: PathBuf,
    check_dir: PathBuf,
    report: ExtractReport,
    check: VersionCheckSpec,
}

/// Stages a tar.gz with the given files and extracts it into a real tree.
fn stage(files: &[(&str, u32, Vec<u8>)]) -> Rig {
    let root = TestRoot::new().expect("a test root");
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let prepared = prepare_layout(&platform).expect("the layout prepares");
    let scratch = prepared.layout().operations_dir().join("op-1");
    let stage_dir = scratch.join("stage");
    let tree = stage_dir.join("tree");
    for dir in [&scratch, &stage_dir, &tree] {
        platform
            .fs()
            .create_dir_private(dir)
            .expect("a private folder");
    }
    let mut tar = TarBuilder::new();
    for (name, mode, data) in files {
        tar = tar.file(name, *mode, data);
    }
    let bytes = tar.finish_gz();
    let spec = ExtractSpec::new(None, vec![entry("bin/nats-server")], sample_limits());
    let mut writer = platform
        .fs()
        .open_tree_writer(&tree)
        .expect("a tree writer");
    let report = extract(
        ArtifactFormat::TarGz,
        &mut MemoryFile::new(bytes),
        &spec,
        &mut *writer,
        &StopFlag::new(),
    )
    .expect("the tree extracts");
    let marker = Marker {
        operation_id: "op-1".to_owned(),
        sha256: "ab".repeat(32),
        software: "nats".to_owned(),
        version: VERSION.to_owned(),
        target: "macos-aarch64".to_owned(),
    };
    finish_tree(&mut *writer, &report.entries, &marker).expect("the tree is finished");
    Rig {
        check_dir: scratch.join("check"),
        _root: root,
        fake,
        platform,
        tree,
        report,
        check: version_check("^nats-server: v([0-9.]+)$", 1),
    }
}

fn nats(exec: &MachOBuilder) -> Rig {
    stage(&[("bin/nats-server", 0o755, exec.build())])
}

impl Rig {
    async fn run(
        &self,
        policy: SigningPolicy,
        expected: Option<&[String]>,
        check: Option<&VersionCheckSpec>,
    ) -> Result<ValidateReport, ValidateError> {
        let version: ExactVersion = VERSION.parse().expect("a version");
        let executables = [entry("bin/nats-server")];
        let stop = StopFlag::new();
        let input = ValidateInput {
            platform: &self.platform,
            tree: &self.tree,
            check_dir: &self.check_dir,
            entries: &self.report.entries,
            executables: &executables,
            min_os: OsVersion::major_minor(13, 0),
            signing: policy,
            maintainer: false,
            expected_libraries: expected,
            version: &version,
            version_check: check,
            stop: &stop,
        };
        validate(input, &|_, _| {}).await
    }

    /// Runs the validation and, when a check runs, ends the helper with
    /// `status` and `stdout` as soon as it is spawned.
    async fn run_with_output(
        &self,
        status: Option<ExitStatus>,
        stdout: &str,
    ) -> Result<ValidateReport, ValidateError> {
        let driver = async {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let spawned = self
                    .fake
                    .processes()
                    .spawns()
                    .into_iter()
                    .find(|spawn| spawn.role == ChildRole::RuntimeCheck);
                if let Some(spawn) = spawned {
                    self.fake.processes().exit_with_output(
                        spawn.pid,
                        status,
                        CapturedOutput {
                            stdout: stdout.as_bytes().to_vec(),
                            stderr: b"a line on stderr\n".to_vec(),
                        },
                    );
                    return;
                }
                assert!(Instant::now() < deadline, "the version check never spawned");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        let (result, ()) = tokio::join!(
            self.run(SigningPolicy::KeepOrAdhoc, Some(&[]), Some(&self.check)),
            driver
        );
        result
    }
}

fn arm64() -> MachOBuilder {
    MachOBuilder::executable().min_os(13, 0)
}

#[tokio::test]
async fn a_staged_runtime_that_passes_every_check_reports_its_signature_and_files() {
    let rig = nats(&arm64());

    let report = rig
        .run_with_output(Some(ExitStatus::Exited(0)), "nats-server: v2.15.1\n")
        .await
        .expect("the runtime validates");

    assert_eq!(report.mach_o_files, 1);
    assert!(!report.adhoc_signed);
    assert_eq!(report.executables.len(), 1);
    assert_eq!(report.executables[0].path, "bin/nats-server");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The check ran with the role of its own, an empty environment but four
    // names, and the tree as the program.
    let spawn = rig
        .fake
        .processes()
        .spawns()
        .into_iter()
        .find(|spawn| spawn.role == ChildRole::RuntimeCheck)
        .expect("a check spawn");
    assert!(spawn.program.ends_with("stage/tree/bin/nats-server"));
    assert_eq!(spawn.args, ["version"]);
    let names: Vec<&str> = spawn.env.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["PATH", "HOME", "TMPDIR", "LANG"]);
    assert!(spawn.lock.is_none());
}

#[tokio::test]
async fn an_intel_only_executable_is_refused_with_the_rosetta_diagnosis() {
    let rig = nats(&arm64().x86_64());

    let result = rig.run(SigningPolicy::KeepOrAdhoc, None, None).await;

    assert_eq!(
        result,
        Err(ValidateError::ArchitectureMismatch {
            path: "bin/nats-server".to_owned(),
            arches: vec!["x86_64".to_owned()]
        })
    );
    assert!(
        result
            .expect_err("it failed")
            .to_string()
            .contains("Rosetta")
    );
}

#[tokio::test]
async fn a_universal_executable_with_an_arm64_slice_is_accepted() {
    let both = universal(&[arm64().x86_64().build(), arm64().build()]);
    let rig = stage(&[("bin/nats-server", 0o755, both)]);

    let report = rig.run(SigningPolicy::KeepOrAdhoc, None, None).await;

    assert!(report.is_ok(), "{report:?}");
}

#[tokio::test]
async fn a_binary_that_needs_a_newer_macos_than_this_mac_is_refused() {
    let rig = nats(&MachOBuilder::executable().min_os(99, 0));

    let result = rig.run(SigningPolicy::KeepOrAdhoc, None, None).await;

    assert!(
        matches!(result, Err(ValidateError::OsTooOld { ref path, .. }) if path == "bin/nats-server"),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_library_outside_the_runtime_is_refused() {
    let rig = nats(&arm64().load("/opt/homebrew/lib/libfoo.dylib"));

    let result = rig.run(SigningPolicy::KeepOrAdhoc, None, None).await;

    assert_eq!(
        result,
        Err(ValidateError::DependencyOutsideRuntime {
            path: "bin/nats-server".to_owned(),
            library: "/opt/homebrew/lib/libfoo.dylib".to_owned()
        })
    );
}

#[tokio::test]
async fn the_libraries_found_must_equal_the_catalog_dependencies() {
    let rig = stage(&[
        (
            "bin/nats-server",
            0o755,
            arm64()
                .load("@executable_path/../lib/libssl.3.dylib")
                .build(),
        ),
        (
            "lib/libssl.3.dylib",
            0o644,
            MachOBuilder::dylib("@rpath/libssl.3.dylib").build(),
        ),
    ]);
    let listed = ["libssl.3.dylib".to_owned(), "libcrypto.3.dylib".to_owned()];

    let result = rig
        .run(SigningPolicy::KeepOrAdhoc, Some(&listed), None)
        .await;

    assert_eq!(
        result,
        Err(ValidateError::DependencyMismatch {
            missing: vec!["libcrypto.3.dylib".to_owned()],
            unexpected: Vec::new()
        })
    );
}

#[tokio::test]
async fn keep_or_adhoc_signs_every_mach_o_file_when_one_does_not_verify() {
    let rig = stage(&[
        (
            "bin/nats-server",
            0o755,
            arm64()
                .load("@executable_path/../lib/libssl.3.dylib")
                .build(),
        ),
        (
            "lib/libssl.3.dylib",
            0o644,
            MachOBuilder::dylib("@rpath/libssl.3.dylib").build(),
        ),
    ]);
    rig.fake.code_signing().set_unsigned("lib/libssl.3.dylib");

    let report = rig
        .run(SigningPolicy::KeepOrAdhoc, None, None)
        .await
        .expect("it signs ad hoc and passes");

    assert!(report.adhoc_signed);
    let signed: Vec<_> = rig
        .fake
        .code_signing()
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            SigningCall::SignAdhoc(paths) => Some(paths),
            _ => None,
        })
        .collect();
    assert_eq!(signed.len(), 1);
    assert_eq!(
        signed[0].len(),
        2,
        "every Mach-O file, not only the broken one"
    );
}

#[tokio::test]
async fn keep_or_adhoc_fails_when_a_file_still_does_not_verify_after_signing() {
    let rig = nats(&arm64());
    rig.fake.code_signing().set_unsigned("bin/nats-server");
    rig.fake.code_signing().set_unsignable("bin/nats-server");

    let result = rig.run(SigningPolicy::KeepOrAdhoc, None, None).await;

    assert!(
        matches!(
            result,
            Err(ValidateError::SignatureInvalid {
                policy: "keep_or_adhoc",
                ..
            })
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn adhoc_pipeline_never_signs_and_fails_a_file_that_does_not_verify() {
    let rig = nats(&arm64());
    rig.fake.code_signing().set_unsigned("bin/nats-server");

    let result = rig.run(SigningPolicy::AdhocPipeline, None, None).await;

    assert!(
        matches!(
            result,
            Err(ValidateError::SignatureInvalid {
                policy: "adhoc_pipeline",
                ..
            })
        ),
        "{result:?}"
    );
    assert!(
        !rig.fake
            .code_signing()
            .calls()
            .iter()
            .any(|call| matches!(call, SigningCall::SignAdhoc(_)))
    );
}

#[tokio::test]
async fn a_quarantined_executable_is_refused_without_running_anything() {
    let rig = nats(&arm64());
    rig.fake
        .code_signing()
        .set_quarantined("bin/nats-server", Some("Safari"));

    let result = rig
        .run(SigningPolicy::KeepOrAdhoc, None, Some(&rig.check))
        .await;

    assert_eq!(
        result,
        Err(ValidateError::Quarantined {
            path: "bin/nats-server".to_owned()
        })
    );
    assert!(
        rig.fake.processes().spawns().is_empty(),
        "no process was spawned"
    );
}

#[tokio::test]
async fn without_a_version_check_the_run_is_skipped_and_a_warning_says_so() {
    let rig = nats(&arm64());

    let report = rig
        .run(SigningPolicy::KeepOrAdhoc, None, None)
        .await
        .expect("the runtime validates");

    assert!(
        report
            .warnings
            .contains(&"version check not declared".to_owned())
    );
    assert_eq!(rig.fake.processes().count(ProcessOp::SpawnHelper), 0);
}

#[tokio::test]
async fn a_version_other_than_the_catalogs_fails_with_both_versions() {
    let rig = nats(&arm64());

    let result = rig
        .run_with_output(Some(ExitStatus::Exited(0)), "nats-server: v2.14.0\n")
        .await;

    assert_eq!(
        result,
        Err(ValidateError::VersionMismatch {
            expected: VERSION.to_owned(),
            observed: Some("2.14.0".to_owned())
        })
    );
}

#[tokio::test]
async fn output_the_pattern_does_not_match_is_a_version_mismatch_without_an_observed_version() {
    let rig = nats(&arm64());

    let result = rig
        .run_with_output(Some(ExitStatus::Exited(0)), "hello\n")
        .await;

    assert_eq!(
        result,
        Err(ValidateError::VersionMismatch {
            expected: VERSION.to_owned(),
            observed: None
        })
    );
}

#[tokio::test]
async fn a_version_check_that_exits_non_zero_carries_its_code_and_a_stderr_tail() {
    let rig = nats(&arm64());

    let result = rig.run_with_output(Some(ExitStatus::Exited(3)), "").await;

    match result {
        Err(ValidateError::BinaryCheckFailed {
            exit_code: Some(3),
            stderr: Some(tail),
            ..
        }) => assert_eq!(tail, "a line on stderr"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn sigkill_nobody_sent_is_a_binary_the_os_rejected_and_other_signals_are_a_failed_check() {
    let killed = nats(&arm64())
        .run_with_output(Some(ExitStatus::Signaled(9)), "")
        .await;
    let aborted = nats(&arm64())
        .run_with_output(Some(ExitStatus::Signaled(6)), "")
        .await;

    assert_eq!(
        killed,
        Err(ValidateError::BinaryRejectedByOs {
            path: "bin/nats-server".to_owned(),
            signal: 9
        })
    );
    assert!(
        matches!(
            aborted,
            Err(ValidateError::BinaryCheckFailed {
                signal: Some(6),
                ..
            })
        ),
        "{aborted:?}"
    );
}

#[tokio::test]
async fn a_check_past_its_limit_loses_its_process_group_at_once_and_ends_in_time() {
    let rig = nats(&arm64());
    let started = Instant::now();
    // The kernel ends the group once it is killed.
    let reaper = async {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let killed = rig
                .fake
                .processes()
                .calls()
                .iter()
                .any(|call| call.op == ProcessOp::TerminateHelperGroup);
            if killed {
                let spawn = rig
                    .fake
                    .processes()
                    .spawns()
                    .into_iter()
                    .find(|spawn| spawn.role == ChildRole::RuntimeCheck)
                    .expect("the check was spawned");
                rig.fake
                    .processes()
                    .exit(spawn.pid, Some(ExitStatus::Signaled(9)));
                return;
            }
            assert!(Instant::now() < deadline, "the group was never killed");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };

    let (result, ()) = tokio::join!(
        rig.run(SigningPolicy::KeepOrAdhoc, Some(&[]), Some(&rig.check)),
        reaper
    );

    assert_eq!(
        result,
        Err(ValidateError::BinaryCheckTimeout {
            path: "bin/nats-server".to_owned(),
            seconds: 1
        })
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let forced: Vec<bool> = rig
        .fake
        .processes()
        .calls()
        .into_iter()
        .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
        .filter_map(|call| call.force)
        .collect();
    assert_eq!(forced, [true], "no SIGTERM and no grace");
}

#[tokio::test]
async fn a_spawn_the_kernel_refuses_for_its_architecture_is_an_architecture_mismatch() {
    let rig = nats(&arm64());
    rig.fake.processes().fail_next(
        ProcessOp::SpawnHelper,
        ProcessError::SpawnFailed {
            kind: std::io::ErrorKind::Other,
            errno: 86,
        },
    );

    let result = rig
        .run(SigningPolicy::KeepOrAdhoc, None, Some(&rig.check))
        .await;

    assert!(
        matches!(result, Err(ValidateError::ArchitectureMismatch { .. })),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_spawn_denied_with_eacces_is_a_failed_check_with_the_errno() {
    let rig = nats(&arm64());
    rig.fake.processes().fail_next(
        ProcessOp::SpawnHelper,
        ProcessError::SpawnFailed {
            kind: std::io::ErrorKind::PermissionDenied,
            errno: 13,
        },
    );

    let result = rig
        .run(SigningPolicy::KeepOrAdhoc, None, Some(&rig.check))
        .await;

    assert!(
        matches!(
            result,
            Err(ValidateError::BinaryCheckFailed {
                errno: Some(13),
                ..
            })
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_raised_stop_flag_stops_validation() {
    let rig = nats(&arm64());
    let version: ExactVersion = VERSION.parse().expect("a version");
    let executables = [entry("bin/nats-server")];
    let stop = StopFlag::new();
    stop.stop();
    let input = ValidateInput {
        platform: &rig.platform,
        tree: &rig.tree,
        check_dir: &rig.check_dir,
        entries: &rig.report.entries,
        executables: &executables,
        min_os: OsVersion::major_minor(13, 0),
        signing: SigningPolicy::KeepOrAdhoc,
        maintainer: false,
        expected_libraries: None,
        version: &version,
        version_check: None,
        stop: &stop,
    };

    let result = validate(input, &|_, _| {}).await;

    assert_eq!(result, Err(ValidateError::Stopped));
}

#[test]
fn the_finished_tree_has_the_final_modes_and_a_marker_naming_the_operation() {
    let rig = nats(&arm64());
    let mode = |path: &str| {
        std::fs::symlink_metadata(rig.tree.join(path))
            .expect("the path exists")
            .permissions()
            .mode()
            & 0o777
    };

    assert_eq!(mode("bin/nats-server"), 0o555);
    assert_eq!(mode("bin"), 0o755);
    assert_eq!(mode(".stackly-runtime.json"), 0o444);
    let marker: serde_json::Value = serde_json::from_slice(
        &std::fs::read(rig.tree.join(".stackly-runtime.json")).expect("the marker is there"),
    )
    .expect("the marker is JSON");
    assert_eq!(marker["schema"], "stackly.runtime-marker/v1");
    assert_eq!(marker["operationId"], "op-1");
    let info = rig
        .platform
        .fs()
        .symlink_metadata(&rig.tree.join(".stackly-runtime.json"))
        .expect("readable")
        .expect("exists");
    assert_eq!(info.kind, FileKind::File);
}
