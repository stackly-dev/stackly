//! Changing a PostgreSQL 18 instance through the daemon (spec 0029, milestone
//! 4): the config methods against the real `postgres` process of the managed
//! runtime. A reload setting keeps the postmaster; a restart setting starts a
//! new one; the revision that reached ready becomes the last known good; a
//! setting that stops the start is rolled back; a port change restarts on the
//! new port and a port held by a process outside Stackly is refused; and a
//! `postgresql.conf` edited by hand blocks every render until the revision is
//! kept. Every test is native, `#[ignore = "native: ..."]`, and ends as
//! `BLOCKED` when the artifact is not in the runtime cache.

use std::net::TcpListener;
use std::time::Duration;

use serde_json::{Value, json};
use stackly_ipc::CallOptions;
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ChangeEndpointsReply, ConfigApplyAccepted,
    ConfigChangeRequest, ConfigImpact, ConfigRevisionOrigin, DriftKind, GetConfigParams,
    InstanceConfigView, KeepRevisionParams, ValidateConfigParams,
};
use stackly_protocol::instance::{
    EndpointRequest, InstanceActionAccepted, InstanceBlocker, InstanceLifecycle, InstanceSummary,
    PortRequest, SettingsDocument,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::format::copy_tree;
use crate::harness::{
    PG18, Site, create_instance, eventually, free_port, package_dir, stop_and_shutdown,
};
use crate::support::{Api, key};

fn number(n: u64) -> ConfigRevisionNumber {
    ConfigRevisionNumber::new(n).expect("a revision number")
}

fn pid_of(summary: &InstanceSummary) -> u32 {
    summary.process.as_ref().expect("a process runs").pid
}

/// The config methods of the socket, for one test.
struct Cfg<'a> {
    api: &'a Api,
}

impl Cfg<'_> {
    async fn view(&self, id: Uuid) -> InstanceConfigView {
        match self
            .api
            .client
            .call(Call::InstanceConfigGet(GetConfigParams { instance_id: id }))
            .await
            .expect("the config can be read")
        {
            Reply::InstanceConfigGet(view) => *view,
            other => panic!("not a config reply: {other:?}"),
        }
    }

    async fn wait_view(
        &self,
        id: Uuid,
        what: &str,
        ready: impl Fn(&InstanceConfigView) -> bool,
    ) -> InstanceConfigView {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let view = self.view(id).await;
            if ready(&view) {
                return view;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the config of {id} did not get {what}: {view:#?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn impact(&self, id: Uuid, base: u64, change: ConfigChangeRequest) -> ConfigImpact {
        match self
            .api
            .client
            .call(Call::InstanceConfigValidate(ValidateConfigParams {
                instance_id: id,
                base_revision: number(base),
                change,
            }))
            .await
            .expect("the validation answers")
        {
            Reply::InstanceConfigValidate(verdict) => {
                assert!(verdict.valid, "{verdict:#?}");
                verdict.impact
            }
            other => panic!("not a validate reply: {other:?}"),
        }
    }

    async fn apply(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> Result<ConfigApplyAccepted, Box<ErrorBody>> {
        match self
            .api
            .client
            .call_detailed(
                Call::InstanceConfigApply(ApplyConfigParams {
                    instance_id: id,
                    base_revision: number(base),
                    change,
                    expected_revision: None,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceConfigApply(accepted) => Ok(*accepted),
            other => panic!("not an apply reply: {other:?}"),
        }
    }

    /// Applies `change` and returns the operation once it ended, whatever
    /// its state.
    async fn apply_final(
        &self,
        id: Uuid,
        base: u64,
        change: ConfigChangeRequest,
    ) -> stackly_protocol::operation::OperationDetail {
        let accepted = self
            .apply(id, base, change)
            .await
            .expect("the apply is accepted")
            .accepted
            .expect("a change begins an operation");
        self.api.wait_final(accepted.operation.id).await
    }

    async fn apply_done(&self, id: Uuid, base: u64, change: ConfigChangeRequest) {
        let detail = self.apply_final(id, base, change).await;
        assert_eq!(
            detail.operation.state,
            OperationState::Succeeded,
            "{detail:?}"
        );
    }

    async fn keep(&self, id: Uuid) -> Result<InstanceActionAccepted, Box<ErrorBody>> {
        match self
            .api
            .client
            .call_detailed(
                Call::InstanceConfigKeepRevision(KeepRevisionParams { instance_id: id }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceConfigKeepRevision(accepted) => Ok(*accepted),
            other => panic!("not a keep reply: {other:?}"),
        }
    }

    async fn change_endpoints(
        &self,
        id: Uuid,
        base: u64,
        port: PortRequest,
    ) -> Result<ChangeEndpointsReply, Box<ErrorBody>> {
        match self
            .api
            .client
            .call_detailed(
                Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                    instance_id: id,
                    base_revision: number(base),
                    endpoints: vec![EndpointRequest {
                        name: "client".to_owned(),
                        port,
                    }],
                    dry_run: false,
                }),
                CallOptions::default().with_idempotency_key(key()),
            )
            .await?
        {
            Reply::InstanceChangeEndpoints(reply) => Ok(*reply),
            other => panic!("not a change endpoints reply: {other:?}"),
        }
    }
}

/// The settings of the active revision with `edits` laid over them.
fn settings_with(view: &InstanceConfigView, edits: Value) -> ConfigChangeRequest {
    let mut map = view.revision.values.as_map().clone();
    for (field, value) in edits.as_object().expect("edits are an object") {
        map.insert(field.clone(), value.clone());
    }
    ConfigChangeRequest::Settings(SettingsDocument::new(map))
}

fn error_code(result: Result<impl std::fmt::Debug, Box<ErrorBody>>) -> String {
    result
        .expect_err("the request is refused")
        .code()
        .to_owned()
}

// covers: AC-4, AC-7
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_applies_a_reload_setting_in_place_and_a_restart_setting_with_a_new_postmaster()
 {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Native,
        expected = "changing logStatement of a running PostgreSQL 18 instance is a reload impact that keeps the postmaster pid and start time and makes SHOW log_statement say all; changing maxConnections is a restart impact that ends with a new pid, SHOW max_connections 60, and the same data; each applied revision becomes the last known good after the instance was ready on it",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let made = create_instance(&site, &api, "cfg-apply").await;
    let first = pid_of(&api.start_ready(made.id).await);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    let view = cfg
        .wait_view(made.id, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;
    assert_eq!(view.revision.number.get(), 1);
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Created);
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "none");
    assert!(psql.admin(&made.password, "CREATE TABLE kept (n int)").ok);
    assert!(psql.admin(&made.password, "INSERT INTO kept VALUES (7)").ok);
    let started = psql.scalar(&made.password, "SELECT pg_postmaster_start_time()");

    // A reload setting: the same postmaster, the new value, a new revision.
    let change = settings_with(&view, json!({ "logStatement": "all" }));
    assert_eq!(
        cfg.impact(made.id, 1, change.clone()).await,
        ConfigImpact::Reload
    );
    cfg.apply_done(made.id, 1, change).await;
    let view = cfg
        .wait_view(made.id, "revision 2 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(2)
        })
        .await;
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Applied);
    assert_eq!(pid_of(&api.get(made.id).await.expect("instance")), first);
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "all");
    assert_eq!(
        psql.scalar(&made.password, "SELECT pg_postmaster_start_time()"),
        started
    );

    // A restart setting: a new postmaster that serves the same data.
    let change = settings_with(&view, json!({ "maxConnections": 60 }));
    assert_eq!(
        cfg.impact(made.id, 2, change.clone()).await,
        ConfigImpact::Restart
    );
    cfg.apply_done(made.id, 2, change).await;
    let summary = api
        .wait_instance(made.id, "a new postmaster", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != first)
                && summary.readiness.state
                    == stackly_protocol::instance::InstanceReadinessState::Ready
        })
        .await;
    assert_ne!(pid_of(&summary), first);
    let view = cfg
        .wait_view(made.id, "revision 3 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(3)
        })
        .await;
    assert_eq!(view.history.len(), 3);
    assert_eq!(psql.scalar(&made.password, "SHOW max_connections"), "60");
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "all");
    assert_eq!(psql.scalar(&made.password, "SELECT n FROM kept"), "7");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "reload kept postmaster {first}, restart ended on {}, revision 3 is the last known good",
        pid_of(&summary)
    ));
}

/// A copy of the package whose schema lets `maxConnections` go past what
/// PostgreSQL itself accepts (262143), so a value the schema allows is one
/// the postmaster refuses to start with.
fn copy_that_allows_too_many_connections(root: &TestRoot) -> std::path::PathBuf {
    let dir = root.path().join("postgresql");
    copy_tree(&package_dir(), &dir);
    let schema = dir.join("schemas/settings.schema.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&schema).expect("the schema can be read"))
            .expect("the schema is JSON");
    let field = document
        .pointer_mut("/properties/maxConnections/maximum")
        .expect("maxConnections has a maximum");
    *field = json!(1_000_000);
    std::fs::write(
        &schema,
        serde_json::to_vec_pretty(&document).expect("the schema is written"),
    )
    .expect("the schema can be written");
    dir
}

// covers: AC-8
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_rolls_back_a_setting_that_stops_the_start() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Native,
        expected = "a restart change that PostgreSQL 18 refuses to start with (maxConnections past its own limit, allowed by an edited copy of the schema) ends its apply operation as failed, revision 2 never reaches ready and the last known good stays revision 1, and a rollback writes revision 3 with origin rollback and the values of revision 1, after which the instance is ready again with its data",
    );
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_that_allows_too_many_connections(&copies);
    let Some(site) = Site::open_package(&ev, PG18, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let made = create_instance(&site, &api, "cfg-rollback").await;
    api.start_ready(made.id).await;
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    assert!(psql.admin(&made.password, "CREATE TABLE kept (n int)").ok);
    assert!(
        psql.admin(&made.password, "INSERT INTO kept VALUES (11)")
            .ok
    );
    let view = cfg
        .wait_view(made.id, "revision 1 as last known good", |view| {
            view.last_known_good.map(|n| n.get()) == Some(1)
        })
        .await;

    let change = settings_with(&view, json!({ "maxConnections": 300_000 }));
    let detail = cfg.apply_final(made.id, 1, change).await;
    assert_eq!(
        detail.operation.state,
        OperationState::Failed,
        "PostgreSQL 18 started with maxConnections 300000: {detail:?}"
    );
    let after = cfg.view(made.id).await;
    assert_eq!(after.revision.number.get(), 2, "{after:#?}");
    assert!(after.revision.ready_at.is_none(), "{after:#?}");
    assert_eq!(after.last_known_good.map(|n| n.get()), Some(1));

    cfg.apply_done(made.id, 2, ConfigChangeRequest::Rollback)
        .await;
    let back = cfg.view(made.id).await;
    assert_eq!(back.revision.number.get(), 3);
    assert_eq!(back.revision.origin, ConfigRevisionOrigin::Rollback);
    assert_eq!(back.revision.source_number.map(|n| n.get()), Some(1));
    assert_eq!(back.revision.values, view.revision.values);
    let summary = api.get(made.id).await.expect("instance");
    if summary.process.is_none() {
        api.start_ready(made.id).await;
    }
    api.wait_instance(made.id, "ready again", |summary| {
        summary.readiness.state == stackly_protocol::instance::InstanceReadinessState::Ready
    })
    .await;
    cfg.wait_view(made.id, "revision 3 as last known good", |view| {
        view.last_known_good.map(|n| n.get()) == Some(3)
    })
    .await;
    assert_eq!(psql.scalar(&made.password, "SELECT n FROM kept"), "11");
    assert_eq!(psql.scalar(&made.password, "SHOW max_connections"), "100");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("revision 2 failed to start, revision 3 rolled back to the values of revision 1");
}

// covers: AC-14
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_changes_a_fixed_port_with_a_restart_and_refuses_a_port_an_outside_process_holds()
 {
    let ev = evidence!(
        covers = ["REQ-010", "REQ-015"],
        kind = Kind::Native,
        expected = "a fixed port that a listener outside Stackly holds is refused with a suggested port, no operation, and the same postmaster; a free fixed port restarts the running PostgreSQL 18 instance, SHOW port and the socket name follow it, the old port is free again, and the revision has origin endpoints",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let made = create_instance(&site, &api, "cfg-port").await;
    let first = pid_of(&api.start_ready(made.id).await);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    cfg.wait_view(made.id, "a last known good", |view| {
        view.last_known_good.is_some()
    })
    .await;

    // A listener of this test is a process outside Stackly.
    let outside = TcpListener::bind("127.0.0.1:0").expect("a free port to hold");
    let held = outside.local_addr().expect("its address").port();
    let refused = cfg
        .change_endpoints(made.id, 1, PortRequest::Fixed(held))
        .await
        .expect_err("a port an outside process holds is refused");
    assert!(
        refused.code().starts_with("supervisor.endpoint_"),
        "{refused:?}"
    );
    assert!(refused.detail("suggestedPort").is_some(), "{refused:?}");
    assert_eq!(pid_of(&api.get(made.id).await.expect("instance")), first);
    assert_eq!(cfg.view(made.id).await.revision.number.get(), 1);

    let free = free_port();
    let ChangeEndpointsReply::Applied(applied) = cfg
        .change_endpoints(made.id, 1, PortRequest::Fixed(free))
        .await
        .expect("a free port is accepted")
    else {
        panic!("not an applied reply");
    };
    assert_eq!(applied.impact, ConfigImpact::Restart);
    let accepted = applied.accepted.expect("a port change begins an operation");
    let detail = api.wait_final(accepted.operation.id).await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    let summary = api
        .wait_instance(made.id, "ready on the new port", |summary| {
            summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != first)
                && summary.endpoints[0].port == Some(free)
                && summary.readiness.state
                    == stackly_protocol::instance::InstanceReadinessState::Ready
        })
        .await;
    let view = cfg.view(made.id).await;
    assert_eq!(view.revision.origin, ConfigRevisionOrigin::Endpoints);
    assert_eq!(view.revision.number.get(), 2);
    let moved = site.psql(made.id, free);
    assert_eq!(moved.scalar(&made.password, "SHOW port"), free.to_string());
    assert!(
        site.run_dir(made.id)
            .join(format!(".s.PGSQL.{free}"))
            .exists()
    );
    drop(outside);
    eventually("the old port is free", Duration::from_secs(10), || {
        TcpListener::bind(("127.0.0.1", made.port)).is_ok()
    })
    .await;

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "port {held} was refused, port {free} restarted the instance on postmaster {}",
        pid_of(&summary)
    ));
}

// covers: AC-11
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_blocks_every_render_while_postgresql_conf_is_edited_by_hand() {
    let ev = evidence!(
        covers = ["REQ-015", "AC-015"],
        kind = Kind::Native,
        expected = "after postgresql.conf of a PostgreSQL 18 instance is edited outside Stackly, config get reports it modified with a diff and the instance blocker is configDrift; apply, restart, and reload are refused config.drift_detected with no change to the file; keep revision writes the rendered copy back, after which a reload applies and the postmaster keeps running",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let made = create_instance(&site, &api, "cfg-drift").await;
    let first = pid_of(&api.start_ready(made.id).await);
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);
    let view = cfg
        .wait_view(made.id, "a last known good", |view| {
            view.last_known_good.is_some()
        })
        .await;
    let path = site.instance_paths(made.id).config.join("postgresql.conf");
    let original = std::fs::read_to_string(&path).expect("postgresql.conf was rendered");

    let edited = format!("{original}\n# edited by hand\nwork_mem = '9MB'\n");
    std::fs::write(&path, &edited).expect("the file is edited");
    let drifted = cfg.view(made.id).await;
    let drift = drifted.drift.expect("the edit is drift");
    let file = drift
        .files
        .iter()
        .find(|file| file.path == "postgresql.conf")
        .expect("postgresql.conf is named");
    assert_eq!(file.kind, DriftKind::Modified);
    assert!(!file.diff.is_empty());
    assert!(matches!(
        api.get(made.id).await.expect("instance").blocker,
        Some(InstanceBlocker::ConfigDrift(_))
    ));

    assert_eq!(
        error_code(
            cfg.apply(
                made.id,
                1,
                settings_with(&view, json!({ "logStatement": "all" }))
            )
            .await
        ),
        "config.drift_detected"
    );
    assert_eq!(
        error_code(api.restart(made.id).await),
        "config.drift_detected"
    );
    assert_eq!(
        error_code(api.reload(made.id).await),
        "config.drift_detected"
    );
    assert_eq!(std::fs::read_to_string(&path).expect("reads"), edited);
    assert_eq!(pid_of(&api.get(made.id).await.expect("instance")), first);

    let kept = cfg
        .keep(made.id)
        .await
        .expect("keep revision is accepted")
        .accepted
        .expect("a drift begins an operation");
    api.wait_final(kept.operation.id).await;
    assert_eq!(std::fs::read_to_string(&path).expect("restored"), original);
    assert!(cfg.view(made.id).await.drift.is_none());

    cfg.apply_done(
        made.id,
        1,
        settings_with(&view, json!({ "logStatement": "ddl" })),
    )
    .await;
    assert_eq!(psql.scalar(&made.password, "SHOW log_statement"), "ddl");
    assert_eq!(pid_of(&api.get(made.id).await.expect("instance")), first);

    api.wait_instance(made.id, "running", |summary| {
        summary.lifecycle == InstanceLifecycle::Running
    })
    .await;
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("drift on postgresql.conf blocked apply, restart, and reload until keep revision");
}
