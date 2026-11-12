//! The compatibility evidence of the ClickHouse package (spec 0043 AC-16,
//! BR-025, spec 0018 *Compatibility record*): one test runs the final
//! package on ClickHouse 26.8.30.8 through the daemon, from create to stop,
//! with the provider checks the generic conformance harness has no step
//! for (a readiness probe that checks the instance ID, the listener audit,
//! the password, two instances with a table of the same name, a reload, and
//! data over a restart), and writes `compatibility.json` with the checks
//! that passed. The evidence comes from the final `provider.yaml`, so a
//! change of the package needs a new run.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use serde_json::json;
use stackly_core::provider::{PlanContext, ProbeSpec, ResolvedEndpoint, RuntimeContext};
use stackly_core::secret::Redactor;
use stackly_core::supervisor::SupervisorConfig;
use stackly_core::supervisor::probe::{ProbeContext, ProbeOutcome, TcpConnect, run};
use stackly_domain::Port;
use stackly_protocol::instance::{
    InstanceHealthState, InstanceLifecycle, InstanceReadinessState, InstanceSummary,
};
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::config_calls::{Cfg, settings_with};
use crate::harness::{
    Foreign, ForeignOptions, Made, PROVIDER, Site, VERSION, alive, assert_two_listeners,
    create_instance, eventually, http_query, instance_id, package, pid_of, port_of, stop_instance,
};
use crate::support::Api;

/// The checks a run proved, in the order they passed. A name is added only
/// after its assertions passed.
#[derive(Debug, Default)]
struct Checks(Vec<&'static str>);

impl Checks {
    fn passed(&mut self, name: &'static str) {
        assert!(!self.0.contains(&name), "check {name} passed twice");
        self.0.push(name);
    }

    /// Writes `compatibility.json` into the artifact folder of `evidence`:
    /// the final package revision of the run and the checks in alphabetical
    /// order.
    fn write(&self, evidence: &Evidence, site: &Site) {
        let mut checks = self.0.clone();
        checks.sort_unstable();
        let file = json!({
            "schema": "stackly.compatibility-evidence/v1",
            "providerRevision": {
                "id": PROVIDER,
                "revision": site.revision,
                "manifestSha256": site.manifest_sha256,
            },
            "checks": checks,
            "knownLimitations": [],
        });
        let path = evidence.artifact_dir().join("compatibility.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&file).expect("the file is JSON"),
        )
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    }
}

/// The readiness probe of the run plan of `made` with its endpoints on
/// `http` and `native`, the way the supervisor builds it.
fn readiness_probe(site: &Site, made: &Made, http: u16, native: u16) -> (ProbeSpec, Duration) {
    let package = package(&site.platform);
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let endpoint = |port: u16| ResolvedEndpoint::Ip {
        host: loopback,
        port: Some(Port::new(port).expect("a test port is not zero")),
    };
    let context = PlanContext {
        instance_id: instance_id(made.id),
        instance_name: "compat".parse().expect("a valid name"),
        paths: site.instance_paths(made.id),
        runtime: RuntimeContext {
            root: site.runtime_dir.clone(),
            version: VERSION.parse().expect("a valid version"),
            line: "26.8".parse().expect("a valid line"),
        },
        endpoints: BTreeMap::from([
            ("http".parse().expect("an endpoint name"), endpoint(http)),
            (
                "native".parse().expect("an endpoint name"),
                endpoint(native),
            ),
        ]),
        settings: package
            .settings_schema()
            .validate(&json!({
                "defaultPassword": format!("local:instance/{}/default-password", made.id),
            }))
            .expect("the settings are valid"),
    };
    let plan = package.run_plan(&context).expect("the run plan builds");
    (plan.readiness.probe, plan.readiness.attempt_timeout)
}

/// One attempt of `probe` through the supervisor's probe runner.
async fn attempt(site: &Site, probe: &ProbeSpec, timeout: Duration) -> ProbeOutcome {
    let redactor = Redactor::new();
    let config = SupervisorConfig::default();
    run(
        probe,
        timeout,
        ProbeContext {
            platform: &site.platform,
            redactor: &redactor,
            config: &config,
            native: &TcpConnect,
        },
    )
    .await
}

/// The rows of `default.events` of the instance, as `id\tv` lines.
fn events(site: &Site, made: &Made, native: u16) -> String {
    site.sql_ok(
        native,
        "default",
        Some(&made.password),
        "SELECT * FROM default.events ORDER BY id",
    )
}

/// Creates `default.events` on the instance and inserts `row`.
fn fill_events(site: &Site, made: &Made, native: u16, row: &str) {
    site.exec(
        native,
        "default",
        Some(&made.password),
        "CREATE TABLE default.events (id UInt64, v String) ENGINE = MergeTree ORDER BY id",
    );
    site.exec(
        native,
        "default",
        Some(&made.password),
        &format!("INSERT INTO default.events VALUES {row}"),
    );
}

/// Stops `id` and waits until its process is gone.
async fn stop_gone(api: &Api, id: Uuid, pid: u32) {
    stop_instance(api, id).await;
    eventually("the server to be gone", Duration::from_secs(60), || {
        !alive(pid)
    })
    .await;
}

/// Starts `id` and waits until it is healthy.
async fn start_healthy(api: &Api, id: Uuid) -> InstanceSummary {
    let running = api.start_ready(id).await;
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    api.wait_instance(id, "healthy", |summary| {
        summary.health.state == InstanceHealthState::Healthy
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_compatibility() {
    let ev = evidence!(
        covers = ["REQ-037", "AC-037", "AC-074", "BR-025"],
        kind = Kind::Native,
        expected = "a ClickHouse 26.8.30.8 instance of the final package is created, started, ready, and healthy, its readiness probe refuses a foreign server with another instance ID, it listens only on its two registry ports, its default user needs the Keychain password, a second instance keeps its own default.events row, a logLevel reload keeps its PID, its row survives a stop and start, and both stop with no process left; compatibility.json lists those eight checks",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let mut checks = Checks::default();
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    // lifecycle: create, start, ready, healthy, a table and a row
    let a = create_instance(&site, &api, "compat-a", json!({})).await;
    let created = api.get(a.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true), "{created:#?}");
    let running = start_healthy(&api, a.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    let pid = pid_of(&running);
    let version = site.version(http, &a.password);
    assert_eq!(version, VERSION);
    site.subject(&ev, &version);
    fill_events(&site, &a, native, "(1, 'alpha')");
    assert_eq!(events(&site, &a, native), "1\talpha");
    checks.passed("lifecycle");

    // readiness_identity: the probe checks the instance ID macro
    let (own, timeout) = readiness_probe(&site, &a, http, native);
    let outcome = attempt(&site, &own, timeout).await;
    assert!(outcome.is_ok(), "the instance's own server: {outcome:?}");
    {
        let other = Foreign::start(
            &site,
            &Uuid::now_v7().to_string(),
            &ForeignOptions {
                wait: true,
                ..ForeignOptions::default()
            },
        );
        let (probe, timeout) = readiness_probe(&site, &a, other.http, other.native);
        let outcome = attempt(&site, &probe, timeout).await;
        assert!(
            !outcome.is_ok(),
            "a server with another ID is never ready: {outcome:?}"
        );
    }
    checks.passed("readiness_identity");

    // listeners: exactly the two registry ports, no UDP
    assert_two_listeners(pid, http, native);
    checks.passed("listeners");

    // auth: the default user needs its password
    assert!(
        !site
            .sql(native, "default", Some("not-the-password"), "SELECT 1")
            .ok(),
        "a wrong password is refused on the native port"
    );
    assert!(
        !site.sql(native, "default", None, "SELECT 1").ok(),
        "an empty password is refused on the native port"
    );
    let (status, body) = http_query(http, "default", "not-the-password", "SELECT 1");
    assert_ne!(status, 200, "a wrong password is refused over HTTP: {body}");
    assert_eq!(
        http_query(http, "default", &a.password, "SELECT 1"),
        (200, "1\n".to_owned())
    );
    checks.passed("auth");

    // isolation: a second instance with a table of the same name
    let b = create_instance(&site, &api, "compat-b", json!({})).await;
    let running_b = start_healthy(&api, b.id).await;
    let http_b = port_of(&running_b, "http");
    let native_b = port_of(&running_b, "native");
    fill_events(&site, &b, native_b, "(2, 'beta')");
    assert_eq!(events(&site, &a, native), "1\talpha");
    assert_eq!(events(&site, &b, native_b), "2\tbeta");
    assert!(
        !site
            .sql(native, "default", Some(&b.password), "SELECT 1")
            .ok(),
        "the password of b does not open a"
    );
    assert_two_listeners(pid_of(&running_b), http_b, native_b);
    assert_ne!(
        site.instance_paths(a.id).data,
        site.instance_paths(b.id).data
    );
    checks.passed("isolation");

    // reload: a logLevel change keeps the process
    let cfg = Cfg { api: &api };
    let view = cfg.view(a.id).await;
    cfg.apply_done(
        a.id,
        view.revision.number.get(),
        settings_with(&view, json!({ "logLevel": "debug" })),
    )
    .await;
    let reloaded = api.get(a.id).await.expect("the instance can be read");
    assert_eq!(pid_of(&reloaded), pid, "a reload keeps the process");
    assert_eq!(reloaded.readiness.state, InstanceReadinessState::Ready);
    let config = std::fs::read_to_string(site.instance_paths(a.id).config.join("config.xml"))
        .expect("config.xml reads");
    assert!(config.contains("<level>debug</level>"), "{config}");
    checks.passed("reload");

    // persistence: the row of a survives a stop and a start
    stop_gone(&api, a.id, pid).await;
    let again = start_healthy(&api, a.id).await;
    let native = port_of(&again, "native");
    assert_eq!(events(&site, &a, native), "1\talpha");
    checks.passed("persistence");

    // stop: both instances, no process left
    for (id, summary) in [(a.id, &again), (b.id, &running_b)] {
        stop_gone(&api, id, pid_of(summary)).await;
        let stopped = api.get(id).await.expect("the instance can be read");
        assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
        assert!(stopped.process.is_none(), "{stopped:#?}");
        assert!(stopped.last_failure.is_none(), "{stopped:#?}");
        assert!(stopped.leftover_processes.is_empty(), "{stopped:#?}");
    }
    drop(api);
    daemon.shutdown().await;
    checks.passed("stop");

    checks.write(&ev, &site);
    ev.pass_with(&format!(
        "ClickHouse {version} passed {}",
        checks.0.join(", ")
    ));
}
