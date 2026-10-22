//! `instance duplicate` against a real `stacklyd` with the fake runtime and
//! provider (spec 0024 AC-11 and the fake part of JOURNEY-02): the parity
//! of the CLI with the `Client` for a matrix of bad inputs, and a journey
//! that creates an instance, duplicates it, starts both, and reads a
//! different sentinel from each.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::{
    CreateInstanceParams, GetDuplicateFormParams, PortRequest, SecretFieldInput,
};
use stackly_protocol::secret::SecretInput;
use stackly_protocol::{Call, ErrorBody, ErrorCategory, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, paths, run, uid};
use crate::stacklyd::seed;

const LINE: &str = "Duplicating settings of fake-a; no data is copied.\n";

fn block_on<T: Send + 'static>(
    socket: &std::path::Path,
    work: impl AsyncFnOnce(Client) -> T + Send + 'static,
) -> T {
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
                work(client).await
            })
    })
    .join()
    .expect("the client thread ends")
}

/// The params of a create the sheet would send for the duplicate form of
/// `source`, after `edit`.
fn client_error(
    socket: &std::path::Path,
    source: String,
    edit: impl FnOnce(&mut CreateInstanceParams) + Send + 'static,
) -> Box<ErrorBody> {
    block_on(socket, async move |client| {
        let Reply::InstanceGetDuplicateForm(form) = client
            .call(Call::InstanceGetDuplicateForm(GetDuplicateFormParams {
                instance_id: source.parse().expect("an instance id"),
            }))
            .await
            .expect("the duplicate form is built")
        else {
            panic!("not a duplicate form");
        };
        let draft = form.draft;
        let mut params = CreateInstanceParams {
            name: draft.name,
            display_name: draft.display_name,
            provider: draft.provider,
            runtime_id: draft.runtime_id,
            endpoints: draft.endpoints,
            settings: draft.settings,
            secrets: draft
                .secrets
                .into_iter()
                .map(|plan| SecretFieldInput {
                    field: plan.field,
                    input: SecretInput::Generate,
                })
                .collect(),
        };
        edit(&mut params);
        client
            .call_detailed(
                Call::InstanceCreate(params),
                CallOptions::default().with_idempotency_key("parity-key-0002"),
            )
            .await
            .expect_err("a bad draft is refused")
    })
}

/// The parts of an error both paths must agree on.
fn essence(error: &Value) -> Value {
    json!({
        "code": error["code"],
        "category": error["category"],
        "details": error["details"],
        "fields": error["fields"],
    })
}

fn create_source(root: &TestRoot, runtime: &str, port: u16) -> String {
    let created = run(
        root.path(),
        &[
            "instance",
            "create",
            "fake-a",
            "--runtime",
            runtime,
            "--provider",
            "testing/fake",
            "--revision",
            "0.3.0",
            "--port",
            &format!("http={port}"),
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(created.code, Some(0), "{created:?}");
    envelope(&created)["result"]["instance"]["id"]
        .as_str()
        .expect("the created instance has an ID")
        .to_owned()
}

// covers: AC-11, AC-028
#[test]
fn a_bad_duplicate_gets_the_same_error_through_the_client_and_through_the_cli() {
    let ev = evidence!(
        covers = ["AC-028", "REQ-025"],
        kind = Kind::Fixture,
        expected = "for a bad name, a name in use, the port of the source, a setting of the wrong type, a foreign setting, and a bind to a missing reference, the params the Client sends from the duplicate form and the ones stackly instance duplicate --json sends get the same code, category, details, and fields from a real stacklyd",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let free = TcpListener::bind(("127.0.0.1", 0)).expect("a free port binds");
    let port = free.local_addr().expect("an address").port();
    drop(free);
    let source = create_source(&root, &runtime, port);
    let missing = "local:instance/0192f3a5-ffff-7000-8000-00000000ffff/auth-token";

    type Edit = Box<dyn FnOnce(&mut CreateInstanceParams) + Send>;
    let cases: Vec<(&str, Vec<String>, Edit)> = vec![
        (
            "name",
            vec!["Bad_Name".to_owned()],
            Box::new(|params| "Bad_Name".clone_into(&mut params.name)),
        ),
        (
            "name in use",
            vec!["fake-a".to_owned()],
            Box::new(|params| "fake-a".clone_into(&mut params.name)),
        ),
        (
            "port of the source",
            vec!["--port".to_owned(), format!("http={port}")],
            Box::new(move |params| {
                params.endpoints[0].port = PortRequest::Fixed(port);
            }),
        ),
        (
            "setting of the wrong type",
            vec!["--set-json".to_owned(), "readyDelayMs=\"abc\"".to_owned()],
            Box::new(|params| {
                let mut map = params.settings.as_map().clone();
                map.insert("readyDelayMs".to_owned(), json!("abc"));
                params.settings = stackly_protocol::instance::SettingsDocument::new(map);
            }),
        ),
        (
            "unknown setting",
            vec!["--set-json".to_owned(), "nosuch=1".to_owned()],
            Box::new(|params| {
                let mut map = params.settings.as_map().clone();
                map.insert("nosuch".to_owned(), json!(1));
                params.settings = stackly_protocol::instance::SettingsDocument::new(map);
            }),
        ),
        (
            "bind to a reference that does not exist",
            vec!["--secret".to_owned(), format!("authToken=bind:{missing}")],
            Box::new(move |params| {
                params.secrets = vec![SecretFieldInput {
                    field: "authToken".to_owned(),
                    input: SecretInput::Bind(missing.to_owned()),
                }];
            }),
        ),
    ];

    for (label, flags, edit) in cases {
        let from_client = client_error(daemon.socket(), source.clone(), edit);
        let mut args: Vec<String> = ["instance", "duplicate", "fake-a"]
            .map(str::to_owned)
            .to_vec();
        // A positional name is the first flag-less word of the case.
        let (name, rest) = match flags.first() {
            Some(first) if !first.starts_with("--") => (Some(first.clone()), &flags[1..]),
            _ => (None, &flags[..]),
        };
        args.extend(name);
        args.extend(rest.iter().cloned());
        args.push("--json".to_owned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let from_cli = run(root.path(), &refs);

        let value = envelope(&from_cli);
        assert_eq!(
            from_cli.stdout.trim_end().lines().count(),
            1,
            "{label}: one envelope line"
        );
        assert_eq!(from_cli.stderr, LINE, "{label}: {from_cli:?}");
        assert!(value["result"].is_null(), "{label}: {value}");
        let client_json = serde_json::to_value(&*from_client).expect("an error encodes");
        assert_eq!(essence(&value["error"]), essence(&client_json), "{label}");
        let exit = match from_client.category() {
            ErrorCategory::InvalidInput => 2,
            ErrorCategory::Conflict => 3,
            ErrorCategory::Unavailable => 6,
            ErrorCategory::NotFound => 10,
            other => panic!("{label}: the matrix has no exit code for {other:?}"),
        };
        assert_eq!(from_cli.code, Some(exit), "{label}");
    }
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}

/// One request to the dummy server on `port`: the status code and body.
fn exchange(port: u16, request: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the dummy server accepts");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a read timeout is set");
    stream
        .write_all(request.as_bytes())
        .expect("the request is written");
    let mut text = String::new();
    stream.read_to_string(&mut text).expect("the reply is read");
    let status = text
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    (status, body)
}

fn put_sentinel(port: u16, value: &str) -> u16 {
    exchange(
        port,
        &format!(
            "PUT /sentinel HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{value}",
            value.len()
        ),
    )
    .0
}

fn get_sentinel(port: u16) -> (u16, String) {
    exchange(
        port,
        "GET /sentinel HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
}

fn port_of(root: &TestRoot, name: &str) -> u16 {
    let inspected = run(root.path(), &["instance", "inspect", name, "--json"]);
    assert_eq!(inspected.code, Some(0), "{inspected:?}");
    envelope(&inspected)["result"]["instance"]["endpoints"][0]["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .expect("a started instance shows its port")
}

// covers: AC-11
#[test]
fn the_fake_journey_creates_duplicates_starts_both_and_reads_a_sentinel_of_each() {
    let ev = evidence!(
        covers = ["REQ-009", "REQ-025", "AC-009", "AC-025", "AC-028"],
        kind = Kind::Fixture,
        expected = "with a real stacklyd and the fake runtime, stackly instance create and stackly instance duplicate make two instances of one runtime, both start with --wait, each answers its own sentinel on its own port, the duplicate answers 404 before its first PUT although the source already holds a sentinel, and the duplicate result names the source",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);

    let first = run(
        root.path(),
        &[
            "instance",
            "create",
            "fake-a",
            "--runtime",
            &runtime,
            "--provider",
            "testing/fake",
            "--revision",
            "0.3.0",
            "--set",
            "greeting=hello from fake-a",
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(first.code, Some(0), "{first:?}");
    let first_id = envelope(&first)["result"]["instance"]["id"]
        .as_str()
        .expect("an instance ID")
        .to_owned();
    let second = run(
        root.path(),
        &[
            "instance",
            "duplicate",
            "fake-a",
            "fake-b",
            "--wait",
            "--timeout",
            "60s",
            "--json",
        ],
    );
    assert_eq!(second.code, Some(0), "{second:?}");
    assert_eq!(second.stderr.lines().next(), Some(LINE.trim_end()));
    let created = envelope(&second);
    assert_eq!(
        created["result"]["source"],
        json!({ "id": first_id, "name": "fake-a" })
    );
    let second_instance = &created["result"]["instance"];
    assert_eq!(second_instance["name"], "fake-b");
    assert_eq!(second_instance["runtimeId"], runtime.as_str());
    let second_id = second_instance["id"]
        .as_str()
        .expect("an instance ID")
        .to_owned();

    for id in [&first_id, &second_id] {
        let started = run(
            root.path(),
            &["instance", "start", id, "--wait", "--timeout", "60s"],
        );
        assert_eq!(started.code, Some(0), "{id}: {started:?}");
    }
    let (port_a, port_b) = (port_of(&root, "fake-a"), port_of(&root, "fake-b"));
    assert_ne!(port_a, port_b);

    assert_eq!(put_sentinel(port_a, "alpha"), 204);
    assert_eq!(get_sentinel(port_b).0, 404, "no data is copied");
    assert_eq!(put_sentinel(port_b, "bravo"), 204);
    assert_eq!(get_sentinel(port_a), (200, "alpha".to_owned()));
    assert_eq!(get_sentinel(port_b), (200, "bravo".to_owned()));

    for id in [&first_id, &second_id] {
        let stopped = run(
            root.path(),
            &["instance", "stop", id, "--wait", "--timeout", "60s"],
        );
        assert_eq!(stopped.code, Some(0), "{id}: {stopped:?}");
    }
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
