//! `instance::connection::render` (spec 0021 AC-2, and the render half of
//! AC-3): the `connection` entries of the `fake-command` fixture package
//! plus a block each test adds, rendered with a `PlanContext`. The cases of
//! `protocols/fixtures/instance/connection-compose.json` give the expected
//! text parts, the same ones `compose` in `stackly-protocol` and
//! `ConnectionComposer` in Swift are held to.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use stackly_core::instance::connection::{
    ConnectionPart, ConnectionState, ConnectionUnavailable, RenderedConnection, render,
};
use stackly_core::provider::{
    NameCheck, PackageFiles, PlanContext, ResolvedEndpoint, RuntimeContext, SettingsValues,
    ValidatedPackage,
};
use stackly_domain::{
    Endpoint, Instance, InstanceId, InstanceLifecycle, IpEndpoint, OperationId, Port, PortRequest,
    SecretFieldName, SecretRef, Transport, UnixEndpoint,
};
use stackly_platform::{DirectoryLayout, RootSet};
use stackly_testkit::{InstanceBuilder, RuntimeBuilder};

const ID: &str = "0192f000-0000-7000-8000-00000000000a";
const SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-00000000000a/auth-token";
const RUN_DIR: &str = "/Volumes/Test Root/run";
const URI: &str = "fake://user:${secret.authToken}@localhost/db?port=${endpoint.http.port}";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/fake-command")
}

/// The `fake-command` package with one `connection` block appended, each
/// entry `(name, template)` titled after its name.
fn package(entries: &[(&str, &str)]) -> ValidatedPackage {
    let dir = fixture_dir();
    let mut files = BTreeMap::new();
    for path in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        files.insert(
            path.to_owned(),
            fs::read(dir.join(path)).expect("package file"),
        );
    }
    for entry in fs::read_dir(dir.join("templates")).expect("templates folder") {
        let entry = entry.expect("template entry");
        let name = entry.file_name().into_string().expect("UTF-8 name");
        files.insert(
            format!("templates/{name}"),
            fs::read(entry.path()).expect("template"),
        );
    }
    let mut block = String::from("\nconnection:\n");
    for (name, template) in entries {
        block.push_str(&format!(
            "  - name: {name}\n    title: Title of {name}\n    template: {}\n",
            serde_json::to_string(template).expect("a string serializes")
        ));
    }
    files
        .get_mut("provider.yaml")
        .expect("a manifest")
        .extend_from_slice(block.as_bytes());
    ValidatedPackage::from_files(
        PackageFiles::new("fake-command", files),
        NameCheck::FolderName,
    )
    .unwrap_or_else(|err| panic!("the package is valid: {:#?}", err.issues))
}

fn port(number: u16) -> Port {
    Port::new(number).expect("a test port is not zero")
}

/// A context with the given `http` host and port, and `run` as the run
/// folder.
fn context(address: &str, http_port: Option<u16>, run: &str) -> PlanContext {
    let base = Path::new("/Volumes/Test Root");
    let roots = RootSet::new(
        base.join("data"),
        base.join("logs"),
        base.join("cache"),
        PathBuf::from("/private/tmp/skt-render/stackly-0000000a"),
    )
    .expect("the roots are valid");
    let layout = DirectoryLayout::new(roots);
    let id: InstanceId = ID.parse().expect("a UUID v7");
    let mut paths = layout.instance_paths(id);
    paths.run = PathBuf::from(run);
    let mut endpoints = BTreeMap::new();
    endpoints.insert(
        "http".parse().expect("endpoint name"),
        ResolvedEndpoint::Ip {
            host: address.parse::<IpAddr>().expect("an address"),
            port: http_port.map(port),
        },
    );
    endpoints.insert(
        "control".parse().expect("endpoint name"),
        ResolvedEndpoint::Unix,
    );
    PlanContext {
        instance_id: id,
        instance_name: "orders-db".parse().expect("valid name"),
        paths,
        runtime: RuntimeContext {
            root: PathBuf::from("/private/tmp/skt-render/runtime"),
            version: "0.1.0".parse().expect("valid version"),
            line: "0.1".parse().expect("valid line"),
        },
        endpoints,
        settings: SettingsValues::default(),
    }
}

/// What the instance's `http` endpoint asks for and has.
struct Http {
    request: PortRequest,
    bound: Option<u16>,
}

impl Http {
    fn auto() -> Self {
        Self {
            request: PortRequest::Auto,
            bound: None,
        }
    }

    fn fixed(number: u16) -> Self {
        Self {
            request: PortRequest::Fixed(port(number)),
            bound: None,
        }
    }
}

fn instance(package: &ValidatedPackage, http: &Http, lifecycle: InstanceLifecycle) -> Instance {
    let runtime = RuntimeBuilder::new().build();
    let endpoints = vec![
        Endpoint::Ip(IpEndpoint::new(
            "http".parse().expect("endpoint name"),
            Transport::Tcp,
            "127.0.0.1".parse().expect("an address"),
            http.request,
            http.bound.map(port),
        )),
        Endpoint::Unix(UnixEndpoint::new(
            "control".parse().expect("endpoint name"),
            "fake.14000".parse().expect("socket name"),
        )),
    ];
    let builder = InstanceBuilder::new(&runtime, package.key())
        .name("orders-db")
        .endpoints(endpoints);
    if lifecycle == InstanceLifecycle::Retained {
        builder
            .retained(
                OperationId::generate(),
                "2026-10-22T04:00:00Z".parse().expect("a time"),
            )
            .build()
    } else {
        builder
            .lifecycle(lifecycle)
            .data_initialized_at("2026-10-22T04:00:00Z".parse().expect("a time"))
            .build()
    }
}

fn secrets() -> Vec<(SecretFieldName, SecretRef)> {
    vec![(
        "authToken".parse().expect("a field name"),
        SECRET_REF.parse().expect("a reference"),
    )]
}

/// The parts of an available entry as the fixture writes them.
fn parts_json(entry: &RenderedConnection) -> Value {
    let ConnectionState::Available { parts, .. } = &entry.state else {
        panic!("{} is not available: {:?}", entry.name, entry.state);
    };
    parts
        .iter()
        .map(|part| match part {
            ConnectionPart::Text(text) => json!({ "kind": "text", "text": text }),
            ConnectionPart::Secret { field, reference } => json!({
                "kind": "secret",
                "secret": { "field": field.as_str(), "reference": reference.as_str() },
            }),
            other => panic!("unexpected part {other:?}"),
        })
        .collect()
}

fn state_of(entries: &[RenderedConnection], name: &str) -> ConnectionState {
    entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("no entry {name}"))
        .state
        .clone()
}

#[test]
fn every_fixture_render_case_gives_the_expected_text_parts() {
    let text = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/instance/connection-compose.json"),
    )
    .expect("the shared fixture exists");
    let fixture: Value = serde_json::from_str(&text).expect("the fixture is JSON");
    assert_eq!(fixture["secretReference"], SECRET_REF);
    let cases = fixture["render"].as_array().expect("render cases");
    assert!(cases.len() >= 4, "the fixture covers the render cases");

    for case in cases {
        let field = |name: &str| case[name].as_str().expect("a text field");
        let package = package(&[("uri", field("template"))]);
        let http = u16::try_from(case["port"].as_u64().expect("a port")).expect("a port");
        let mut context = context(field("address"), Some(http), field("runDir"));
        context.instance_name = field("instanceName").parse().expect("valid name");
        let instance = instance(&package, &Http::fixed(http), InstanceLifecycle::Running);

        let entries = render(&package, &context, &instance, &secrets());

        assert_eq!(entries.len(), 1, "{}", field("name"));
        assert_eq!(
            parts_json(&entries[0]),
            case["expectedParts"],
            "{}",
            field("name")
        );
    }
}

#[test]
fn an_entry_has_masked_text_and_parts_in_order_with_the_secret_as_a_reference() {
    let package = package(&[("uri", URI)]);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let instance = instance(&package, &Http::fixed(14000), InstanceLifecycle::Running);

    let entries = render(&package, &context, &instance, &secrets());

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "uri");
    assert_eq!(entries[0].title, "Title of uri");
    assert!(!entries[0].port_may_change);
    assert_eq!(
        entries[0].state,
        ConnectionState::Available {
            masked: "fake://user:****@localhost/db?port=14000".to_owned(),
            parts: vec![
                ConnectionPart::Text("fake://user:".to_owned()),
                ConnectionPart::Secret {
                    field: "authToken".parse().unwrap(),
                    reference: SECRET_REF.parse().unwrap(),
                },
                ConnectionPart::Text("@localhost/db?port=14000".to_owned()),
            ],
        }
    );
}

#[test]
fn entries_come_in_manifest_order_and_an_entry_without_a_secret_has_one_text_part() {
    let package = package(&[
        ("zeta", "http://${endpoint.http.address}/z"),
        ("alpha", URI),
    ]);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let instance = instance(&package, &Http::fixed(14000), InstanceLifecycle::Running);

    let entries = render(&package, &context, &instance, &secrets());

    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["zeta", "alpha"]);
    assert_eq!(
        entries[0].state,
        ConnectionState::Available {
            masked: "http://127.0.0.1:14000/z".to_owned(),
            parts: vec![ConnectionPart::Text("http://127.0.0.1:14000/z".to_owned())],
        }
    );
}

#[test]
fn a_fixed_port_is_available_and_never_changes() {
    let package = package(&[("uri", URI)]);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let stopped = instance(&package, &Http::fixed(14000), InstanceLifecycle::Stopped);

    let entries = render(&package, &context, &stopped, &secrets());

    assert!(matches!(
        entries[0].state,
        ConnectionState::Available { .. }
    ));
    assert!(!entries[0].port_may_change, "a fixed port stays");
}

#[test]
fn an_auto_port_may_change_unless_the_instance_is_running() {
    let package = package(&[("uri", URI)]);
    let mut http = Http::auto();
    http.bound = Some(14000);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let mut results = Vec::new();
    for lifecycle in [
        InstanceLifecycle::Running,
        InstanceLifecycle::Stopped,
        InstanceLifecycle::Starting,
    ] {
        let instance = instance(&package, &http, lifecycle);
        let entries = render(&package, &context, &instance, &secrets());
        assert!(matches!(
            entries[0].state,
            ConnectionState::Available { .. }
        ));
        results.push(entries[0].port_may_change);
    }

    assert_eq!(results, [false, true, true]);
}

#[test]
fn an_auto_port_that_is_not_bound_is_port_not_assigned() {
    let package = package(&[("uri", URI), ("addr", "http://${endpoint.http.address}/")]);
    let context = context("127.0.0.1", None, RUN_DIR);
    let instance = instance(&package, &Http::auto(), InstanceLifecycle::Stopped);

    let entries = render(&package, &context, &instance, &secrets());

    for entry in &entries {
        assert_eq!(
            entry.state,
            ConnectionState::Unavailable(ConnectionUnavailable::PortNotAssigned),
            "{}",
            entry.name
        );
        assert!(entry.port_may_change, "{}", entry.name);
    }
}

#[test]
fn an_entry_that_names_no_port_is_available_while_another_waits_for_one() {
    let package = package(&[
        (
            "run_dir",
            "fake://localhost/?dir=${endpoint.control.socketDir}",
        ),
        ("uri", URI),
    ]);
    let context = context("127.0.0.1", None, RUN_DIR);
    let instance = instance(&package, &Http::auto(), InstanceLifecycle::Stopped);

    let entries = render(&package, &context, &instance, &secrets());

    assert!(matches!(
        state_of(&entries, "run_dir"),
        ConnectionState::Available { .. }
    ));
    assert_eq!(
        state_of(&entries, "uri"),
        ConnectionState::Unavailable(ConnectionUnavailable::PortNotAssigned)
    );
    assert!(!entries[0].port_may_change, "it names no Auto endpoint");
}

#[test]
fn a_socket_path_named_after_an_auto_port_waits_for_the_port() {
    let package = package(&[(
        "socket",
        "fake://localhost/?socket=${endpoint.control.socket}",
    )]);
    let unbound = instance(&package, &Http::auto(), InstanceLifecycle::Stopped);

    let waiting = render(
        &package,
        &context("127.0.0.1", None, RUN_DIR),
        &unbound,
        &secrets(),
    );

    assert_eq!(
        waiting[0].state,
        ConnectionState::Unavailable(ConnectionUnavailable::PortNotAssigned)
    );
    assert!(
        waiting[0].port_may_change,
        "the socket is named after the Auto port"
    );
}

#[test]
fn a_package_other_than_the_pinned_one_is_package_unavailable_for_every_entry() {
    let pinned = package(&[("uri", URI)]);
    let other = package(&[("uri", URI), ("more", "http://${endpoint.http.address}/")]);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let instance = instance(&pinned, &Http::fixed(14000), InstanceLifecycle::Running);

    let entries = render(&other, &context, &instance, &secrets());

    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert_eq!(
            entry.state,
            ConnectionState::Unavailable(ConnectionUnavailable::PackageUnavailable)
        );
    }
}

#[test]
fn a_secret_without_a_use_makes_the_entry_package_unavailable() {
    let package = package(&[("uri", URI), ("addr", "http://${endpoint.http.address}/")]);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let instance = instance(&package, &Http::fixed(14000), InstanceLifecycle::Running);

    let entries = render(&package, &context, &instance, &[]);

    assert_eq!(
        state_of(&entries, "uri"),
        ConnectionState::Unavailable(ConnectionUnavailable::PackageUnavailable)
    );
    assert!(matches!(
        state_of(&entries, "addr"),
        ConnectionState::Available { .. }
    ));
}

#[test]
fn the_reasons_are_tried_retained_then_package_then_port() {
    let pinned = package(&[("uri", URI)]);
    let other = package(&[("uri", URI), ("more", "http://${endpoint.http.address}/")]);
    let no_port = context("127.0.0.1", None, RUN_DIR);

    let retained = instance(&pinned, &Http::auto(), InstanceLifecycle::Retained);
    let live = instance(&pinned, &Http::auto(), InstanceLifecycle::Stopped);

    let retained_other = render(&other, &no_port, &retained, &secrets());
    let live_other = render(&other, &no_port, &live, &secrets());
    let live_pinned = render(&pinned, &no_port, &live, &secrets());

    for entry in &retained_other {
        assert_eq!(
            entry.state,
            ConnectionState::Unavailable(ConnectionUnavailable::Retained)
        );
    }
    for entry in &live_other {
        assert_eq!(
            entry.state,
            ConnectionState::Unavailable(ConnectionUnavailable::PackageUnavailable)
        );
    }
    assert_eq!(
        live_pinned[0].state,
        ConnectionState::Unavailable(ConnectionUnavailable::PortNotAssigned)
    );
}

#[test]
fn a_package_without_a_connection_block_renders_no_entries() {
    let bare = package(&[]);
    let context = context("127.0.0.1", Some(14000), RUN_DIR);
    let instance = instance(&bare, &Http::fixed(14000), InstanceLifecycle::Running);

    assert!(render(&bare, &context, &instance, &secrets()).is_empty());
}

/// Reads a package of the repo like `read_package` would.
fn repo_package(name: &str) -> ValidatedPackage {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../providers")
        .join(name);
    let mut files = BTreeMap::new();
    for path in [
        "provider.yaml",
        "versions.json",
        "schemas/settings.schema.json",
    ] {
        files.insert(
            path.to_owned(),
            fs::read(dir.join(path)).expect("package file"),
        );
    }
    for entry in fs::read_dir(dir.join("templates")).expect("templates folder") {
        let entry = entry.expect("template entry");
        let file = entry.file_name().into_string().expect("UTF-8 name");
        files.insert(
            format!("templates/{file}"),
            fs::read(entry.path()).expect("template"),
        );
    }
    ValidatedPackage::from_files(PackageFiles::new(name, files), NameCheck::FolderName)
        .unwrap_or_else(|err| panic!("{name} is valid: {:#?}", err.issues))
}

/// The golden context of spec 0009 for a repo package, with every IP
/// endpoint on its default host and an `Auto` port bound from 14000, and the
/// instance that goes with it, stopped.
fn golden_entries(name: &str, secret_fields: &[&str]) -> Value {
    let package = repo_package(name);
    let roots = RootSet::new(
        PathBuf::from("/Volumes/Test Root/\u{dc}n\u{ef} c\u{f6}d\u{e9}/data"),
        PathBuf::from("/Volumes/Test Root/\u{dc}n\u{ef} c\u{f6}d\u{e9}/logs"),
        PathBuf::from("/Volumes/Test Root/\u{dc}n\u{ef} c\u{f6}d\u{e9}/cache"),
        PathBuf::from("/private/tmp/skt-golden/stackly-0000000a"),
    )
    .expect("the golden roots are valid");
    let layout = DirectoryLayout::new(roots);
    let id: InstanceId = ID.parse().expect("a UUID v7");
    let mut next = 14000;
    let mut resolved = BTreeMap::new();
    let mut endpoints = Vec::new();
    for spec in package.endpoints() {
        match spec.default_host() {
            Some(host) => {
                resolved.insert(
                    spec.name().clone(),
                    ResolvedEndpoint::Ip {
                        host,
                        port: Some(port(next)),
                    },
                );
                endpoints.push(Endpoint::Ip(IpEndpoint::new(
                    spec.name().clone(),
                    Transport::Tcp,
                    host,
                    PortRequest::Auto,
                    Some(port(next)),
                )));
                next += 1;
            }
            None => {
                resolved.insert(spec.name().clone(), ResolvedEndpoint::Unix);
                endpoints.push(Endpoint::Unix(UnixEndpoint::new(
                    spec.name().clone(),
                    "s.sock".parse().expect("socket name"),
                )));
            }
        }
    }
    let context = PlanContext {
        instance_id: id,
        instance_name: "golden".parse().expect("valid name"),
        paths: layout.instance_paths(id),
        runtime: RuntimeContext {
            root: PathBuf::from("/private/tmp/skt-golden/runtime"),
            version: "1.0.0".parse().expect("valid version"),
            line: "1".parse().expect("valid line"),
        },
        endpoints: resolved,
        settings: SettingsValues::default(),
    };
    let runtime = RuntimeBuilder::new().build();
    let instance = InstanceBuilder::new(&runtime, package.key())
        .name("golden")
        .endpoints(endpoints)
        .lifecycle(InstanceLifecycle::Stopped)
        .data_initialized_at("2026-10-22T04:00:00Z".parse().expect("a time"))
        .build();
    let refs: Vec<(SecretFieldName, SecretRef)> = secret_fields
        .iter()
        .map(|field| {
            (
                field.parse().expect("a field name"),
                format!("local:instance/{ID}/{}", kebab(field))
                    .parse()
                    .expect("a reference"),
            )
        })
        .collect();

    let entries = render(&package, &context, &instance, &refs);

    entries
        .iter()
        .map(|entry| {
            let ConnectionState::Available { masked, .. } = &entry.state else {
                panic!("{name} {} is not available: {:?}", entry.name, entry.state);
            };
            json!({
                "name": entry.name,
                "title": entry.title,
                "masked": masked,
                "parts": parts_json(entry),
                "portMayChange": entry.port_may_change,
            })
        })
        .collect()
}

/// `superuserPassword` as `superuser-password`, the segment of its reference.
fn kebab(field: &str) -> String {
    let mut out = String::new();
    for ch in field.chars() {
        if ch.is_ascii_uppercase() {
            out.push('-');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Compares `actual` with a golden file, or rewrites it when
/// `STACKLY_UPDATE_GOLDEN=1`.
fn assert_golden(package: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/provider")
        .join(package)
        .join("connection.json");
    let text = format!("{}\n", serde_json::to_string_pretty(actual).expect("JSON"));
    if std::env::var("STACKLY_UPDATE_GOLDEN").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().expect("a golden folder")).expect("the folder");
        fs::write(&path, &text).expect("the golden file can be written");
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "{}: {err}; run with STACKLY_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(
        expected,
        text,
        "{} differs from the golden file",
        path.display()
    );
}

#[test]
fn the_repo_packages_render_their_connections_as_the_golden_files() {
    assert_golden(
        "postgresql",
        &golden_entries("postgresql", &["superuserPassword"]),
    );
    assert_golden("nats", &golden_entries("nats", &[]));
    assert_golden("fake", &golden_entries("fake", &["authToken"]));
}
