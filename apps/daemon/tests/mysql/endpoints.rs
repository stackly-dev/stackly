//! Where `mysqld` listens and which parameters are Stackly's (spec 0042
//! AC-5): one TCP listener on the endpoint and one socket in the run folder,
//! no X Protocol, and `SET PERSIST` that never outlives a restart.

use stackly_protocol::instance::{EndpointPortMode, InstanceReadinessState};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::config_calls::pid_of;
use crate::harness::{
    MY84, MY97, My, READINESS, Site, Via, create_auto, create_instance, listeners,
    stop_and_shutdown, variable,
};
use crate::support::Api;

/// Stops `id` and waits for the stop to succeed.
pub async fn stop(api: &Api, id: uuid::Uuid) {
    let accepted = api.stop_when_free(id).await.accepted.expect("a stop");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
}

async fn listens_only_on_its_endpoint_and_ignores_persisted_variables(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "listen").await;
    let running = api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let (listening, sockets) = listeners(pid_of(&running));
    assert_eq!(listening.len(), 1, "{listening:?}");
    assert!(
        listening[0].contains(&format!("127.0.0.1:{}", made.port)),
        "{listening:?}"
    );
    let socket = site.run_dir(made.id).join("mysql.sock");
    assert!(!sockets.is_empty(), "{sockets:?}");
    assert!(
        sockets
            .iter()
            .all(|name| name == socket.to_str().unwrap_or_default()),
        "{sockets:?} against {}",
        socket.display()
    );
    assert_eq!(
        mysql.scalar(
            &made.password,
            "SELECT COUNT(*) FROM information_schema.plugins WHERE plugin_name = 'mysqlx' AND plugin_status = 'ACTIVE'"
        ),
        "0"
    );
    assert_eq!(variable(&mysql, &made.password, "admin_address"), "NULL");
    assert_eq!(
        variable(&mysql, &made.password, "port"),
        made.port.to_string()
    );

    // SET PERSIST writes mysqld-auto.cnf, which the next start never loads.
    for statement in [
        "SET PERSIST max_connections = 77",
        "SET PERSIST_ONLY skip_name_resolve = OFF",
    ] {
        let ran = mysql.admin(&made.password, statement);
        assert!(ran.ok, "{statement}: {ran:?}");
    }
    assert!(
        site.instance_paths(made.id)
            .data
            .join("mysqld-auto.cnf")
            .is_file()
    );
    stop(&api, made.id).await;
    let again = api.start_ready(made.id).await;
    assert_eq!(again.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(variable(&mysql, &made.password, "max_connections"), "151");
    assert_eq!(variable(&mysql, &made.password, "skip_name_resolve"), "1");
    let readiness_tcp = mysql.run(Via::Tcp, READINESS, None, "SELECT 1");
    assert!(!readiness_tcp.ok, "the readiness account came in over TCP");

    // An Auto port is bound at the start and is the port mysqld uses.
    let (auto, password) = create_auto(&site, &api, "auto").await;
    let summary = api.start_ready(auto).await;
    let client = summary
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "client")
        .expect("the client endpoint");
    assert_eq!(client.port_mode, EndpointPortMode::Auto);
    assert!(client.bound, "{client:?}");
    let port = client.port.expect("the bound Auto port is shown");
    let auto_mysql = site.mysql(auto, port);
    assert_eq!(variable(&auto_mysql, &password, "port"), port.to_string());
    stop(&api, auto).await;

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("one TCP listener and one socket, mysqlx off, SET PERSIST ignored after a restart, Auto port bound");
}

// covers: AC-5 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_listens_only_on_its_endpoint_and_ignores_persisted_variables() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "mysqld listens only on 127.0.0.1 at the client port and on <run>/mysql.sock with mysqlx and the admin interface off; after SET PERSIST max_connections = 77 and SET PERSIST_ONLY skip_name_resolve = OFF and a stop and start, max_connections is the settings value 151, skip_name_resolve is 1, readiness passes, and stackly_readiness is still refused over TCP; an Auto client port is bound after the start and equals @@port",
    );
    listens_only_on_its_endpoint_and_ignores_persisted_variables(MY84, ev).await;
}

// covers: AC-5 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_listens_only_on_its_endpoint_and_ignores_persisted_variables() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "mysqld listens only on 127.0.0.1 at the client port and on <run>/mysql.sock with mysqlx and the admin interface off; after SET PERSIST max_connections = 77 and SET PERSIST_ONLY skip_name_resolve = OFF and a stop and start, max_connections is the settings value 151, skip_name_resolve is 1, readiness passes, and stackly_readiness is still refused over TCP; an Auto client port is bound after the start and equals @@port",
    );
    listens_only_on_its_endpoint_and_ignores_persisted_variables(MY97, ev).await;
}
