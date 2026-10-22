//! An init whose readiness role step fails (spec 0019 AC-5): the single user
//! step exits 1, the init fails at that step, no password file is left, and
//! the instance never starts (BR-008).

use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Evidence, Kind, Source, evidence};

use crate::failure::{ROLE_STEP, failed_create};
use crate::format::copy_tree;
use crate::harness::{PG17, PG18, Pg, Site, package_dir};
use crate::phases::nothing_was_spawned;

/// A copy of the package whose role script creates `postgres`, a role that
/// `initdb` already made, so the single user step fails on an SQL error.
fn copy_with_a_failing_role_script(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("postgresql");
    copy_tree(&package_dir(), &dir);
    let manifest = dir.join("provider.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest can be read");
    let changed = text.replace(
        "CREATE ROLE stackly_readiness WITH LOGIN",
        "CREATE ROLE postgres WITH LOGIN",
    );
    assert_ne!(
        text, changed,
        "the manifest holds the CREATE ROLE statement"
    );
    std::fs::write(&manifest, changed).expect("the manifest can be written");
    dir
}

async fn init_whose_role_step_fails_is_failed_and_the_instance_never_starts(pg: Pg, ev: Evidence) {
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_with_a_failing_role_script(&copies);
    let Some(site) = Site::open_package(&ev, pg, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    let (id, detail) = failed_create(&site, &api, "norole", &crate::harness::new_password()).await;

    let error = &detail
        .operation
        .failure
        .as_ref()
        .expect("a failed operation names its error")
        .error;
    assert_eq!(error.code(), "supervisor.init_failed", "{error:?}");
    assert_eq!(
        error.detail("reason"),
        Some("init_step_failed"),
        "{error:?}"
    );
    assert_eq!(error.detail("step"), Some(ROLE_STEP), "{error:?}");
    assert_eq!(error.detail("exitCode"), Some("1"), "{error:?}");
    assert!(
        !site.run_dir(id).join("secrets/superuser-password").exists(),
        "the password file outlived the failed init"
    );
    nothing_was_spawned(&site.instance_paths(id).data);
    let gone = api
        .get(id)
        .await
        .expect_err("the instance of a failed create is gone");
    assert_eq!(gone.code(), "resource.not_found", "{gone:?}");
    daemon.shutdown().await;
    ev.subject(
        "postgresql",
        pg.version,
        Some(&site.artifact_sha256),
        Source::Managed,
    );
    ev.pass_with("the role step exited 1, the init failed at it, and nothing was started");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_init_whose_role_step_fails_is_failed_and_the_instance_never_starts() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "a role script that postgres --single rejects makes that step exit 1, the init ends init_step_failed at the step of the role script, no password file is left, no postmaster runs, and the create is compensated",
    );
    init_whose_role_step_fails_is_failed_and_the_instance_never_starts(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_init_whose_role_step_fails_is_failed_and_the_instance_never_starts() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "a role script that postgres --single rejects makes that step exit 1, the init ends init_step_failed at the step of the role script, no password file is left, no postmaster runs, and the create is compensated",
    );
    init_whose_role_step_fails_is_failed_and_the_instance_never_starts(pg, ev).await;
}
