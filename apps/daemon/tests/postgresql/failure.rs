//! An init that fails (spec 0019 AC-5, the failure half; spec 0020 D9).

use std::sync::Arc;

use stackly_core::instance::create::Hook;
use stackly_protocol::instance::PortRequest;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_params, new_password};
use crate::support::LogRecorder;

/// Where the steps of `providers/postgresql` sit in its init plan: the
/// folders (0), the three configs (1 to 3), the password file (4), `initdb`
/// (5), and the single user step that creates the readiness role (6).
pub const INITDB_STEP: &str = "5";
pub const ROLE_STEP: &str = "6";

/// Creates an instance that is expected to fail and waits for the failed
/// operation: returns the instance it named and the operation.
pub async fn failed_create(
    site: &Site,
    api: &crate::support::Api,
    name: &str,
) -> (uuid::Uuid, stackly_protocol::operation::OperationDetail) {
    let accepted = api
        .create(create_params(
            site,
            name,
            PortRequest::Auto,
            &new_password(),
            serde_json::json!({}),
        ))
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id: uuid::Uuid = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Failed)
        .await;
    (id, detail)
}

async fn init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_is_compensated(
    pg: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    // The create refuses a folder that is not empty before init starts
    // (`instance.data_not_empty`), so the test puts the file in the data
    // folder after that check, through the hook of the create (D9).
    let stray: Hook = Arc::new(|paths| {
        std::fs::write(paths.data.join("stray.txt"), b"keep me")
            .expect("the data folder takes the file");
    });
    let daemon = site
        .in_process(|config| config.create_hooks.before_init = Some(stray))
        .await;
    let api = site.api().await;

    let (id, detail) = failed_create(&site, &api, "broken").await;

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
    assert_eq!(error.detail("step"), Some(INITDB_STEP), "{error:?}");
    assert_eq!(error.detail("exitCode"), Some("1"), "{error:?}");
    assert!(detail.operation.recovery.is_none(), "{detail:?}");
    let gone = api
        .get(id)
        .await
        .expect_err("the instance of a failed create is gone");
    assert_eq!(gone.code(), "resource.not_found", "{gone:?}");
    assert!(
        !site.instance_paths(id).root.exists(),
        "the instance folder, with the stray file, is gone"
    );
    assert!(
        !site.run_dir(id).join("secrets/superuser-password").exists(),
        "the password file outlived the failed init"
    );
    daemon.shutdown().await;
    // The version of the runtime is the one that refused the folder.
    ev.subject(
        "postgresql",
        pg.version,
        Some(&site.artifact_sha256),
        stackly_testkit::evidence::Source::Managed,
    );
    ev.pass_with("initdb refused the folder and the create was compensated in full");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_is_compensated()
 {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009"],
        kind = Kind::Native,
        expected = "a data folder that holds a file once the create checked it empty makes initdb exit 1, the init fails with init_step_failed, no password file is left, and the create ends failed with the instance row, the instance folder, and the secret reference all removed",
    );
    init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_is_compensated(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_is_compensated()
 {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009"],
        kind = Kind::Native,
        expected = "a data folder that holds a file once the create checked it empty makes initdb exit 1, the init fails with init_step_failed, no password file is left, and the create ends failed with the instance row, the instance folder, and the secret reference all removed",
    );
    init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_is_compensated(pg, ev).await;
}

/// The error that `initdb` printed when it refused the folder reaches the
/// log sinks and the failure excerpt of the instance (spec 0019 AC-5, spec
/// 0010 init step failures: "exit code and excerpt").
async fn init_that_initdb_refuses_shows_the_initdb_error_as_its_excerpt(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let stray: Hook = Arc::new(|paths| {
        std::fs::write(paths.data.join("stray.txt"), b"keep me")
            .expect("the data folder takes the file");
    });
    let recorder = Arc::new(LogRecorder::default());
    let daemon = site
        .in_process(|config| {
            config.create_hooks.before_init = Some(stray);
            config
                .extra_log_sinks
                .push(Arc::clone(&recorder) as Arc<dyn stackly_core::supervisor::LogSink>);
        })
        .await;
    let api = site.api().await;

    let (id, _) = failed_create(&site, &api, "excerpt").await;

    let written = recorder.text_of(id);
    assert!(
        written
            .iter()
            .any(|line| line.contains("exists but is not empty")),
        "initdb said nothing that reached the sinks about the folder it refused: {written:#?}"
    );
    daemon.shutdown().await;
    ev.subject(
        "postgresql",
        pg.version,
        Some(&site.artifact_sha256),
        stackly_testkit::evidence::Source::Managed,
    );
    ev.pass_with("the initdb error line reached the log sinks");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_init_that_initdb_refuses_shows_the_initdb_error_as_its_excerpt() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "the error line initdb prints for a data folder that is not empty is among the lines the log sinks received for the instance whose init failed",
    );
    init_that_initdb_refuses_shows_the_initdb_error_as_its_excerpt(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_init_that_initdb_refuses_shows_the_initdb_error_as_its_excerpt() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "the error line initdb prints for a data folder that is not empty is among the lines the log sinks received for the instance whose init failed",
    );
    init_that_initdb_refuses_shows_the_initdb_error_as_its_excerpt(pg, ev).await;
}
