//! Steps of the PostgreSQL scenarios that more than one test runs: the
//! reload of a postmaster the daemon left running, the adoption that
//! follows it, and the refusal of data of another major.
#![allow(dead_code, reason = "each scenario module uses a part of these steps")]

use std::sync::Arc;

use stackly_core::secret::{
    Redactor, SecretCaller, SecretChange, SecretDeps, SecretEvents, SecretFieldRule,
    SecretFieldRules, SecretService,
};
use stackly_core::supervisor::{RegistryMode, ReloadRequest, Supervisor, SupervisorDeps};
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, Instance, Registry, SecretFieldName,
    WriteBatch, WriteOp,
};
use stackly_protocol::operation::OperationState;
use stackly_storage::{OpenOptions, RegistryOpen};

use crate::harness::{InProcess, Made, Pg, Site, instance_id, output_of};
use crate::support::Api;

struct AnyField;

impl SecretFieldRules for AnyField {
    fn rules(&self, _instance: &Instance, _field: &SecretFieldName) -> Option<SecretFieldRule> {
        Some(SecretFieldRule::new(8, Some(128), None))
    }
}

struct NoEvents;

impl SecretEvents for NoEvents {
    fn changed(&self, _change: &SecretChange) {}
}

/// Reloads `log_statement` of the running postmaster `pid` of `made` the way
/// `execute_reload` does, while no daemon runs (no contract method reloads
/// yet, feature 29). `started` is `pg_postmaster_start_time()` before the
/// reload; it and the pid must not change. // provisional: F29
pub async fn reload_log_statement(site: &Site, made: &Made, pid: u32, started: &str) {
    let psql = site.psql(made.id, made.port);
    // The postmaster outlives the daemon; a supervisor of the test adopts it.
    let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
        .open(
            &site.platform,
            site.prepared.layout(),
            stackly_core::daemon_info().version,
        )
        .expect("the registry opens again")
    else {
        panic!("the registry opens ready");
    };
    let registry = Arc::new(registry);
    let platform = Arc::new(site.platform.clone());
    let layout = Arc::new(site.prepared.clone());
    let redactor = Arc::new(Redactor::new());
    let secrets = Arc::new(SecretService::new(SecretDeps {
        registry: Arc::clone(&registry) as Arc<dyn Registry>,
        platform: Arc::clone(&platform),
        redactor: Arc::clone(&redactor),
        fields: Arc::new(AnyField),
        events: Arc::new(NoEvents),
    }));
    let deps = SupervisorDeps::new(
        Arc::clone(&registry) as Arc<dyn Registry>,
        Arc::clone(&platform),
        Arc::clone(&layout),
        secrets,
        redactor,
    );
    let supervisor = Supervisor::new(deps);
    supervisor
        .reconcile(RegistryMode::Ready)
        .await
        .expect("the supervisor reconciles");

    let id = instance_id(made.id);
    let active = registry
        .active_config_revision(id)
        .expect("the revision can be read")
        .value
        .expect("the instance has an active config revision");
    let mut values: serde_json::Value =
        serde_json::from_str(active.values.as_str()).expect("the values are JSON");
    values["logStatement"] = serde_json::Value::String("all".to_owned());
    let mut revision = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        id,
        ConfigValuesJson::new(values.to_string()).expect("the values are valid"),
        None,
        site.platform.clock().now(),
    );
    revision.previous = Some(active.id);
    registry
        .commit(WriteBatch::new(vec![WriteOp::InsertConfigRevision(
            revision,
        )]))
        .expect("the revision can be committed");

    let caller = SecretCaller::client("cli".parse().expect("a client code"), site.uid(), false);
    supervisor
        .execute_reload(&ReloadRequest::new(id, caller))
        .await
        .expect("the reload succeeds");

    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "all");
    assert_eq!(
        psql.scalar(&made.password, "SELECT pg_postmaster_start_time()"),
        started,
        "the postmaster was not restarted"
    );
    let rendered =
        std::fs::read_to_string(site.instance_paths(made.id).config.join("postgresql.conf"))
            .expect("the config was rendered again");
    assert!(rendered.contains("log_statement = 'all'"), "{rendered}");
    let still = std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(still.success(), "the postmaster pid {pid} is gone");

    drop(supervisor);
    drop(registry);
}

/// A new in process daemon adopts the postmaster `pid` of `made`. Returns
/// the daemon and a client of it.
pub async fn readopt(site: &Site, made: &Made, pid: u32) -> (InProcess, Api) {
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let adopted = api
        .wait_instance(made.id, "adopted", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.adopted)
        })
        .await;
    assert_eq!(
        adopted.process.as_ref().map(|process| process.pid),
        Some(pid)
    );
    (daemon, api)
}

/// Starts `id`, waits for the operation to fail, and returns its error.
pub async fn refused_start(api: &Api, id: uuid::Uuid) -> stackly_protocol::ErrorBody {
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start from stopped begins an operation");
    let failed = api.wait_final(accepted.operation.id).await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    failed
        .operation
        .failure
        .expect("the failed operation carries its error")
        .error
}

/// No `postgres` process serves `data` and no `postmaster.pid` is in it.
pub fn nothing_was_spawned(data: &std::path::Path) {
    let processes = output_of("/bin/ps", &["-axww", "-o", "command="]);
    assert!(
        !processes.contains(&data.display().to_string()),
        "a process serves {}",
        data.display()
    );
    assert!(
        !data.join("postmaster.pid").exists(),
        "a postmaster pid file is in the data folder"
    );
}

/// Binds the stopped instance `made` to `other_runtime` and starts it: the
/// start fails with `supervisor.data_incompatible` before any process is
/// spawned (AC-11). The instance is bound to its own runtime again after.
/// No daemon runs when this is called, and none after it returns.
pub async fn refuse_other_major(
    site: &Site,
    made: &Made,
    pg: Pg,
    other: Pg,
    other_runtime: &(stackly_domain::RuntimeId, stackly_domain::RuntimeLine),
) {
    let data = site.instance_paths(made.id).data;
    // The data of one major on a runtime of the other.
    site.rebind_instance(made.id, other_runtime);
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let error = refused_start(&api, made.id).await;
    assert_eq!(error.code(), "supervisor.data_incompatible", "{error:?}");
    assert_eq!(error.detail("outcome"), Some("incompatible"));
    assert_eq!(error.detail("data"), Some(pg.major));
    assert_eq!(error.detail("runtime"), Some(other.major));
    nothing_was_spawned(&data);
    drop(api);
    daemon.shutdown().await;

    site.rebind_instance(made.id, &site.own_runtime());
}
