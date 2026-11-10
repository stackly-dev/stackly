//! A fixed port an outside process holds, on a real PostgreSQL (spec 0033
//! AC-4 and AC-14, milestone 5): a listener of this test holds the fixed
//! `client` port, so the start fails at the `endpoints` stage with
//! `supervisor.endpoint_in_use`, a free `suggestedPort` after it, the static
//! hint, and its operation ID, before any `postgres` runs. The instance stays
//! `stopped` with its port, registry row, config revision, data, lock, and
//! run folder exactly as before. Once the listener is gone, the same start
//! reaches ready on the port it asked for, not on the suggestion.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, TcpListener};

use stackly_protocol::instance::config::GetConfigParams;
use stackly_protocol::instance::{EndpointPortMode, InstanceLifecycle};
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::harness::{PG18, Pg, Site, create_instance, stop_and_shutdown, tree_digest};
use crate::support::Api;

/// The static hint of `supervisor.endpoint_in_use` (spec 0033).
const IN_USE_HINT: &str = "Stackly never moves a fixed port on its own. Pick another port for this endpoint, or stop the process that holds it.";
/// How many ports after the held one the suggestion search takes.
const CANDIDATES: u16 = 64;

/// What a refused start must leave as it was (AC-14): the endpoints and
/// their port requests, the registry revision of the instance, the config
/// revisions, the SHA-256 and mode of every data file, the instance lock,
/// and the names in the run folder other than spools and logs.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    endpoints: Vec<(String, Option<u16>, String)>,
    registry_revision: String,
    config_revision: String,
    data: BTreeMap<String, (String, u32)>,
    lock: Option<(u64, Vec<u8>)>,
    run: Vec<String>,
}

impl Snapshot {
    async fn take(site: &Site, api: &Api, id: Uuid) -> Self {
        use std::os::unix::fs::MetadataExt;
        let paths = site.instance_paths(id);
        let summary = api.get(id).await.expect("the instance reads");
        let config = match api
            .client
            .call(Call::InstanceConfigGet(GetConfigParams { instance_id: id }))
            .await
            .expect("the config can be read")
        {
            Reply::InstanceConfigGet(view) => *view,
            other => panic!("not a config reply: {other:?}"),
        };
        let lock = std::fs::metadata(&paths.lock).ok().map(|metadata| {
            (
                metadata.ino(),
                std::fs::read(&paths.lock).unwrap_or_default(),
            )
        });
        let run = tree_digest(&paths.run)
            .into_keys()
            .filter(|name| !name.ends_with(".spool") && !name.contains("log"))
            .collect();
        Self {
            endpoints: summary
                .endpoints
                .iter()
                .map(|endpoint| {
                    (
                        endpoint.name.clone(),
                        endpoint.port,
                        format!("{:?}", endpoint.port_mode),
                    )
                })
                .collect(),
            registry_revision: format!("{:?}", summary.revision),
            config_revision: serde_json::to_string(&(&config.revision, &config.history))
                .expect("the config revisions encode"),
            data: tree_digest(&paths.data),
            lock,
            run,
        }
    }
}

async fn a_held_fixed_port_fails_the_start_with_a_free_port_and_changes_nothing(
    pg: Pg,
    ev: Evidence,
) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "held").await;
    let created = api.get(made.id).await.expect("the instance reads");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.endpoints[0].port_mode, EndpointPortMode::Fixed);
    assert_eq!(created.endpoints[0].port, Some(made.port));

    // A listener of this test is the process outside Stackly.
    let holder =
        TcpListener::bind((Ipv4Addr::LOCALHOST, made.port)).expect("the test holds the port");
    let before = Snapshot::take(&site, &api, made.id).await;
    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start of a stopped instance begins an operation")
        .operation
        .id;
    let detail = api.wait_final(operation).await;

    assert_eq!(detail.operation.state, OperationState::Failed, "{detail:?}");
    assert_eq!(detail.operation.stage.as_deref(), Some("endpoints"));
    assert_eq!(Api::failure_code(&detail), "supervisor.endpoint_in_use");
    let error = &detail.operation.failure.as_ref().expect("a failure").error;
    assert_eq!(error.operation_id(), Some(operation), "the correlation ID");
    assert_eq!(error.hint(), Some(IN_USE_HINT));
    assert_eq!(error.detail("endpoint"), Some("client"));
    let port_text = made.port.to_string();
    assert_eq!(error.detail("port"), Some(port_text.as_str()));
    let suggested: u16 = error
        .detail("suggestedPort")
        .unwrap_or_else(|| panic!("a free port is suggested: {error:?}"))
        .parse()
        .expect("a port number");
    assert!(
        suggested > made.port
            && u32::from(suggested) <= u32::from(made.port) + u32::from(CANDIDATES),
        "{suggested} is not one of the candidates after {}",
        made.port
    );
    drop(
        TcpListener::bind((Ipv4Addr::LOCALHOST, suggested))
            .expect("the suggested port was free when the search ran and still is"),
    );

    // AC-14: the refusal came before the journal and repaired nothing.
    let refused = api.get(made.id).await.expect("the instance reads");
    assert_eq!(refused.lifecycle, InstanceLifecycle::Stopped);
    assert!(refused.process.is_none(), "no postgres was spawned");
    assert_eq!(refused.last_failure, None, "a refusal before the journal");
    assert_eq!(
        refused.endpoints[0].port,
        Some(made.port),
        "the fixed port stays"
    );
    assert_eq!(Snapshot::take(&site, &api, made.id).await, before);
    assert!(
        !site
            .instance_paths(made.id)
            .data
            .join("postmaster.pid")
            .exists(),
        "postgres never ran on the data"
    );

    // With the listener gone the same start reaches ready on its own port.
    drop(holder);
    let running = api.start_ready(made.id).await;
    assert_eq!(running.endpoints[0].port, Some(made.port));
    assert_eq!(running.endpoints[0].port_mode, EndpointPortMode::Fixed);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    assert_eq!(psql.scalar(&made.password, "SHOW port"), port_text);

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "with 127.0.0.1:{} held the start failed at endpoints with supervisor.endpoint_in_use, suggestedPort {suggested}, the static hint, and operation {operation}; the instance stayed stopped with port, registry revision, config revision, {} data files, lock, and run folder unchanged; after the listener closed PostgreSQL {version} reached ready on port {}",
        made.port,
        before.data.len(),
        made.port,
    ));
}

// covers: AC-4, AC-14 (spec 0033)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_with_a_held_fixed_port_fails_the_start_with_a_free_port() {
    let ev = evidence!(
        covers = ["AC-010", "AC-055", "NFR-011"],
        kind = Kind::Native,
        expected = "a start of a PostgreSQL 18 instance whose fixed client port a listener of the test holds fails at the endpoints stage with supervisor.endpoint_in_use, the endpoint and port, a free suggestedPort after it, the static hint, and its operation ID, without spawning postgres; the instance stays stopped without a last failure and its port, registry revision, config revision, data, lock, and run folder are unchanged; after the listener closes, the same instance reaches ready on the port it asked for",
    );
    a_held_fixed_port_fails_the_start_with_a_free_port_and_changes_nothing(PG18, ev).await;
}
