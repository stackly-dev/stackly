//! AC-028 for the config verbs (spec 0029 AC-17): for a matrix of refused
//! changes, the params a client sends through `stackly-ipc`'s `Client` and
//! the ones `stackly instance config apply`, `rollback`, and `endpoint set`
//! send get the same error from a real `stacklyd`: the same code, category,
//! details, and fields, and the CLI exits with the code of the category.

use serde_json::{Value, json};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::config::{
    ApplyConfigParams, ChangeEndpointsParams, ConfigChangeRequest,
};
use stackly_protocol::instance::{EndpointRequest, PortRequest, SettingsDocument};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, ErrorCategory};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};
use uuid::Uuid;

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, paths, run, uid};
use crate::stacklyd::seed;

/// The parts of an error both paths must agree on.
fn essence(error: &Value) -> Value {
    json!({
        "code": error["code"],
        "category": error["category"],
        "details": error["details"],
        "fields": error["fields"],
    })
}

fn client_error(socket: &std::path::Path, call: Call) -> Box<ErrorBody> {
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime starts")
            .block_on(async {
                let (client, _) = Client::connect(
                    &socket,
                    ConnectPolicy::same_user(uid()),
                    ClientInfo {
                        kind: ClientKind::Gui,
                        version: "0.1.0".to_owned(),
                        pid: Some(std::process::id()),
                        interactive: Some(true),
                    },
                )
                .await
                .expect("the client connects");
                client
                    .call_detailed(
                        call,
                        CallOptions::default().with_idempotency_key("config-parity-0001"),
                    )
                    .await
                    .expect_err("a refused change")
            })
    })
    .join()
    .expect("the client thread ends")
}

fn number(n: u64) -> ConfigRevisionNumber {
    ConfigRevisionNumber::new(n).expect("a revision number")
}

// covers: AC-028
#[test]
fn a_refused_change_gets_the_same_error_through_the_client_and_through_the_cli() {
    let ev = evidence!(
        covers = ["AC-028"],
        kind = Kind::Fixture,
        expected = "for a secret, a set once field, a value out of range, an unknown field, a stale base revision, a roll back with no last known good, and an unknown endpoint, the params sent through the Client and by stackly instance config apply, rollback, and endpoint set get the same code, category, details, and fields from a real stacklyd, and the CLI exits with the code of the category",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let created = run(
        root.path(),
        &[
            "instance",
            "create",
            "demo",
            "--runtime",
            &runtime,
            "--provider",
            "testing/fake",
            "--revision",
            "0.4.0",
            "--secret",
            "authToken=generate",
            "--wait",
            "--json",
        ],
    );
    assert_eq!(created.code, Some(0), "{created:?}");
    let view = envelope(&run(
        root.path(),
        &["instance", "config", "get", "demo", "--json"],
    ));
    let inspected = envelope(&run(
        root.path(),
        &["instance", "inspect", "demo", "--json"],
    ));
    let id: Uuid = inspected["result"]["instance"]["id"]
        .as_str()
        .and_then(|text| text.parse().ok())
        .expect("the instance id");
    let active = view["result"]["revision"]["values"]
        .as_object()
        .expect("the active settings")
        .clone();
    let token = active["authToken"]
        .as_str()
        .expect("a reference")
        .to_owned();
    let settings = |edit: (&str, Value)| {
        let mut map = active.clone();
        map.insert(edit.0.to_owned(), edit.1);
        ConfigChangeRequest::Settings(SettingsDocument::new(map))
    };
    let apply = |base: u64, change: ConfigChangeRequest| {
        Call::InstanceConfigApply(ApplyConfigParams {
            instance_id: id,
            base_revision: number(base),
            change,
            expected_revision: None,
        })
    };

    let other_token = format!("{token}x");
    let cases: Vec<(&str, Call, Vec<String>)> = vec![
        (
            "secret",
            apply(1, settings(("authToken", json!(other_token.clone())))),
            vec![
                "config".to_owned(),
                "apply".to_owned(),
                "demo".to_owned(),
                "--set".to_owned(),
                format!("authToken={other_token}"),
            ],
        ),
        (
            "set once",
            apply(1, settings(("failInit", json!(true)))),
            vec![
                "config".to_owned(),
                "apply".to_owned(),
                "demo".to_owned(),
                "--set".to_owned(),
                "failInit=true".to_owned(),
            ],
        ),
        (
            "out of range",
            apply(1, settings(("readyDelayMs", json!(-5)))),
            vec![
                "config".to_owned(),
                "apply".to_owned(),
                "demo".to_owned(),
                "--set".to_owned(),
                "readyDelayMs=-5".to_owned(),
            ],
        ),
        (
            "unknown field",
            apply(1, settings(("nosuch", json!(1)))),
            vec![
                "config".to_owned(),
                "apply".to_owned(),
                "demo".to_owned(),
                "--set-json".to_owned(),
                "nosuch=1".to_owned(),
            ],
        ),
        (
            "stale base",
            apply(7, settings(("greeting", json!("x")))),
            vec![
                "config".to_owned(),
                "apply".to_owned(),
                "demo".to_owned(),
                "--set".to_owned(),
                "greeting=x".to_owned(),
                "--expect-revision".to_owned(),
                "7".to_owned(),
            ],
        ),
        (
            "no last known good",
            apply(1, ConfigChangeRequest::Rollback),
            vec![
                "config".to_owned(),
                "rollback".to_owned(),
                "demo".to_owned(),
                "--yes".to_owned(),
            ],
        ),
        (
            "unknown endpoint",
            Call::InstanceChangeEndpoints(ChangeEndpointsParams {
                instance_id: id,
                base_revision: number(1),
                endpoints: vec![EndpointRequest {
                    name: "nope".to_owned(),
                    port: PortRequest::Fixed(5599),
                }],
                dry_run: false,
                override_leases: false,
            }),
            vec![
                "endpoint".to_owned(),
                "set".to_owned(),
                "demo".to_owned(),
                "--port".to_owned(),
                "nope=5599".to_owned(),
            ],
        ),
    ];

    for (label, call, verb) in cases {
        let from_client = client_error(daemon.socket(), call);
        let mut args: Vec<&str> = vec!["instance"];
        args.extend(verb.iter().map(String::as_str));
        args.push("--json");
        let from_cli = run(root.path(), &args);

        let value = envelope(&from_cli);
        assert_eq!(
            from_cli.stdout.trim_end().lines().count(),
            1,
            "{label}: one envelope line"
        );
        assert!(value["result"].is_null(), "{label}: {value}");
        let client_json = serde_json::to_value(&*from_client).expect("an error encodes");
        assert_eq!(essence(&value["error"]), essence(&client_json), "{label}");
        let exit = match from_client.category() {
            ErrorCategory::InvalidInput => 2,
            ErrorCategory::Conflict => 3,
            other => panic!("{label}: the matrix has no exit code for {other:?}"),
        };
        assert_eq!(from_cli.code, Some(exit), "{label}: {from_cli:?}");
    }
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
