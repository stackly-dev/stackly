//! Reload of a ClickHouse instance (spec 0043 AC-10): a change of
//! `logLevel` from `information` to `debug` applied as a reload keeps the
//! PID and turns on `<Debug>` lines for the next queries, a new
//! `defaultPassword` through `secret.set` and a reload refuses the old
//! password and takes the new one as soon as the reload ends, and a broken
//! `config.xml` makes `SYSTEM RELOAD CONFIG` fail while the server keeps
//! serving its last good config.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{SecretInput, SecretMode, SecretText, SetSecretParams};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::config_calls::{Cfg, settings_with};
use crate::harness::{
    RELOAD_USER, Site, create_instance, http_query, log_lines, new_password, pid_of, port_of,
    stop_and_shutdown, wait_log,
};

// covers: AC-10 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn clickhouse_reload_changes_the_log_level_and_the_password() {
    let ev = evidence!(
        covers = ["REQ-037"],
        kind = Kind::Native,
        expected = "a logLevel change from information to debug applied to a running ClickHouse instance reloads it through SYSTEM RELOAD CONFIG with the same PID and ready, and <Debug> lines then appear for queries where there were none; a new defaultPassword through secret.set and a reload refuses the old password and takes the new one right after the reload; a broken config.xml makes SYSTEM RELOAD CONFIG exit non zero while the server keeps answering",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "reload", json!({})).await;
    let running = api.start_ready(made.id).await;
    let http = port_of(&running, "http");
    let native = port_of(&running, "native");
    let pid = pid_of(&running);
    site.subject(&ev, &site.version(http, &made.password));

    assert_eq!(
        site.sql_ok(native, "default", Some(&made.password), "SELECT 1"),
        "1"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    let before = log_lines(&api, made.id, "<Debug>").await;
    assert!(
        before.is_empty(),
        "no <Debug> line at information: {before:#?}"
    );

    let cfg = Cfg { api: &api };
    let view = cfg.view(made.id).await;
    let base = view.revision.number.get();
    cfg.apply_done(
        made.id,
        base,
        settings_with(&view, json!({ "logLevel": "debug" })),
    )
    .await;
    let reloaded = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(pid_of(&reloaded), pid, "a reload keeps the process");
    assert_eq!(reloaded.readiness.state, InstanceReadinessState::Ready);
    let config = std::fs::read_to_string(site.instance_paths(made.id).config.join("config.xml"))
        .expect("config.xml reads");
    assert!(config.contains("<level>debug</level>"), "{config}");
    assert_eq!(
        site.sql_ok(
            native,
            "default",
            Some(&made.password),
            "SELECT 'after the reload'"
        ),
        "after the reload"
    );
    let debug = wait_log(&api, made.id, "<Debug>", Duration::from_secs(10)).await;

    let fresh = new_password();
    let reply = api
        .client
        .call(Call::SecretSet(SetSecretParams {
            reference: format!("local:instance/{}/default-password", made.id),
            input: SecretInput::Value(SecretText::new(fresh.clone())),
            mode: SecretMode::Replace,
            expected_revision: None,
            confirm_stored_only: false,
        }))
        .await
        .expect("secret.set answers");
    assert!(matches!(reply, Reply::SecretSet(_)), "{reply:?}");
    assert!(
        site.sql(native, "default", Some(&made.password), "SELECT 1")
            .ok(),
        "the old password works until the reload"
    );
    let accepted = api
        .reload(made.id)
        .await
        .expect("the reload is accepted")
        .accepted
        .expect("a reload starts an operation");
    let done = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(done.operation.state, OperationState::Succeeded);
    let old = site.sql(native, "default", Some(&made.password), "SELECT 1");
    assert!(
        !old.ok(),
        "the old password is refused right after: {old:?}"
    );
    let (status, body) = http_query(http, "default", &made.password, "SELECT 1");
    assert_ne!(status, 200, "{body}");
    assert_eq!(
        site.sql_ok(native, "default", Some(&fresh), "SELECT 1"),
        "1",
        "the new password works right after"
    );
    assert_eq!(
        http_query(http, "default", &fresh, "SELECT 1"),
        (200, "1\n".to_owned())
    );
    assert_eq!(
        pid_of(&api.get(made.id).await.expect("the instance can be read")),
        pid
    );

    let config_path = site.instance_paths(made.id).config.join("config.xml");
    let good = std::fs::read_to_string(&config_path).expect("config.xml reads");
    std::fs::write(&config_path, "<clickhouse>\n    <logger>\n").expect("config.xml can be broken");
    let broken = site.sql(native, RELOAD_USER, None, "SYSTEM RELOAD CONFIG");
    assert!(!broken.ok(), "a broken config is refused: {broken:?}");
    assert_eq!(
        site.sql_ok(native, "default", Some(&fresh), "SELECT 'still here'"),
        "still here",
        "the server keeps serving its last good config"
    );
    std::fs::write(&config_path, good).expect("config.xml can be restored");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "PID {pid} kept through both reloads, {} <Debug> lines after the level change, the old password refused and the new one taken, and the broken config refused with exit {:?}",
        debug.len(),
        broken.code
    ));
}
