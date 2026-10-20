//! No secret value from a request that starts an operation reaches the
//! registry file or the daemon log (spec 0007 AC-3, NFR-011).

use serde_json::json;
use stackly_core::operation::testing::ScriptedWork;
use stackly_domain::OperationKind;
use stackly_protocol::codes::operation;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};

use crate::support::{
    Raw, SECRET_PARAM, Script, Scripts, Site, accepted, client, error_of, key, log_lines, log_text,
    wait_state,
};

#[tokio::test]
async fn a_secret_in_the_params_never_reaches_the_registry_file_or_the_daemon_log() {
    let ev = evidence!(
        covers = ["NFR-011"],
        kind = Kind::Fixture,
        expected = "after operations start over the socket with a fixture credential in their params, including a refused request without an idempotency key and a key replay, a byte scan of every registry file and of the daemon log at debug finds no fixture credential, and the request lines carry operation_id and no params",
    );
    let site = Site::new();
    let scripts = Scripts::new();
    scripts.add(
        "with_secret",
        Script::new(
            OperationKind::CreateInstance,
            ScriptedWork::new()
                .step("reserve")
                .creates_instance_tree()
                .step("init"),
        )
        .creating(),
    );
    let secret = FixtureCredential::new();
    let daemon = site.start(&scripts).await;
    let client = client(&site).await;
    let mut raw = Raw::ready(&site.socket()).await;
    let params = json!({ "script": "with_secret", SECRET_PARAM: secret.value() });
    let key = key();

    let refused = raw.start(None, params.clone()).await;
    let first = raw.start(Some(&key), params.clone()).await;
    let started = accepted(&first);
    let other_value =
        json!({ "script": "with_secret", SECRET_PARAM: FixtureCredential::new().value() });
    let replay = accepted(&raw.start(Some(&key), other_value).await);
    wait_state(&client, started.operation.id, OperationState::Succeeded).await;
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    assert!(error_of(&refused).is(&operation::IDEMPOTENCY_KEY_REQUIRED));
    assert!(
        replay.replayed,
        "two requests that differ only in the secret have one fingerprint"
    );
    assert_eq!(replay.operation.id, started.operation.id);
    secret.assert_absent(&refused.to_string(), "the refusal frame");
    secret.assert_absent(&first.to_string(), "the accepted frame");
    let files = site.registry_files();
    assert!(!files.is_empty());
    for file in files {
        let bytes = std::fs::read(&file).expect("a registry file can be read");
        secret.assert_absent(
            &String::from_utf8_lossy(&bytes),
            &file.display().to_string(),
        );
    }
    let log = log_text();
    secret.assert_absent(&log, "the daemon log");
    let id = started.operation.id.to_string();
    let request_lines: Vec<_> = log_lines()
        .into_iter()
        .filter(|line| {
            line["fields"]["message"] == "request"
                && line["fields"]["method"] == "test.start_operation"
                && line["fields"]["operation_id"] == id.as_str()
        })
        .collect();
    assert_eq!(
        request_lines.len(),
        2,
        "the start and its replay: {request_lines:?}"
    );
    for line in &request_lines {
        assert!(line["fields"].get("params").is_none());
        assert_eq!(line["fields"]["outcome"], "ok");
    }
    ev.pass();
}
