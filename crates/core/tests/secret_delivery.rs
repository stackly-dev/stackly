//! Secret delivery on `FakePlatform` (spec 0011, AC-12 and AC-20): the one
//! time `SecretFile`, the removal and sweep of `secrets/` folders, and the
//! argv guard. Every deletion goes through `remove_owned_tree`, so a
//! symlink is removed as itself and its target outside the folder stays.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use stackly_core::secret::ResolvedSecrets;
use stackly_core::secret::delivery::{
    DeliveryError, SecretFile, SecretInArgvReason, counts_as_running, ensure_not_in_argv,
    remove_secret_material, sweep_secret_material,
};
use stackly_core::{CoreError, PreparedLayout, prepare_layout};
use stackly_domain::{InstanceId, InstanceLifecycle, ShortKey};
use stackly_evidence::{Kind, evidence};
use stackly_platform::{Platform, SecretValue};
use stackly_testkit::{FakePlatform, FsOp, TestRoot};

/// A fixture value, never a real secret.
const FIXTURE: &str = "stackly-fixture-credential-0123456789abcdef";

/// A fake platform on a fresh test root, with its layout prepared.
fn prepared() -> (TestRoot, FakePlatform, Platform, PreparedLayout) {
    let root = TestRoot::new().expect("a test root can be created");
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let prepared = prepare_layout(&platform).expect("a fresh test root prepares");
    (root, fake, platform, prepared)
}

fn value(text: &str) -> SecretValue {
    SecretValue::new(text.to_owned()).expect("a valid test value")
}

fn key() -> ShortKey {
    InstanceId::generate().short_key()
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("the path exists")
        .permissions()
        .mode()
        & 0o777
}

/// Leaves a file in an instance's `secrets/` folder, as a crash or a
/// running instance's plaintext config would.
fn leave_material(platform: &Platform, prepared: &PreparedLayout, key: ShortKey, name: &str) {
    let file = SecretFile::create(platform, prepared, key, name, &value(FIXTURE))
        .expect("the secret file is written");
    std::mem::forget(file);
}

#[test]
fn a_secret_file_is_0600_in_a_0700_folder_and_is_gone_when_its_guard_drops() {
    let (_root, _fake, platform, prepared) = prepared();
    let key = key();
    let secrets_dir = prepared.layout().instance_secrets_dir(key);

    let file = SecretFile::create(&platform, &prepared, key, "admin-password", &value(FIXTURE))
        .expect("the secret file is written");
    let path = file.path().to_path_buf();

    assert_eq!(path, secrets_dir.join("admin-password"));
    assert_eq!(fs::read_to_string(&path).expect("readable"), FIXTURE);
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&secrets_dir), 0o700);
    assert_eq!(mode(&prepared.layout().instance_run_dir(key)), 0o700);
    assert!(!format!("{file:?}").contains(FIXTURE));

    drop(file);

    assert!(!path.exists());
    assert!(secrets_dir.is_dir(), "only the file goes, not its folder");
}

#[test]
fn a_secret_file_is_removed_when_the_step_that_reads_it_panics() {
    let (_root, _fake, platform, prepared) = prepared();
    let key = key();
    let path = prepared.layout().instance_secrets_dir(key).join("pwfile");

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let file = SecretFile::create(&platform, &prepared, key, "pwfile", &value(FIXTURE))
            .expect("the secret file is written");
        assert!(file.path().is_file());
        panic!("the step that reads the file fails");
    }));

    assert!(outcome.is_err());
    assert!(!path.exists());
}

#[test]
fn an_explicit_remove_reports_the_removed_file_and_replaces_an_old_one() {
    let (_root, _fake, platform, prepared) = prepared();
    let key = key();
    leave_material(&platform, &prepared, key, "token");

    let file = SecretFile::create(&platform, &prepared, key, "token", &value("second-value"))
        .expect("an old file of the same name is replaced");
    assert_eq!(
        fs::read_to_string(file.path()).expect("readable"),
        "second-value"
    );
    let path = file.path().to_path_buf();
    let report = file.remove().expect("the file is removed");

    assert_eq!((report.files, report.dirs, report.missing), (1, 0, false));
    assert!(!path.exists());
}

#[test]
fn a_secret_file_name_is_one_literal_component_and_a_bad_one_writes_nothing() {
    let (_root, fake, platform, prepared) = prepared();
    let key = key();

    for name in [
        "",
        ".",
        "..",
        "../escape",
        "a/b",
        "sp ace",
        "x".repeat(65).as_str(),
    ] {
        let err = SecretFile::create(&platform, &prepared, key, name, &value(FIXTURE))
            .expect_err("the name is rejected");
        assert_eq!(
            err,
            DeliveryError::InvalidFileName {
                name: name.to_owned()
            }
        );
        assert!(!err.to_string().contains(FIXTURE));
    }
    for name in ["token", "admin-password", "Fake.JSON", "a_b-c.d"] {
        let file = SecretFile::create(&platform, &prepared, key, name, &value(FIXTURE))
            .unwrap_or_else(|err| panic!("{name} is accepted: {err}"));
        drop(file);
    }
    assert!(
        fake.fs()
            .mutations()
            .iter()
            .filter(|mutation| mutation.op == FsOp::WriteFileAtomic)
            .all(|mutation| mutation
                .path
                .starts_with(prepared.layout().instance_secrets_dir(key))),
        "a rejected name never writes"
    );
}

#[test]
fn removing_secret_material_takes_the_whole_folder_and_leaves_the_run_folder() {
    let (_root, _fake, platform, prepared) = prepared();
    let key = key();
    leave_material(&platform, &prepared, key, "fake.json");
    leave_material(&platform, &prepared, key, "token");
    let run = prepared.layout().instance_run_dir(key);
    fs::write(run.join("server.pid"), "42").expect("a run file");

    let report = remove_secret_material(&platform, &prepared, key).expect("removed");

    assert_eq!((report.files, report.dirs, report.missing), (2, 1, false));
    assert!(!prepared.layout().instance_secrets_dir(key).exists());
    assert!(run.join("server.pid").is_file());
    let again = remove_secret_material(&platform, &prepared, key).expect("removing twice is fine");
    assert!(again.missing);
}

#[test]
fn a_symlink_in_the_secrets_folder_is_removed_as_itself_and_its_target_stays() {
    let ev = evidence!(
        covers = ["BR-018"],
        kind = Kind::Fixture,
        expected = "removing an instance's secrets folder on FakePlatform goes through remove_owned_tree only: a symlink inside it is removed as itself, its target outside the folder keeps its bytes, and a secrets folder that is itself a symlink out of the roots is refused without touching the target",
    );
    let (root, fake, platform, prepared) = prepared();
    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).expect("an outside folder");
    fs::write(outside.join("keep.txt"), "keep").expect("an outside file");

    let key = key();
    leave_material(&platform, &prepared, key, "token");
    let secrets = prepared.layout().instance_secrets_dir(key);
    symlink(outside.join("keep.txt"), secrets.join("link")).expect("a symlink");
    symlink(&outside, secrets.join("dir-link")).expect("a folder symlink");

    let report = remove_secret_material(&platform, &prepared, key).expect("removed");

    assert_eq!((report.files, report.dirs), (3, 1));
    assert!(!secrets.exists());
    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).expect("the target stays"),
        "keep"
    );

    let other = InstanceId::generate().short_key();
    let run = prepared.layout().instance_run_dir(other);
    fs::create_dir_all(&run).expect("a run folder");
    symlink(&outside, run.join("secrets")).expect("a secrets folder that escapes");

    let err = remove_secret_material(&platform, &prepared, other).expect_err("refused");

    assert!(
        matches!(err, CoreError::OutsideOwnedRoots { .. }),
        "{err:?}"
    );
    assert!(outside.join("keep.txt").is_file());
    let removals: Vec<_> = fake
        .fs()
        .mutations()
        .into_iter()
        .filter(|mutation| mutation.op == FsOp::RemoveTree)
        .map(|mutation| mutation.path)
        .collect();
    assert_eq!(removals.len(), 1, "{removals:?}");
    assert!(
        removals[0].ends_with(format!("i/{key}/secrets")),
        "{removals:?}"
    );
    ev.pass();
}

#[test]
fn the_sweep_removes_secrets_of_instances_that_are_not_running_and_keeps_the_rest() {
    let (root, _fake, platform, prepared) = prepared();
    let layout = prepared.layout();
    let running = key();
    let stopped = key();
    let unknown = key();
    let orphan = key();
    for key in [running, stopped, unknown, orphan] {
        leave_material(&platform, &prepared, key, "token");
    }
    let runs = layout.instance_runs_dir();
    fs::create_dir_all(runs.join("not-a-key/secrets")).expect("a stray folder");
    fs::write(runs.join("0000abcd"), "a file named like a key").expect("a stray file");
    let outside = root.path().join("outside");
    fs::create_dir_all(outside.join("secrets")).expect("an outside folder");
    let linked = key();
    symlink(&outside, runs.join(linked.to_string())).expect("a run folder symlink");

    let report = sweep_secret_material(
        &platform,
        &prepared,
        [
            (running, InstanceLifecycle::Running),
            (stopped, InstanceLifecycle::Stopped),
            (unknown, InstanceLifecycle::Unknown),
        ],
    )
    .expect("the sweep runs");

    let mut swept = vec![stopped, orphan];
    swept.sort();
    let mut kept = vec![running, unknown];
    kept.sort();
    assert_eq!(report.swept, swept);
    assert_eq!(report.kept, kept);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    for key in [running, unknown] {
        assert!(layout.instance_secrets_dir(key).join("token").is_file());
    }
    for key in [stopped, orphan] {
        assert!(!layout.instance_secrets_dir(key).exists());
        assert!(layout.instance_run_dir(key).is_dir());
    }
    assert!(runs.join("not-a-key/secrets").is_dir());
    assert!(runs.join("0000abcd").is_file());
    assert!(outside.join("secrets").is_dir());
}

#[test]
fn a_sweep_without_run_folders_finds_nothing() {
    let (_root, _fake, platform, prepared) = prepared();
    fs::remove_dir_all(prepared.layout().instance_runs_dir()).ok();

    let report = sweep_secret_material(&platform, &prepared, []).expect("the sweep runs");

    assert!(report.swept.is_empty() && report.kept.is_empty() && report.failed.is_empty());
}

#[test]
fn only_live_lifecycles_count_as_running() {
    let running: Vec<_> = InstanceLifecycle::ALL
        .iter()
        .copied()
        .filter(|lifecycle| counts_as_running(*lifecycle))
        .collect();

    assert_eq!(
        running,
        [
            InstanceLifecycle::Starting,
            InstanceLifecycle::Running,
            InstanceLifecycle::Stopping,
            InstanceLifecycle::Recovering,
            InstanceLifecycle::Unknown,
        ]
    );
}

#[test]
fn argv_never_carries_a_secret_value() {
    let ev = evidence!(
        covers = ["BR-018"],
        kind = Kind::Fixture,
        expected = "ensure_not_in_argv rejects an argument equal to any secret value (4 bytes included), one ending in =<value>, and one holding a value of 16 bytes or more, names the argument and the field but never the value, and accepts a path that holds an 8 byte value inside it",
    );
    let long = "0123456789abcdef0123456789abcdef";
    let holds_long = format!("token-{long}-tail");
    let secrets = ResolvedSecrets::for_tests([
        ("pin".parse().expect("a field name"), value("8642")),
        (
            "adminPassword".parse().expect("a field name"),
            value("postgres"),
        ),
        ("apiToken".parse().expect("a field name"), value(long)),
    ]);
    let rejected = [
        (
            vec!["--port", "8642"],
            1,
            "pin",
            SecretInArgvReason::EqualsValue,
        ),
        (
            vec!["--password=postgres"],
            0,
            "adminPassword",
            SecretInArgvReason::AssignsValue,
        ),
        (
            vec!["-x", "-x", "=8642"],
            2,
            "pin",
            SecretInArgvReason::AssignsValue,
        ),
        (
            vec![holds_long.as_str()],
            0,
            "apiToken",
            SecretInArgvReason::ContainsValue,
        ),
    ];

    for (args, index, field, reason) in rejected {
        let err = ensure_not_in_argv(&args, &secrets).expect_err("the argument is rejected");
        assert_eq!(
            (err.index, err.field.as_str(), err.reason),
            (index, field, reason)
        );
        for secret in ["8642", "postgres", long] {
            assert!(!err.to_string().contains(secret), "{err}");
        }
    }
    let accepted = [
        "/data/postgres/base",
        "--user=postgres-admin",
        "--pin=86420",
        "--data=/Volumes/T/data",
    ];
    ensure_not_in_argv(&accepted, &secrets).expect("these arguments carry no secret");
    assert_eq!(SecretInArgvReason::AssignsValue.as_code(), "assigns_value");
    ev.pass();
}
