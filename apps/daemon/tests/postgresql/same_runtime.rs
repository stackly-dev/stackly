//! Two PostgreSQL 17 instances of one runtime through the real daemon (spec
//! 0024, step 9, AC-7 and AC-9): `pg17-dev` made by a plain create and
//! `pg17-test` made by `instance.get_duplicate_form` and the create of its
//! draft, the same path as `stackly instance duplicate pg17-dev pg17-test
//! --secret superuserPassword=copy`. Both run from the one runtime in the
//! cache of `cargo xtask`, never from a download.
//!
//! The 18 part of AC-009 is proven by spec 0025, which starts from
//! [`two_postgresql_17_instances`].

use std::path::Path;

use stackly_protocol::instance::{
    DuplicateForm, InstanceReadinessState, InstanceSummary, SecretFieldInput,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::SecretInput;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{PG17, Site, create_instance, files_holding, stop_and_shutdown};
use crate::sentinel::SentinelCluster;
use crate::support::Api;

/// The sentinel `pg17-dev` writes.
const DEV_SENTINEL: &str = "written-to-pg17-dev";
/// The sentinel `pg17-test` writes.
const TEST_SENTINEL: &str = "written-to-pg17-test";

/// The two running clusters of [`two_postgresql_17_instances`].
#[derive(Debug)]
pub struct TwoPostgres17 {
    /// `pg17-dev`, made by a plain create, with [`DEV_SENTINEL`] written.
    pub dev: SentinelCluster,
    /// `pg17-test`, made from the duplicate form of `pg17-dev`, with
    /// [`TEST_SENTINEL`] written.
    pub test: SentinelCluster,
    /// The one runtime both instances run from.
    pub runtime_id: Uuid,
}

/// The secret inputs of a client where the user chose Copy from for every
/// secret field the source has a reference for.
fn copied(form: &DuplicateForm) -> Vec<SecretFieldInput> {
    form.secrets
        .iter()
        .map(|secret| SecretFieldInput {
            field: secret.field.clone(),
            input: SecretInput::CopyFrom(
                secret
                    .source_reference
                    .clone()
                    .expect("the source has a reference for the password"),
            ),
        })
        .collect()
}

/// Duplicates `source` as `name` with the password copied, and waits until
/// the create succeeded. Returns the ID of the duplicate.
async fn duplicate(api: &Api, source: Uuid, name: &str) -> Uuid {
    let mut form = api
        .duplicate_form(source)
        .await
        .expect("the duplicate form is built");
    assert_eq!(form.source.instance_id, source);
    form.draft.name = name.to_owned();
    let secrets = copied(&form);
    let operation = api
        .create_from_draft(form.draft, secrets)
        .await
        .expect("the create of the draft is accepted")
        .accepted
        .expect("a create starts an operation")
        .operation;
    let id = operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    let detail = api
        .wait_state(operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(detail.operation.state, OperationState::Succeeded);
    id
}

/// Starts `first` and `second` together and waits until both are `ready`.
async fn start_together(
    api: &Api,
    first: Uuid,
    second: Uuid,
) -> (InstanceSummary, InstanceSummary) {
    let mut operations = Vec::new();
    for id in [first, second] {
        operations.push(
            api.start(id)
                .await
                .expect("the start is accepted")
                .accepted
                .expect("a start from stopped begins an operation")
                .operation
                .id,
        );
    }
    for operation in operations {
        api.wait_state(operation, OperationState::Succeeded).await;
    }
    let mut summaries = Vec::new();
    for id in [first, second] {
        summaries.push(
            api.wait_instance(id, "ready", |summary| {
                summary.readiness.state == InstanceReadinessState::Ready
            })
            .await,
        );
    }
    let second = summaries.pop().expect("two summaries");
    let first = summaries.pop().expect("two summaries");
    (first, second)
}

/// Stops the instance `id` and waits until the stop succeeded.
async fn stop(api: &Api, id: Uuid) {
    let accepted = api
        .stop_when_free(id)
        .await
        .accepted
        .expect("a stop starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
}

/// The port of the `client` endpoint of a running instance.
fn port_of(summary: &InstanceSummary) -> u16 {
    summary.endpoints[0]
        .port
        .expect("a ready instance shows its port")
}

/// Every file and folder under `root` as (relative path, size, modification
/// time in nanoseconds), sorted.
fn tree(root: &Path) -> Vec<(String, u64, u128)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, u64, u128)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_nanos());
            let relative = entry
                .path()
                .strip_prefix(base)
                .expect("an entry sits under its base")
                .display()
                .to_string();
            out.push((relative, meta.len(), modified));
            if meta.is_dir() {
                walk(base, &entry.path(), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// The shared set up of the native tests of spec 0024 and spec 0025: with a
/// `site` opened for [`PG17`], creates `pg17-dev` on a free fixed port,
/// duplicates it as `pg17-test` with the password copied, starts both until
/// `ready`, and writes a different sentinel to each.
///
/// # Panics
///
/// Panics when any step fails, as the tests do.
pub async fn two_postgresql_17_instances(site: &Site, api: &Api) -> TwoPostgres17 {
    let dev = create_instance(site, api, "pg17-dev").await;
    let test_id = duplicate(api, dev.id, "pg17-test").await;
    let (dev_up, test_up) = start_together(api, dev.id, test_id).await;
    assert_eq!(
        port_of(&dev_up),
        dev.port,
        "the fixed port is the one asked"
    );
    let dev_cluster = SentinelCluster::new(
        "pg17-dev",
        dev.id,
        site.psql(dev.id, dev.port),
        &dev.password,
    );
    let test_cluster = SentinelCluster::new(
        "pg17-test",
        test_id,
        site.psql(test_id, port_of(&test_up)),
        &dev.password,
    );
    dev_cluster.write(DEV_SENTINEL);
    test_cluster.write(TEST_SENTINEL);
    TwoPostgres17 {
        dev: dev_cluster,
        test: test_cluster,
        runtime_id: dev_up.runtime_id,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn two_postgresql_17_instances_from_one_runtime_keep_their_sentinels() {
    let ev = evidence!(
        covers = ["REQ-009", "AC-009", "AC-025"],
        kind = Kind::Native,
        expected = "pg17-dev (plain create, fixed port) and pg17-test (duplicate of pg17-dev with the password copied) share one PostgreSQL 17 runtime ID, run ready at the same time on different ports, folders, sockets, and processes, each reads only its own sentinel and has its own system identifier, and the reads stay clean when each instance is stopped and started again in turn (JOURNEY-02 through the CLI path; the PostgreSQL 18 part of AC-009 is spec 0025)",
    );
    let Some(site) = Site::open(&ev, PG17) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let two = two_postgresql_17_instances(&site, &api).await;
    let version = two
        .dev
        .psql
        .scalar(&two.dev.password, "SHOW server_version");
    site.subject(&ev, &version);

    // One runtime, two instances, nothing shared.
    let dev = api.get(two.dev.id).await.expect("pg17-dev reads");
    let test = api.get(two.test.id).await.expect("pg17-test reads");
    assert_eq!(dev.runtime_id, site.runtime_id);
    assert_eq!(test.runtime_id, site.runtime_id);
    assert_eq!(two.runtime_id, site.runtime_id);
    assert_ne!(two.dev.id, two.test.id);
    assert_ne!(port_of(&dev), port_of(&test));
    let (dev_paths, test_paths) = (
        site.instance_paths(two.dev.id),
        site.instance_paths(two.test.id),
    );
    assert_ne!(dev_paths.data, test_paths.data);
    assert_ne!(dev_paths.run, test_paths.run);
    assert!(dev_paths.data.is_dir() && test_paths.data.is_dir());
    let dev_pid = dev.process.as_ref().expect("pg17-dev runs").pid;
    let test_pid = test.process.as_ref().expect("pg17-test runs").pid;
    assert_ne!(dev_pid, test_pid);
    assert!(
        dev_paths
            .run
            .join(format!(".s.PGSQL.{}", port_of(&dev)))
            .exists()
    );
    assert!(
        test_paths
            .run
            .join(format!(".s.PGSQL.{}", port_of(&test)))
            .exists()
    );

    // Each cluster reads its own sentinel and no other.
    SentinelCluster::assert_isolated(&[(&two.dev, DEV_SENTINEL), (&two.test, TEST_SENTINEL)]);
    let dev_identifier = two.dev.system_identifier();
    let test_identifier = two.test.system_identifier();
    assert_ne!(dev_identifier, test_identifier);

    // Stop and start each instance in turn; the other one is untouched while
    // it is down, and both read clean afterwards.
    for (down, up, up_value) in [
        (&two.dev, &two.test, TEST_SENTINEL),
        (&two.test, &two.dev, DEV_SENTINEL),
    ] {
        let port_before = {
            let summary = api.get(down.id).await.expect("the instance reads");
            port_of(&summary)
        };
        stop(&api, down.id).await;
        SentinelCluster::assert_isolated(&[(up, up_value)]);
        let summary = api.start_ready(down.id).await;
        assert_eq!(port_of(&summary), port_before, "the port stays the same");
        SentinelCluster::assert_isolated(&[(&two.dev, DEV_SENTINEL), (&two.test, TEST_SENTINEL)]);
        assert_eq!(two.dev.system_identifier(), dev_identifier);
        assert_eq!(two.test.system_identifier(), test_identifier);
    }

    stop(&api, two.test.id).await;
    stop_and_shutdown(&api, daemon, two.dev.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version}: pg17-dev and pg17-test ran ready together from runtime {} on ports {} and {}, each read only its own sentinel, system identifiers {dev_identifier} and {test_identifier} differ, and reads stayed clean over a stop and start of each in turn; the PostgreSQL 18 part of AC-009 is proven by spec 0025",
        site.runtime_id,
        port_of(&dev),
        port_of(&test)
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn duplicate_postgresql_instance_starts_with_empty_data() {
    let ev = evidence!(
        covers = ["REQ-009", "AC-025"],
        kind = Kind::Native,
        expected = "a duplicate of a stopped PostgreSQL 17 instance that holds a sentinel starts with a data folder of its own: it has no stackly_sentinel table, its system identifier differs from the source, the source folder is byte for byte the same after the form, the create, and the run of the duplicate, and the source still holds its sentinel when it starts again",
    );
    let Some(site) = Site::open(&ev, PG17) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    // The source holds data, then stops.
    let made = create_instance(&site, &api, "pg17-dev").await;
    api.start_ready(made.id).await;
    let dev = SentinelCluster::new(
        "pg17-dev",
        made.id,
        site.psql(made.id, made.port),
        &made.password,
    );
    let version = dev.psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    dev.write(DEV_SENTINEL);
    let dev_identifier = dev.system_identifier();
    stop(&api, made.id).await;
    let dev_data = site.instance_paths(made.id).data;
    let before = tree(&dev_data);
    assert!(!before.is_empty(), "the source has a data folder");

    // Duplicate it; nothing under the source changes.
    let test_id = duplicate(&api, made.id, "pg17-test").await;
    assert_eq!(
        tree(&dev_data),
        before,
        "the form and the create touched the source"
    );
    let summary = api.start_ready(test_id).await;
    let test = SentinelCluster::new(
        "pg17-test",
        test_id,
        site.psql(test_id, port_of(&summary)),
        &made.password,
    );
    assert!(
        test.read().is_empty(),
        "the duplicate reads the sentinel of its source"
    );
    assert_ne!(test.system_identifier(), dev_identifier);
    test.write(TEST_SENTINEL);
    SentinelCluster::assert_isolated(&[(&test, TEST_SENTINEL)]);
    stop(&api, test_id).await;
    assert_eq!(
        tree(&dev_data),
        before,
        "the run of the duplicate touched the source"
    );
    assert!(
        files_holding(&site.instance_paths(test_id).data, DEV_SENTINEL).is_empty(),
        "the data folder of the duplicate holds the sentinel of the source"
    );

    // The source is still the source.
    api.start_ready(made.id).await;
    SentinelCluster::assert_isolated(&[(&dev, DEV_SENTINEL)]);
    assert_eq!(dev.system_identifier(), dev_identifier);

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version}: the duplicate started with no sentinel table and its own system identifier, the data folder of the source ({} entries) was unchanged by the form, the create, and the run, and the source kept its sentinel",
        before.len()
    ));
}
