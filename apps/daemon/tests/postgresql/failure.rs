//! An init that fails (spec 0019 AC-5, the failure half).

use std::time::{Duration, Instant};

use stackly_protocol::instance::{InstanceLifecycle, PortRequest};
use stackly_protocol::operation::{AbandonOperationParams, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_params, new_password};

async fn init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_never_starts(
    pg: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    // The create refuses a folder that is not empty only from spec 0020 on,
    // so the test puts the file in the data folder once the layout stage made
    // it, before initdb runs. // provisional: F20
    let instances = site.prepared.layout().instances_dir();
    let stray = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(entries) = std::fs::read_dir(&instances) {
                for entry in entries.flatten() {
                    let data = entry.path().join("data");
                    // The layout stage makes the data folder; the file goes in
                    // right after, before the init stage reaches initdb.
                    let made = if data.is_dir() {
                        std::fs::write(data.join("stray.txt"), b"keep me")
                    } else {
                        Err(std::io::ErrorKind::NotFound.into())
                    };
                    if made.is_ok() {
                        return Some(data);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    });
    let password = new_password();
    let accepted = api
        .create(create_params(
            &site,
            "broken",
            PortRequest::Auto,
            &password,
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
        .wait_state(accepted.operation.id, OperationState::NeedsRecovery)
        .await;
    let data = stray
        .join()
        .expect("the watcher ends")
        .expect("the watcher found the instance folder");

    let recovery = detail.operation.recovery.as_ref().expect("a recovery");
    println!("{recovery:#?}");
    assert!(
        recovery
            .error
            .as_ref()
            .is_some_and(|error| error.code() == "supervisor.init_failed"),
        "{recovery:?}"
    );
    let failed = api.get(id).await.expect("the instance stays registered");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);
    assert_eq!(failed.initialized, Some(false));
    let failure = format!("{:?}", failed.last_failure);
    assert!(failure.contains("init_step_failed"), "{failure}");
    assert_eq!(
        std::fs::read(data.join("stray.txt")).expect("the stray file is still there"),
        b"keep me"
    );
    assert!(
        !site.run_dir(id).join("secrets/superuser-password").exists(),
        "the password file outlived the failed init"
    );
    let refused = api
        .start(id)
        .await
        .expect_err("a start of a failed instance is refused");
    assert_eq!(refused.code(), "supervisor.not_initialized", "{refused:?}");

    let reply = api
        .client
        .call(Call::OperationAbandon(AbandonOperationParams {
            operation_id: accepted.operation.id,
            expected_revision: detail.operation.revision,
        }))
        .await
        .expect("a needs_recovery operation can be abandoned");
    let Reply::OperationAbandon(abandoned) = reply else {
        panic!("not an abandon reply");
    };
    assert_eq!(abandoned.operation.state, OperationState::Failed);
    daemon.shutdown().await;
    // The version of the runtime is the one that refused the folder.
    ev.subject(
        "postgresql",
        pg.version,
        Some(&site.artifact_sha256),
        stackly_testkit::evidence::Source::Managed,
    );
    ev.pass_with(
        "initdb refused the folder, the instance stayed failed, the operation was abandoned",
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_never_starts()
{
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009"],
        kind = Kind::Native,
        expected = "a data folder that holds a file makes initdb exit 1, the init fails with init_step_failed, the file is untouched, no password file is left, the instance stays failed and cannot start, and the create operation waits in needs_recovery until it is abandoned",
    );
    init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_never_starts(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_never_starts()
{
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035", "BR-009"],
        kind = Kind::Native,
        expected = "a data folder that holds a file makes initdb exit 1, the init fails with init_step_failed, the file is untouched, no password file is left, the instance stays failed and cannot start, and the create operation waits in needs_recovery until it is abandoned",
    );
    init_that_finds_a_file_in_the_data_folder_fails_at_initdb_and_never_starts(pg, ev).await;
}
