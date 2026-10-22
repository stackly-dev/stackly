//! Creating PostgreSQL through the daemon on this Mac (spec 0020 milestone
//! 4: AC-12, AC-16, AC-25 for the daemon half): a preflight, a create with a
//! generated password on a fixed port, the result in the registry and the
//! manifest, the socket path, and the start to `ready`. An `Auto` port is
//! bound after the start, and a fixed port that an outside program takes
//! before the start fails the start.

use std::net::TcpListener;

use serde_json::json;
use stackly_domain::{Registry, SecretRef};
use stackly_protocol::instance::{
    CreateInstanceParams, CreatePreflight, EndpointPortMode, EndpointRequest, InstanceLifecycle,
    InstanceReadinessState, PortRequest, PreflightCheckKind, PreflightCreateParams,
    PreflightStatus, ProviderSelector, SecretFieldInput, SecretPlan, SecretSource,
    SettingsDocument,
};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::SecretInput;
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    PG17, PG18, PROVIDER, Pg, REVISION, Site, create_params, free_port, instance_id,
    stop_and_shutdown,
};
use crate::support::Api;

/// The secret field of the package.
const PASSWORD_FIELD: &str = "superuserPassword";

/// The preflight draft for a `client` endpoint on `port`, with a generated
/// password.
pub fn preflight_draft(site: &Site, name: &str, port: PortRequest) -> PreflightCreateParams {
    PreflightCreateParams {
        name: name.to_owned(),
        display_name: None,
        provider: ProviderSelector {
            name: PROVIDER.to_owned(),
            revision: Some(REVISION.to_owned()),
        },
        runtime_id: site.runtime_id,
        endpoints: vec![EndpointRequest {
            name: "client".to_owned(),
            port,
        }],
        settings: SettingsDocument::new(serde_json::Map::new()),
        secrets: vec![SecretPlan {
            field: PASSWORD_FIELD.to_owned(),
            source: SecretSource::Generate,
        }],
        start: false,
    }
}

/// The create that goes with [`preflight_draft`]: the daemon makes the
/// password.
pub fn generated_create(site: &Site, name: &str, port: PortRequest) -> CreateInstanceParams {
    let mut params = create_params(site, name, port, "unused", json!({}));
    params.secrets = vec![SecretFieldInput {
        field: PASSWORD_FIELD.to_owned(),
        input: SecretInput::Generate,
    }];
    params
}

/// Sends `params` and waits for the create to succeed. Returns the instance.
pub async fn create_and_wait(api: &Api, params: CreateInstanceParams) -> Uuid {
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .expect("the operation names the instance")
        .id
        .parse()
        .expect("an instance id");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(
        detail.operation.state,
        OperationState::Succeeded,
        "{detail:?}"
    );
    id
}

/// The password the daemon generated for `id`, read from the fake store of
/// the site through the reference `instance.get_detail` shows.
pub async fn generated_password(site: &Site, api: &Api, id: Uuid) -> String {
    let detail = api.detail(id).await.expect("the detail can be read");
    let secret = detail
        .secrets
        .iter()
        .find(|secret| secret.field == PASSWORD_FIELD)
        .expect("the instance uses the password field");
    let reference = SecretRef::parse(&secret.reference).expect("a valid reference");
    site.secrets
        .stored_value(&reference)
        .expect("the store holds the generated password")
}

/// The row of the preflight with `id`.
pub fn check<'a>(
    preflight: &'a CreatePreflight,
    id: &str,
) -> &'a stackly_protocol::instance::PreflightCheck {
    preflight
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("no row {id}: {:#?}", preflight.checks))
}

async fn is_created_with_a_generated_password_and_served_on_its_socket(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let port = free_port();

    let preflight = api
        .preflight(preflight_draft(&site, "made", PortRequest::Fixed(port)))
        .await
        .expect("the preflight is answered");
    assert!(preflight.ready, "{:#?}", preflight.checks);
    assert!(preflight.fields.is_empty(), "{:#?}", preflight.fields);
    let endpoint = check(&preflight, "endpoint.client");
    assert_eq!(endpoint.kind, PreflightCheckKind::Endpoint);
    assert_eq!(endpoint.status, PreflightStatus::Ok, "{endpoint:?}");
    assert_eq!(
        check(&preflight, "secret.superuserPassword").status,
        PreflightStatus::Ok
    );

    let id = create_and_wait(
        &api,
        generated_create(&site, "made", PortRequest::Fixed(port)),
    )
    .await;
    let created = api.get(id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));
    assert_eq!(created.provider.name, PROVIDER);
    assert_eq!(created.provider.revision, REVISION);
    assert_eq!(created.runtime_id, site.runtime_id);
    assert_eq!(created.runtime_version, pg.version);
    let run = site.run_dir(id);
    let client = created
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .expect("a client endpoint");
    assert_eq!(client.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(client.port, Some(port));
    assert_eq!(client.port_mode, EndpointPortMode::Fixed);
    assert!(!client.bound);
    // The socket path comes from the pinned `socketName` and the Fixed port
    // of the client endpoint, before any start.
    let socket = created
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "socket")
        .expect("a socket endpoint");
    let socket_path = run.join(format!(".s.PGSQL.{port}"));
    assert_eq!(
        socket.socket_path.as_deref(),
        Some(socket_path.to_str().expect("a UTF-8 path"))
    );
    assert!(!socket_path.exists(), "no server yet");

    let password = generated_password(&site, &api, id).await;
    assert!(password.len() >= 16, "a generated password is long enough");
    let paths = site.instance_paths(id);
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&paths.manifest).expect("the manifest is written"),
    )
    .expect("the manifest is JSON");
    assert!(
        !std::fs::read_to_string(&paths.manifest)
            .expect("the manifest is written")
            .contains(&password),
        "the password is in the manifest"
    );

    let running = api.start_ready(id).await;
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    let client = running
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .expect("a client endpoint");
    assert!(client.bound);
    assert_eq!(client.port, Some(port));
    assert!(socket_path.exists(), "the server made the socket");
    let psql = site.psql(id, port);
    let version = psql.scalar(&password, "SHOW server_version");
    assert_eq!(version, pg.version);
    site.subject(&ev, &version);
    let tcp = psql.run(
        crate::harness::Via::Tcp,
        "postgres",
        Some(&password),
        "postgres",
        "SELECT 1",
    );
    assert!(tcp.ok, "{tcp:?}");

    stop_and_shutdown(&api, daemon, id).await;

    // The result in the registry and the manifest (AC-12).
    let registry = site.open_registry();
    let instance = registry
        .instance(instance_id(id))
        .expect("the registry reads")
        .value
        .expect("the instance is registered");
    let initialized = instance
        .data_initialized_at()
        .expect("init_succeeded recorded the time");
    assert_eq!(
        manifest["createdAt"],
        serde_json::to_value(instance.created_at()).expect("a timestamp"),
        "{manifest}"
    );
    assert_eq!(
        manifest["data"]["initializedAt"],
        serde_json::to_value(initialized).expect("a timestamp"),
        "{manifest}"
    );
    ev.pass_with(&format!(
        "PostgreSQL {version} was created on 127.0.0.1:{port} with a generated password, showed its socket path before the start, and answered on it after"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_is_created_with_a_generated_password_and_served_on_its_socket() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-008", "AC-027"],
        kind = Kind::Native,
        expected = &format!(
            "a preflight of PostgreSQL {} is ready, the create with a generated password succeeds, the instance is stopped and initialized with the fixed port and the socket path of the pinned socketName, the manifest times equal the registry times, and the start reaches ready on that socket and on TCP",
            pg.major
        ),
    );
    is_created_with_a_generated_password_and_served_on_its_socket(pg, ev).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn postgresql_17_is_created_with_a_generated_password_and_served_on_its_socket() {
    let pg = PG17;
    let ev = evidence!(
        covers = ["REQ-008", "AC-027"],
        kind = Kind::Native,
        expected = &format!(
            "a preflight of PostgreSQL {} is ready, the create with a generated password succeeds, the instance is stopped and initialized with the fixed port and the socket path of the pinned socketName, the manifest times equal the registry times, and the start reaches ready on that socket and on TCP",
            pg.major
        ),
    );
    is_created_with_a_generated_password_and_served_on_its_socket(pg, ev).await;
}

async fn an_auto_port_is_bound_after_the_start(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    let preflight = api
        .preflight(preflight_draft(&site, "auto", PortRequest::Auto))
        .await
        .expect("the preflight is answered");
    assert!(preflight.ready, "{:#?}", preflight.checks);
    assert_eq!(
        check(&preflight, "endpoint.client").status,
        PreflightStatus::Ok
    );
    let id = create_and_wait(&api, generated_create(&site, "auto", PortRequest::Auto)).await;
    let created = api.get(id).await.expect("the instance can be read");
    let client = created
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .expect("a client endpoint");
    assert_eq!(client.port_mode, EndpointPortMode::Auto);
    assert_eq!((client.port, client.bound), (None, false));
    let socket = created
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "socket")
        .expect("a socket endpoint");
    assert_eq!(socket.socket_path, None, "no port yet, so no socket path");

    let running = api.start_ready(id).await;
    let client = running
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .expect("a client endpoint");
    let port = client.port.expect("the start stored the port");
    assert!(client.bound, "{client:?}");
    let reread = api.get(id).await.expect("the instance can be read");
    assert_eq!(
        reread
            .endpoints
            .iter()
            .find(|endpoint| endpoint.name == "client")
            .map(|endpoint| (endpoint.port, endpoint.bound)),
        Some((Some(port), true))
    );
    let socket_path = site.run_dir(id).join(format!(".s.PGSQL.{port}"));
    let socket = running
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "socket")
        .expect("a socket endpoint");
    assert_eq!(
        socket.socket_path.as_deref(),
        Some(socket_path.to_str().expect("a UTF-8 path"))
    );
    assert!(socket_path.exists(), "the server made the socket");
    let password = generated_password(&site, &api, id).await;
    let version = site.psql(id, port).scalar(&password, "SHOW server_version");
    assert_eq!(version, pg.version);
    site.subject(&ev, &version);

    stop_and_shutdown(&api, daemon, id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version} got port {port} at its start, showed bound true after readiness, and its socket path appeared with the port"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_gets_its_auto_port_at_the_start_and_shows_the_socket_path_then() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-010", "AC-055"],
        kind = Kind::Native,
        expected = "an Auto port instance shows no port and no socket path after the create, then the port, bound true, and the socket path of that port after the start reached ready",
    );
    an_auto_port_is_bound_after_the_start(pg, ev).await;
}

async fn a_port_taken_before_the_start_fails_the_start_and_leaves_the_taker(pg: Pg, ev: Evidence) {
    let Some(site) = Site::open(&ev, pg) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let port = free_port();
    let preflight = api
        .preflight(preflight_draft(&site, "race", PortRequest::Fixed(port)))
        .await
        .expect("the preflight is answered");
    assert!(preflight.ready, "{:#?}", preflight.checks);
    let id = create_and_wait(
        &api,
        generated_create(&site, "race", PortRequest::Fixed(port)),
    )
    .await;

    // An outside program takes the exact address after the preflight.
    let taker = TcpListener::bind(("127.0.0.1", port)).expect("the freed port binds again");
    let accepted = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation");
    let failed = api.wait_final(accepted.operation.id).await;
    assert_eq!(failed.operation.state, OperationState::Failed, "{failed:?}");
    assert_eq!(Api::failure_code(&failed), "supervisor.endpoint_in_use");
    let paths = site.instance_paths(id);
    assert!(
        !paths.data.join("postmaster.pid").exists(),
        "no postmaster was spawned"
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
        "the outside listener is untouched"
    );
    let stopped = api.get(id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(
        stopped
            .endpoints
            .iter()
            .find(|endpoint| endpoint.name == "client")
            .map(|endpoint| (endpoint.port, endpoint.port_mode)),
        Some((Some(port), EndpointPortMode::Fixed)),
        "the stored port did not change"
    );
    drop(taker);

    // A listener on every address does not stop the server, which binds the
    // exact address (spec 0020 K2).
    let wildcard = TcpListener::bind(("0.0.0.0", port)).expect("the wildcard binds");
    let running = api.start_ready(id).await;
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    let password = generated_password(&site, &api, id).await;
    let version = site.psql(id, port).scalar(&password, "SHOW server_version");
    site.subject(&ev, &version);
    drop(wildcard);

    stop_and_shutdown(&api, daemon, id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version}: a taker on 127.0.0.1:{port} failed the start with supervisor.endpoint_in_use, spawned nothing, and stayed alive; a 0.0.0.0:{port} listener did not stop the start"
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_start_fails_when_an_outside_program_takes_its_port_after_the_preflight() {
    let pg = PG18;
    let ev = evidence!(
        covers = ["REQ-010", "AC-055"],
        kind = Kind::Native,
        expected = "a Fixed port that passed the preflight and is taken on the exact address before the start fails the start with supervisor.endpoint_in_use, spawns no postmaster, keeps the stored port, and leaves the outside listener alone; a listener on 0.0.0.0 at the same port does not stop the start",
    );
    a_port_taken_before_the_start_fails_the_start_and_leaves_the_taker(pg, ev).await;
}
