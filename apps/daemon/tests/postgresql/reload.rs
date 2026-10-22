//! A reload of the running postmaster (spec 0019 AC-13, spec 0010 AC-14).
//!
//! No contract method reloads yet (feature 29), so the test stops its
//! in process daemon, which leaves the postmaster running, builds a
//! supervisor of its own on the same registry that adopts that postmaster,
//! writes the new config revision through the registry, and calls
//! `execute_reload`, the way the spec says. // provisional: F29

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
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{PG17, PG18, Pg, Site, create_instance, instance_id, stop_and_shutdown};

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

async fn applies_a_reloadable_setting_with_sighup_and_keeps_its_pid(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "reloaded").await;
    let running = api.start_ready(made.id).await;
    let pid = running.process.as_ref().expect("a running process").pid;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "none");
    let started = psql.scalar(&made.password, "SELECT pg_postmaster_start_time()");
    drop(api);
    daemon.shutdown().await;

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

    // A daemon adopts the postmaster again and stops it.
    drop(supervisor);
    drop(registry);
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
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!("log_statement became all on postmaster {pid}"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_applies_a_reloadable_setting_with_sighup_and_keeps_its_pid() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "after logStatement is changed to all in a new config revision, execute_reload makes SHOW log_statement say all with the same postmaster pid and start time",
    );
    applies_a_reloadable_setting_with_sighup_and_keeps_its_pid(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_applies_a_reloadable_setting_with_sighup_and_keeps_its_pid() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "after logStatement is changed to all in a new config revision, execute_reload makes SHOW log_statement say all with the same postmaster pid and start time",
    );
    applies_a_reloadable_setting_with_sighup_and_keeps_its_pid(pg, ev).await;
}
