//! The client password of NATS revision `1.1.0` against the real
//! `nats-server` (spec 0039 AC-2, AC-3, AC-4, AC-5): the `stackly` user
//! signs in with the instance password, a connection without credentials
//! lands as `stackly-readiness` in its own account with the least rights,
//! the password never leaks, and the readiness probe never reads a secret.
//! The daemon runs in process with the fake secret store, so the test can
//! count its reads.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_testkit::SecretOp;
use stackly_testkit::evidence::{Kind, evidence};

use crate::client::{Nats, loopback};
use crate::config_calls::Cfg;
use crate::harness::{
    PASSWORD_A, PASSWORD_ESCAPED, Serve, Site, USER, create_instance,
    create_instance_with_password, monitor_get, output_of, port_of, stop_instance, varz,
};

/// How long the test watches the probe after the instance is ready.
const WATCH: Duration = Duration::from_secs(60);

/// The monitoring pages the password must never show up on.
const MONITORING: [&str; 5] = [
    "/varz",
    "/connz?auth=1&subs=1",
    "/jsz",
    "/accountz",
    "/subsz",
];

/// Every file under `dir` whose bytes hold `needle`, skipping `except`.
fn files_holding(dir: &Path, needle: &[u8], except: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("the entry reads");
        let path = entry.path();
        let kind = entry.file_type().expect("the entry has a type");
        if kind.is_dir() {
            files_holding(&path, needle, except, found);
        } else if kind.is_file() && path != except {
            let bytes = std::fs::read(&path).unwrap_or_default();
            if bytes.windows(needle.len()).any(|window| window == needle) {
                found.push(path.display().to_string());
            }
        }
    }
}

/// Asserts that a refusal is a permission violation.
fn assert_permission_violation(result: Result<(), String>, what: &str) {
    let error = result.expect_err(what);
    assert!(
        error.starts_with("-ERR 'Permissions Violation"),
        "{what}: {error}"
    );
}

// covers: AC-2, AC-3, AC-4, AC-5 (spec 0039)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn nats_auth_in_process() {
    let ev = evidence!(
        covers = ["REQ-036"],
        kind = Kind::Native,
        expected = "a ready NATS 2.15.1 instance of revision 1.1.0 lets the stackly user with its password create a stream and publish, refuses a wrong password with Authorization Violation, maps a connection without credentials to stackly-readiness that may only PING, ask $JS.API.INFO, and subscribe to _INBOX.stackly.> and cannot reach a stream of the stackly user or see it in $JS.API.INFO, denies that user $JS.API.INFO when JetStream is off, accepts a password holding a double quote, a backslash, and a dollar sign, never shows the password in argv, the environment, an instance file, the log store, the monitoring pages, or the masked connection string, keeps nats.conf out of effectiveFiles with no drift, and probes for 60 seconds with at least 3 new connections and no secret read",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;

    // the stackly user and a wrong password
    let id = create_instance(&site, &api, "auth", json!({})).await;
    let running = api.start_ready(id).await;
    let pid = running.process.as_ref().expect("a running process").pid;
    let client = loopback(port_of(&running, "client"));
    let monitoring = port_of(&running, "monitoring");
    let version = varz(monitoring)["version"]
        .as_str()
        .expect("/varz reports the version")
        .to_owned();
    site.subject(&ev, &version);

    let mut app = Nats::signed_in(client, Some((USER, PASSWORD_A))).await;
    app.stream_create("ORDERS", "orders.>", "file")
        .await
        .expect("the stackly user creates a stream");
    app.publish("orders.one", b"first")
        .await
        .expect("the stackly user publishes");
    assert_eq!(app.stream_messages("ORDERS").await, 1);

    let (_, refused) = Nats::connect(client, Some((USER, "not-the-password"))).await;
    assert!(
        refused.starts_with("-ERR 'Authorization Violation'"),
        "a wrong password: {refused}"
    );

    // a connection without credentials is stackly-readiness
    let mut anonymous = Nats::signed_in(client, None).await;
    assert_permission_violation(
        anonymous.publish_with_reply("orders.two", None, b"x").await,
        "a publish to an application subject",
    );
    assert_permission_violation(
        anonymous.subscribe(">").await.map(|_| ()),
        "a subscribe to >",
    );
    assert_permission_violation(
        anonymous
            .publish_with_reply("$JS.API.STREAM.CREATE.X", None, b"{}")
            .await,
        "a stream create",
    );
    anonymous
        .publish_with_reply("$JS.API.INFO", Some("orders.evil"), b"")
        .await
        .expect("the readiness user may publish $JS.API.INFO with any reply subject");
    let info = anonymous
        .request_in("_INBOX.stackly", "$JS.API.INFO", b"")
        .await
        .expect("the readiness user asks $JS.API.INFO from its inbox space");
    let info: serde_json::Value =
        serde_json::from_slice(&info.payload).expect("$JS.API.INFO answers JSON");
    assert_eq!(
        info["streams"], 0,
        "the readiness account sees no stream: {info}"
    );
    // The reply to orders.evil stayed in the readiness account.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        app.stream_messages("ORDERS").await,
        1,
        "a reply the readiness user aimed at orders.evil never reached ORDERS"
    );

    // no leak: process, files, log store, monitoring, connection string
    let shown = output_of(
        "/bin/ps",
        &["-E", "-ww", "-o", "command=", "-p", &pid.to_string()],
    );
    assert!(shown.contains("nats-server"), "{shown}");
    assert!(
        !shown.contains(PASSWORD_A),
        "argv or env holds the password"
    );
    let conf = site.nats_conf(id);
    let mut found = Vec::new();
    files_holding(&site.data_root, PASSWORD_A.as_bytes(), &conf, &mut found);
    files_holding(
        &site.instance_paths(id).run,
        PASSWORD_A.as_bytes(),
        &conf,
        &mut found,
    );
    assert!(found.is_empty(), "files holding the password: {found:#?}");
    for page in MONITORING {
        let body = monitor_get(monitoring, page);
        assert!(!body.contains(PASSWORD_A), "{page} shows the password");
    }
    let detail = api.detail(id).await.expect("instance.get_detail");
    let uri = detail
        .connections
        .iter()
        .find(|entry| entry.name == "uri")
        .expect("the uri entry");
    let masked = uri.masked.clone().expect("the uri is available");
    assert!(
        masked.starts_with("nats://stackly:****@127.0.0.1:"),
        "{masked}"
    );
    assert!(!masked.contains(PASSWORD_A));
    let view = Cfg { api: &api }.view(id).await;
    assert!(
        view.effective_files.is_empty(),
        "{:#?}",
        view.effective_files
    );
    assert!(view.drift.is_none(), "{:#?}", view.drift);

    // readiness never reads the password
    let before = varz(monitoring)["total_connections"]
        .as_u64()
        .expect("/varz counts connections");
    let reads = site.secrets.count(SecretOp::Read);
    tokio::time::sleep(WATCH).await;
    let after = varz(monitoring)["total_connections"]
        .as_u64()
        .expect("/varz counts connections");
    assert!(
        after >= before + 3,
        "the probe kept connecting: {before} -> {after}"
    );
    assert_eq!(
        site.secrets.count(SecretOp::Read),
        reads,
        "a probe read a secret"
    );
    assert_eq!(
        api.get(id).await.expect("instance.get").readiness.state,
        InstanceReadinessState::Ready
    );

    // without JetStream the readiness user has no rights at all
    let nojs = create_instance(&site, &api, "auth-nojs", json!({ "jetstream": false })).await;
    let nojs_running = api.start_ready(nojs).await;
    assert_eq!(nojs_running.readiness.reason, "probe_ok");
    let mut bare = Nats::signed_in(loopback(port_of(&nojs_running, "client")), None).await;
    assert_permission_violation(
        bare.publish_with_reply("$JS.API.INFO", None, b"").await,
        "$JS.API.INFO without JetStream",
    );

    // a password the escaper has to carry
    let escaped = create_instance_with_password(
        &site,
        &api,
        "auth-escaped",
        json!({ "jetstream": false }),
        PASSWORD_ESCAPED,
    )
    .await;
    let escaped_running = api.start_ready(escaped).await;
    let escaped_client = loopback(port_of(&escaped_running, "client"));
    let (_, first) = Nats::connect(escaped_client, Some((USER, PASSWORD_ESCAPED))).await;
    assert_eq!(first, "PONG", "the escaped password signs in");
    let (_, wrong) = Nats::connect(escaped_client, Some((USER, "p\"q\\r"))).await;
    assert!(
        wrong.starts_with("-ERR 'Authorization Violation'"),
        "{wrong}"
    );

    drop((app, anonymous, bare));
    stop_instance(&api, id).await;
    assert!(!conf.exists(), "a clean stop removes nats.conf");
    stop_instance(&api, nojs).await;
    stop_instance(&api, escaped).await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "NATS {version}: stackly signed in, wrong password and the readiness user refused where they must be, no leak, {} probe connections in {WATCH:?} with no secret read, escaped password signed in",
        after - before
    ));
}

/// The JetStream API subjects a connection without credentials aims a reply
/// at, so the server would write its answer there if both users shared an
/// account (spec 0039, fact 12): a subject an application stream captures,
/// and the APIs that create, purge, or delete a stream or a message.
const INJECTED_REPLIES: [&str; 5] = [
    "orders.evil",
    "$JS.API.STREAM.CREATE.EVIL",
    "$JS.API.STREAM.PURGE.ORDERS",
    "$JS.API.STREAM.DELETE.ORDERS",
    "$JS.API.STREAM.MSG.DELETE.ORDERS",
];

/// What a connection without credentials may not subscribe to.
const REFUSED_SUBSCRIPTIONS: [&str; 5] = ["orders.>", "$JS.API.>", "_INBOX.>", "$SYS.>", ">"];

/// What a connection without credentials may not publish to.
const REFUSED_PUBLISHES: [&str; 8] = [
    "orders.two",
    "$JS.API.STREAM.INFO.ORDERS",
    "$JS.API.STREAM.MSG.GET.ORDERS",
    "$JS.API.STREAM.NAMES",
    "$JS.API.STREAM.PURGE.ORDERS",
    "$JS.API.STREAM.DELETE.ORDERS",
    "$SYS.REQ.SERVER.PING",
    "_INBOX.stackly.x",
];

// covers: AC-3, AC-4 (spec 0039)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed NATS 2.15.1 artifact in the runtime cache"]
async fn a_connection_without_credentials_never_reads_or_changes_the_app_account() {
    let ev = evidence!(
        covers = ["REQ-036"],
        kind = Kind::Native,
        expected = "on a ready NATS 2.15.1 instance of revision 1.1.0 with JetStream, a connection without credentials that aims $JS.API.INFO replies at orders.evil and at the stream create, purge, delete, and message delete APIs leaves ORDERS with its one message and the stackly user with the one stream ORDERS; it is refused with Permissions Violation for every application, JetStream stream, system, and wide inbox subject; its _INBOX.stackly.> subscription never receives what the stackly user publishes there; and an empty password, a password for stackly-readiness, and an unknown user are refused with Authorization Violation",
    );
    let Some(site) = Site::open(&ev, Serve::InProcess) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let id = create_instance(&site, &api, "inject", json!({})).await;
    let running = api.start_ready(id).await;
    let client = loopback(port_of(&running, "client"));
    let version = varz(port_of(&running, "monitoring"))["version"]
        .as_str()
        .expect("/varz reports the version")
        .to_owned();
    site.subject(&ev, &version);
    let mut app = Nats::signed_in(client, Some((USER, PASSWORD_A))).await;
    app.stream_create("ORDERS", "orders.>", "file")
        .await
        .expect("the stackly user creates a stream");
    app.publish("orders.one", b"first")
        .await
        .expect("the stackly user publishes");
    let mut anonymous = Nats::signed_in(client, None).await;

    // replies aimed at the application account
    for reply in INJECTED_REPLIES {
        anonymous
            .publish_with_reply("$JS.API.INFO", Some(reply), b"")
            .await
            .unwrap_or_else(|error| panic!("$JS.API.INFO with reply {reply}: {error}"));
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        app.stream_messages("ORDERS").await,
        1,
        "ORDERS kept its one message"
    );
    assert_eq!(app.stream_get("ORDERS", 1).await, b"first");
    let names = app
        .api("$JS.API.STREAM.NAMES", &json!({}))
        .await
        .expect("the stackly user lists its streams");
    assert_eq!(names["streams"], json!(["ORDERS"]), "{names}");

    // subjects out of reach
    for subject in REFUSED_SUBSCRIPTIONS {
        assert_permission_violation(
            anonymous.subscribe(subject).await.map(|_| ()),
            &format!("a subscribe to {subject}"),
        );
    }
    for subject in REFUSED_PUBLISHES {
        assert_permission_violation(
            anonymous
                .publish_with_reply(subject, Some("_INBOX.stackly.r"), b"")
                .await,
            &format!("a publish to {subject}"),
        );
    }

    // the inbox space of the readiness user is its own
    let inbox = anonymous
        .subscribe("_INBOX.stackly.>")
        .await
        .expect("the readiness user subscribes to its inbox space");
    app.publish_with_reply("_INBOX.stackly.leak", None, b"app-only")
        .await
        .expect("the stackly user publishes to any subject");
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The daemon's probe also asks `$JS.API.INFO` from `_INBOX.stackly.*` of
    // the readiness account, so its replies may arrive here (a residual the
    // spec states); nothing the stackly user sent may.
    let leaked: Vec<_> = anonymous
        .deliveries(inbox)
        .await
        .into_iter()
        .filter(|message| {
            message.subject == "_INBOX.stackly.leak" || message.payload == b"app-only"
        })
        .collect();
    assert!(leaked.is_empty(), "the readiness user received {leaked:?}");

    // credentials that are not the instance password
    for (user, password) in [
        (USER, ""),
        ("stackly-readiness", "any-password"),
        ("root", PASSWORD_A),
    ] {
        let (_, refused) = Nats::connect(client, Some((user, password))).await;
        assert!(
            refused.starts_with("-ERR 'Authorization Violation'"),
            "{user} with {} characters: {refused}",
            password.len()
        );
    }

    drop((app, anonymous));
    stop_instance(&api, id).await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "NATS {version}: {} injected replies left ORDERS at 1 message, {} subscribes and {} publishes refused, no delivery across accounts, 3 foreign credentials refused",
        INJECTED_REPLIES.len(),
        REFUSED_SUBSCRIPTIONS.len(),
        REFUSED_PUBLISHES.len()
    ));
}
