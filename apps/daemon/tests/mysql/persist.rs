//! Persisted variables never outlive `instance.restart` (spec 0042 AC-5):
//! `SET PERSIST` and `SET PERSIST_ONLY` write `mysqld-auto.cnf`, which the
//! next process never loads, so `max_connections` comes back from settings,
//! the endpoint, bind address, and `skip_name_resolve` stay Stackly's, no
//! admin interface or X Protocol listener appears, and `stackly_readiness`
//! keeps only `USAGE` and stays off TCP.

use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::config_calls::pid_of;
use crate::harness::{
    MY84, MY97, My, READINESS, Site, Via, create_instance, free_port, listeners, stop_and_shutdown,
    variable,
};

async fn ignores_persisted_variables_across_an_instance_restart(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "persist").await;
    let first = api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let other_port = free_port();
    let persisted = [
        "SET PERSIST max_connections = 77".to_owned(),
        "SET PERSIST_ONLY skip_name_resolve = OFF".to_owned(),
        "SET PERSIST_ONLY bind_address = '0.0.0.0'".to_owned(),
        format!("SET PERSIST_ONLY port = {other_port}"),
        "SET PERSIST_ONLY admin_address = '127.0.0.1'".to_owned(),
    ];
    // The server may refuse a read only variable; what it accepts is what
    // a later start must not load.
    let mut accepted = Vec::new();
    for statement in &persisted {
        let ran = mysql.admin(&made.password, statement);
        if ran.ok {
            accepted.push(statement.as_str());
        }
    }
    assert!(
        accepted.contains(&"SET PERSIST max_connections = 77")
            && accepted.contains(&"SET PERSIST_ONLY skip_name_resolve = OFF"),
        "{accepted:?}"
    );
    // SET PERSIST changes the running server at once.
    assert_eq!(variable(&mysql, &made.password, "max_connections"), "77");
    let auto_cnf = site.instance_paths(made.id).data.join("mysqld-auto.cnf");
    let written = std::fs::read_to_string(&auto_cnf).expect("SET PERSIST wrote mysqld-auto.cnf");
    assert!(written.contains("max_connections"), "{written}");

    let restart = api
        .restart(made.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart of a running instance begins an operation");
    api.wait_state(restart.operation.id, OperationState::Succeeded)
        .await;
    let again = api
        .wait_instance(made.id, "ready after the restart", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
        })
        .await;
    assert_ne!(
        pid_of(&again),
        pid_of(&first),
        "the restart ran a new mysqld"
    );

    assert_eq!(variable(&mysql, &made.password, "max_connections"), "151");
    assert_eq!(variable(&mysql, &made.password, "skip_name_resolve"), "1");
    assert_eq!(
        variable(&mysql, &made.password, "bind_address"),
        "127.0.0.1"
    );
    assert_eq!(
        variable(&mysql, &made.password, "port"),
        made.port.to_string()
    );
    assert_eq!(variable(&mysql, &made.password, "admin_address"), "NULL");
    let (listening, _) = listeners(pid_of(&again));
    assert_eq!(listening.len(), 1, "{listening:?}");
    assert!(
        listening[0].contains(&format!("127.0.0.1:{}", made.port)),
        "{listening:?}"
    );
    assert_eq!(
        mysql.scalar(
            &made.password,
            "SELECT COUNT(*) FROM information_schema.plugins WHERE plugin_name = 'mysqlx' AND plugin_status = 'ACTIVE'"
        ),
        "0"
    );
    assert_eq!(
        mysql.scalar(
            &made.password,
            "SHOW GRANTS FOR 'stackly_readiness'@'localhost'"
        ),
        "GRANT USAGE ON *.* TO `stackly_readiness`@`localhost`"
    );
    let over_tcp = mysql.run(Via::Tcp, READINESS, None, "SELECT 1");
    assert!(!over_tcp.ok, "the readiness account came in over TCP");
    let over_socket = mysql.run(Via::Socket, READINESS, None, "SELECT 1");
    assert!(over_socket.ok, "{over_socket:?}");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "after instance.restart none of {accepted:?} was loaded: max_connections 151, skip_name_resolve 1, bind 127.0.0.1, the endpoint port, no admin address, one listener, mysqlx off, stackly_readiness USAGE only and refused over TCP"
    ));
}

// covers: AC-5 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_ignores_persisted_variables_across_an_instance_restart() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "SET PERSIST max_connections and SET PERSIST_ONLY of skip_name_resolve, bind_address, port, and admin_address, then instance.restart, give a new mysqld with max_connections from settings, skip_name_resolve 1, the endpoint bind and port, no admin address, one TCP listener, mysqlx off, and stackly_readiness with only USAGE, refused over TCP",
    );
    ignores_persisted_variables_across_an_instance_restart(MY84, ev).await;
}

// covers: AC-5 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_ignores_persisted_variables_across_an_instance_restart() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "SET PERSIST max_connections and SET PERSIST_ONLY of skip_name_resolve, bind_address, port, and admin_address, then instance.restart, give a new mysqld with max_connections from settings, skip_name_resolve 1, the endpoint bind and port, no admin address, one TCP listener, mysqlx off, and stackly_readiness with only USAGE, refused over TCP",
    );
    ignores_persisted_variables_across_an_instance_restart(MY97, ev).await;
}
