//! AC-028 in full (spec 0020 AC-19): for a matrix of bad drafts, the params
//! the sheet sends through `stackly-ipc`'s `Client` and the params
//! `stackly instance create --json` sends get the same error from a real
//! `stacklyd`: the same code, category, details, and fields, and the CLI
//! writes exactly one envelope line.

use std::net::TcpListener;

use serde_json::{Value, json};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, PortRequest, ProviderSelector, SecretFieldInput,
    SettingsDocument,
};
use stackly_protocol::secret::SecretInput;
use stackly_protocol::{Call, ErrorBody, ErrorCategory};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, paths, run, uid};
use crate::stacklyd::seed;

/// What one bad draft is, written once for both paths.
struct Draft {
    label: &'static str,
    name: String,
    runtime: String,
    provider: &'static str,
    port: Option<(&'static str, PortRequest)>,
    settings: Value,
    secret_field: Option<&'static str>,
    /// The CLI flags that say the same as `settings` and `secret_field`.
    flags: Vec<String>,
}

impl Draft {
    fn new(label: &'static str, runtime: &str) -> Self {
        Self {
            label,
            name: "demo".to_owned(),
            runtime: runtime.to_owned(),
            provider: "testing/fake",
            port: None,
            settings: json!({}),
            secret_field: None,
            flags: Vec::new(),
        }
    }

    fn params(&self) -> CreateInstanceParams {
        CreateInstanceParams {
            name: self.name.clone(),
            display_name: None,
            provider: ProviderSelector {
                name: self.provider.to_owned(),
                revision: Some("0.3.0".to_owned()),
            },
            runtime_id: self.runtime.parse().expect("a runtime id"),
            endpoints: self
                .port
                .iter()
                .map(|(name, port)| EndpointRequest {
                    name: (*name).to_owned(),
                    port: *port,
                })
                .collect(),
            settings: SettingsDocument::new(self.settings.as_object().cloned().unwrap_or_default()),
            secrets: self
                .secret_field
                .iter()
                .map(|field| SecretFieldInput {
                    field: (*field).to_owned(),
                    input: SecretInput::Generate,
                })
                .collect(),
        }
    }

    fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "instance",
            "create",
            &self.name,
            "--runtime",
            &self.runtime,
            "--provider",
            self.provider,
            "--revision",
            "0.3.0",
        ]
        .map(str::to_owned)
        .to_vec();
        if let Some((name, port)) = self.port {
            let text = match port {
                PortRequest::Auto => "auto".to_owned(),
                PortRequest::Fixed(port) => port.to_string(),
            };
            args.extend(["--port".to_owned(), format!("{name}={text}")]);
        }
        if let Some(field) = self.secret_field {
            args.extend(["--secret".to_owned(), format!("{field}=generate")]);
        }
        args.extend(self.flags.iter().cloned());
        args.push("--json".to_owned());
        args
    }
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

fn client_error(socket: &std::path::Path, params: CreateInstanceParams) -> Box<ErrorBody> {
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
                        Call::InstanceCreate(params),
                        CallOptions::default().with_idempotency_key("parity-key-0001"),
                    )
                    .await
                    .expect_err("a bad draft is refused")
            })
    })
    .join()
    .expect("the client thread ends")
}

// covers: AC-028
#[test]
fn a_bad_draft_gets_the_same_error_through_the_client_and_through_the_cli() {
    let ev = evidence!(
        covers = ["AC-028"],
        kind = Kind::Fixture,
        expected = "for a bad name, port, setting, secret, runtime, and provider, the params sent through the Client and by stackly instance create --json get the same code, category, details, and fields from a real stacklyd, and the CLI writes exactly one envelope line without progress",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let held = TcpListener::bind(("127.0.0.1", 0)).expect("a free port binds");
    let port = held.local_addr().expect("an address").port();

    let mut drafts = Vec::new();
    let mut bad_name = Draft::new("name", &runtime);
    bad_name.name = "Bad_Name".to_owned();
    drafts.push(bad_name);
    let mut long_name = Draft::new("long name", &runtime);
    long_name.name = "n".repeat(64);
    drafts.push(long_name);
    let mut zero = Draft::new("port 0", &runtime);
    zero.port = Some(("http", PortRequest::Fixed(0)));
    drafts.push(zero);
    let mut taken = Draft::new("port in use", &runtime);
    taken.port = Some(("http", PortRequest::Fixed(port)));
    drafts.push(taken);
    let mut kind = Draft::new("setting of the wrong type", &runtime);
    kind.settings = json!({ "readyDelayMs": "abc" });
    kind.flags = vec!["--set-json".to_owned(), "readyDelayMs=\"abc\"".to_owned()];
    drafts.push(kind);
    let mut unknown = Draft::new("unknown setting", &runtime);
    unknown.settings = json!({ "nosuch": 1 });
    unknown.flags = vec!["--set-json".to_owned(), "nosuch=1".to_owned()];
    drafts.push(unknown);
    let mut in_settings = Draft::new("secret field in settings", &runtime);
    in_settings.settings = json!({ "authToken": "x" });
    in_settings.flags = vec!["--set-json".to_owned(), "authToken=\"x\"".to_owned()];
    drafts.push(in_settings);
    let mut secret = Draft::new("unknown secret field", &runtime);
    secret.secret_field = Some("nosuch");
    drafts.push(secret);
    drafts.push(Draft::new(
        "runtime that does not exist",
        "0192f3a5-ffff-7000-8000-00000000ffff",
    ));
    let mut provider = Draft::new("unknown provider", &runtime);
    provider.provider = "testing/nothing";
    drafts.push(provider);

    for draft in drafts {
        let from_client = client_error(daemon.socket(), draft.params());
        let args = draft.args();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let from_cli = run(root.path(), &refs);

        let value = envelope(&from_cli);
        assert_eq!(
            from_cli.stdout.trim_end().lines().count(),
            1,
            "{}: one envelope line",
            draft.label
        );
        assert!(from_cli.stderr.is_empty(), "{}: {from_cli:?}", draft.label);
        assert!(value["result"].is_null(), "{}: {value}", draft.label);
        let client_json = serde_json::to_value(&*from_client).expect("an error encodes");
        assert_eq!(
            essence(&value["error"]),
            essence(&client_json),
            "{}",
            draft.label
        );
        let exit = match from_client.category() {
            ErrorCategory::InvalidInput => 2,
            ErrorCategory::Conflict => 3,
            ErrorCategory::Unavailable => 6,
            ErrorCategory::NotFound => 10,
            other => panic!("{}: the matrix has no exit code for {other:?}", draft.label),
        };
        assert_eq!(from_cli.code, Some(exit), "{}", draft.label);
    }
    drop(held);
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
