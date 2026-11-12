//! One ClickHouse journey through the `stacklyd` binary (spec 0043 AC-1,
//! AC-6, AC-7, AC-9): the daemon runs as its own process with
//! `STACKLY_ROOT` on a data root whose name holds `&`, a space, and
//! Unicode, so the password lives in the file keychain under that root, not
//! in a `FakeSecretStore`. The password holds all five XML markup
//! characters. Create, start, a query with the password over HTTP and
//! native TCP, a restart with the row kept, and a stop with nothing left.

use std::net::TcpStream;
use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    Serve, Site, VERSION, alive, assert_two_listeners, create_from, create_params, entries_under,
    eventually, http_query, new_password, output_of, pid_of, port_of, stop_instance, xml_text,
};
use crate::security::{files_holding, sha256_hex};

/// A data root name with an ampersand, a space, and Unicode: every path in
/// `config.xml` carries `&amp;`.
const AMPERSAND_ROOT: &str = "R&D Ünï cödé";

// covers: AC-1, AC-6, AC-7, AC-9 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_journey_through_the_stacklyd_binary_with_a_markup_password() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "the stacklyd binary with STACKLY_ROOT on a data root named with an ampersand, a space, and Unicode creates a ClickHouse instance whose password holds & < > \" ' in its file keychain, renders config.xml with &amp; in every path and the server reports the real path, starts it ready, lets the default user in with that password over HTTP and native TCP and refuses it without the markup, keeps users.xml to the SHA-256 of the password with no plain or escaped password in data/preprocessed_configs or anywhere under the test root, restarts it with a new PID and the row kept, and stops it with the process gone and both ports closed",
    );
    let Some(site) = Site::open_with(&ev, AMPERSAND_ROOT, Serve::Binary) else {
        return;
    };
    let daemon = site.spawn();
    let api = site.binary_api(&daemon).await;
    let plain = new_password();
    let password = format!("{plain}&<>\"'");
    let made = create_from(
        &api,
        create_params(&site, "journey", &password, None, json!({})),
    )
    .await;

    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    let pid = pid_of(&running);
    let version = site.version(http, &password);
    assert_eq!(version, VERSION);
    site.subject(&ev, &version);
    assert_two_listeners(pid, http, native);
    assert_eq!(
        site.sql_ok(native, "default", Some(&password), "SELECT 1"),
        "1"
    );
    assert_eq!(
        http_query(http, "default", &password, "SELECT 2"),
        (200, "2\n".to_owned())
    );
    let without_markup = site.sql(native, "default", Some(&plain), "SELECT 1");
    assert!(!without_markup.ok(), "{without_markup:?}");
    assert!(
        without_markup.stderr.contains("Authentication failed"),
        "{without_markup:?}"
    );
    assert_ne!(http_query(http, "default", &plain, "SELECT 1").0, 200);

    let paths = site.instance_paths(made.id);
    let config =
        std::fs::read_to_string(paths.config.join("config.xml")).expect("config.xml is rendered");
    let data_text = format!("{}/", paths.data.display());
    assert!(data_text.contains('&'), "{data_text}");
    assert!(
        config.contains(&format!("<path>{}</path>", xml_text(&data_text))),
        "config.xml writes the data folder with &amp;: {config}"
    );
    assert!(
        !config.contains(AMPERSAND_ROOT),
        "no raw & in config.xml: {config}"
    );
    assert_eq!(
        site.sql_ok(
            native,
            "default",
            Some(&password),
            "SELECT value FROM system.server_settings WHERE name = 'path'"
        ),
        data_text,
        "the server reads the entity back as the real path"
    );

    let hash = sha256_hex(&password);
    let users = std::fs::read_to_string(site.users_xml(made.id)).expect("users.xml is there");
    assert!(
        users.contains(&format!(
            "<password_sha256_hex>{hash}</password_sha256_hex>"
        )),
        "{users}"
    );
    let preprocessed = paths.data.join("preprocessed_configs");
    assert!(
        !files_holding(&preprocessed, hash.as_bytes()).is_empty(),
        "ClickHouse copies the hash into preprocessed_configs"
    );
    let escaped = xml_text(&password)
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    for needle in [password.as_str(), escaped.as_str(), plain.as_str()] {
        let leaked = files_holding(site.root.path(), needle.as_bytes());
        assert!(leaked.is_empty(), "the password is on disk: {leaked:#?}");
    }
    let table = output_of("/bin/ps", &["-axww", "-o", "args="]);
    assert!(!table.contains(&plain), "the password is in an argv");

    site.exec(
        native,
        "default",
        Some(&password),
        "CREATE TABLE default.journey (id UInt64) ENGINE = MergeTree ORDER BY id",
    );
    site.exec(
        native,
        "default",
        Some(&password),
        "INSERT INTO default.journey VALUES (7)",
    );
    let accepted = api
        .restart(made.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let restarted = api
        .wait_instance(made.id, "ready after the restart", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != pid)
        })
        .await;
    let new_pid = pid_of(&restarted);
    assert!(!alive(pid), "the old server is gone after the restart");
    let native_after = port_of(&restarted, "native");
    let http_after = port_of(&restarted, "http");
    assert_eq!(
        site.sql_ok(
            native_after,
            "default",
            Some(&password),
            "SELECT id FROM default.journey"
        ),
        "7",
        "the row survives the restart"
    );
    assert_eq!(
        http_query(http_after, "default", &password, "SELECT 3"),
        (200, "3\n".to_owned())
    );

    stop_instance(&api, made.id).await;
    eventually("the server to be gone", Duration::from_secs(60), || {
        !alive(new_pid)
    })
    .await;
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.process.is_none());
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    for port in [http_after, native_after] {
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "port {port} is closed after the stop"
        );
    }
    assert!(
        !site.users_xml(made.id).exists(),
        "a clean stop removes users.xml"
    );
    for needle in [password.as_str(), escaped.as_str(), plain.as_str()] {
        let leaked = files_holding(site.root.path(), needle.as_bytes());
        assert!(
            leaked.is_empty(),
            "the password is on disk after the stop: {leaked:#?}"
        );
    }
    let keychain = entries_under(&site.prepared.layout().keychain_dir());
    assert!(
        keychain.iter().any(|entry| entry.ends_with(".keychain-db")),
        "the binary daemon keeps the secret in its file keychain: {keychain:?}"
    );

    drop(api);
    let status = daemon.stop().expect("stacklyd stops");
    assert!(status.success(), "stacklyd exits 0: {status:?}");
    ev.pass_with(&format!(
        "stacklyd on `{AMPERSAND_ROOT}` ran ClickHouse {version} with a markup password from its file keychain over HTTP and native, restarted it from PID {pid} to {new_pid} with the row kept, and stopped it cleanly"
    ));
}
