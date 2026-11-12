//! `instance::open_with::render` (spec 0070 AC-2 and AC-13): the `openWith`
//! entries of the repo packages, rendered with a `PlanContext` the way the
//! daemon builds it for `instance.get_detail`. The states follow the order
//! of spec 0021, and the golden files hold every rendered value.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use stackly_core::instance::connection::ConnectionUnavailable;
use stackly_core::instance::open_with::{OpenActionState, OpenTarget, RenderedOpenAction, render};
use stackly_core::provider::{
    NameCheck, OpenWithKind, PackageFiles, PlanContext, ResolvedEndpoint, RuntimeContext,
    SettingsValues, ValidatedPackage,
};
use stackly_domain::{
    Endpoint, Instance, InstanceId, InstanceLifecycle, IpEndpoint, OperationId, Port, PortRequest,
    SecretFieldName, SecretRef, Transport, UnixEndpoint,
};
use stackly_platform::{DirectoryLayout, RootSet};
use stackly_testkit::{InstanceBuilder, RuntimeBuilder};

const ID: &str = "0192f000-0000-7000-8000-00000000000a";

/// The files of a repo package, with `extra` appended to its manifest.
fn package_files(name: &str, extra: &str) -> BTreeMap<String, Vec<u8>> {
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
    files
        .get_mut("provider.yaml")
        .expect("a manifest")
        .extend_from_slice(extra.as_bytes());
    files
}

fn repo_package(name: &str) -> ValidatedPackage {
    validated(name, "")
}

/// A repo package whose manifest has `extra` appended, so its digest
/// differs from the repo one.
fn validated(name: &str, extra: &str) -> ValidatedPackage {
    ValidatedPackage::from_files(
        PackageFiles::new(name, package_files(name, extra)),
        NameCheck::FolderName,
    )
    .unwrap_or_else(|err| panic!("{name} is valid: {:#?}", err.issues))
}

fn port(number: u16) -> Port {
    Port::new(number).expect("a test port is not zero")
}

/// The golden context and instance of a repo package: every IP endpoint on
/// its default host with an `Auto` port bound from 14000 (or no port when
/// `bound` is false), a unix endpoint for the others, and the given
/// lifecycle.
fn setup(
    package: &ValidatedPackage,
    bound: bool,
    lifecycle: InstanceLifecycle,
) -> (PlanContext, Instance) {
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
                let number = bound.then(|| port(next));
                resolved.insert(
                    spec.name().clone(),
                    ResolvedEndpoint::Ip { host, port: number },
                );
                endpoints.push(Endpoint::Ip(IpEndpoint::new(
                    spec.name().clone(),
                    Transport::Tcp,
                    host,
                    PortRequest::Auto,
                    number,
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
    let builder = InstanceBuilder::new(&runtime, package.key())
        .name("golden")
        .endpoints(endpoints);
    let instance = if lifecycle == InstanceLifecycle::Retained {
        builder
            .retained(
                OperationId::generate(),
                "2026-11-02T04:00:00Z".parse().expect("a time"),
            )
            .build()
    } else {
        builder
            .lifecycle(lifecycle)
            .data_initialized_at("2026-11-02T04:00:00Z".parse().expect("a time"))
            .build()
    };
    (context, instance)
}

/// The secret uses of the golden instance for `fields`.
fn refs(fields: &[&str]) -> Vec<(SecretFieldName, SecretRef)> {
    fields
        .iter()
        .map(|field| {
            (
                field.parse().expect("a field name"),
                format!("local:instance/{ID}/{}", kebab(field))
                    .parse()
                    .expect("a reference"),
            )
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

fn state_of<'a>(actions: &'a [RenderedOpenAction], name: &str) -> &'a OpenActionState {
    &actions
        .iter()
        .find(|action| action.name == name)
        .unwrap_or_else(|| panic!("no action {name}"))
        .state
}

/// One action as the golden file writes it.
fn action_json(action: &RenderedOpenAction) -> Value {
    let kind = match action.kind {
        OpenWithKind::App => "app",
        OpenWithKind::Terminal => "terminal",
        OpenWithKind::Browser => "browser",
        OpenWithKind::Reveal => "reveal",
        _ => "unrecognized",
    };
    let value = match &action.state {
        OpenActionState::Available(OpenTarget::App { bundle_ids, url }) => {
            json!({ "bundleIds": bundle_ids, "url": url })
        }
        OpenActionState::Available(OpenTarget::Browser { url }) => json!({ "url": url }),
        OpenActionState::Available(OpenTarget::Terminal(command)) => json!({
            "executable": command.executable,
            "args": command.args,
            "env": command.env,
        }),
        OpenActionState::Available(OpenTarget::Reveal { path }) => {
            json!({ "path": path.to_str().expect("a UTF-8 path") })
        }
        other => panic!("{} is not available: {other:?}", action.name),
    };
    json!({
        "name": action.name,
        "title": action.title,
        "kind": kind,
        "value": value,
        "copySecret": action.copy_secret.as_ref().map(|(field, reference)| {
            json!({ "field": field.as_str(), "reference": reference.as_str() })
        }),
    })
}

/// Compares `actual` with a golden file, or rewrites it when
/// `STACKLY_UPDATE_GOLDEN=1`.
fn assert_golden(package: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/provider")
        .join(package)
        .join("open_with.json");
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

fn golden(name: &str, secret_fields: &[&str]) -> Value {
    let package = repo_package(name);
    let (context, instance) = setup(&package, true, InstanceLifecycle::Running);
    render(&package, &context, &instance, &refs(secret_fields))
        .iter()
        .map(action_json)
        .collect()
}

#[test]
fn the_repo_packages_render_their_open_actions_as_the_golden_files() {
    assert_golden("postgresql", &golden("postgresql", &["superuserPassword"]));
    assert_golden("nats", &golden("nats", &[]));
    assert_golden("traefik", &golden("traefik", &[]));
    assert_golden("fake", &golden("fake", &["authToken"]));
    assert_golden("clickhouse", &golden("clickhouse", &["defaultPassword"]));
}

#[test]
fn actions_come_in_manifest_order_and_never_hold_a_secret_value() {
    let package = repo_package("postgresql");
    let (context, instance) = setup(&package, true, InstanceLifecycle::Running);

    let actions = render(&package, &context, &instance, &refs(&["superuserPassword"]));

    let names: Vec<&str> = actions.iter().map(|action| action.name.as_str()).collect();
    assert_eq!(names, ["tableplus", "postico", "psql"]);
    for action in &actions {
        let (field, reference) = action.copy_secret.as_ref().expect("a copy secret");
        assert_eq!(field.as_str(), "superuserPassword");
        assert!(reference.as_str().starts_with("local:instance/"));
        let OpenActionState::Available(target) = &action.state else {
            panic!("{} is not available", action.name);
        };
        assert!(!format!("{target:?}").contains("secret"), "{target:?}");
    }
}

#[test]
fn the_terminal_command_takes_the_absolute_runtime_executable_and_raw_values() {
    let package = repo_package("postgresql");
    let (context, instance) = setup(&package, true, InstanceLifecycle::Running);

    let actions = render(&package, &context, &instance, &refs(&["superuserPassword"]));

    let OpenActionState::Available(OpenTarget::Terminal(command)) = state_of(&actions, "psql")
    else {
        panic!("psql is a terminal action");
    };
    assert_eq!(
        command.executable,
        "/private/tmp/skt-golden/runtime/bin/psql"
    );
    assert_eq!(
        command.args[0],
        format!("--host={}", context.paths.run.to_str().expect("UTF-8"))
    );
    assert_eq!(command.args[1], "--port=14000");
    assert_eq!(
        command.env,
        [("PGAPPNAME".to_owned(), "stackly-terminal".to_owned())]
    );
}

#[test]
fn a_retained_instance_makes_every_kind_retained_reveal_included() {
    let package = repo_package("fake");
    let (context, instance) = setup(&package, true, InstanceLifecycle::Retained);

    let actions = render(&package, &context, &instance, &refs(&["authToken"]));

    assert_eq!(actions.len(), 4);
    for action in &actions {
        assert_eq!(
            action.state,
            OpenActionState::Unavailable(ConnectionUnavailable::Retained),
            "{}",
            action.name
        );
        assert!(action.copy_secret.is_none());
    }
}

#[test]
fn a_package_other_than_the_pinned_one_is_package_unavailable() {
    let pinned = repo_package("fake");
    let other = validated("fake", "# another revision\n");
    let (context, instance) = setup(&pinned, true, InstanceLifecycle::Running);

    let actions = render(&other, &context, &instance, &refs(&["authToken"]));

    for action in &actions {
        assert_eq!(
            action.state,
            OpenActionState::Unavailable(ConnectionUnavailable::PackageUnavailable)
        );
    }
}

#[test]
fn port_not_assigned_only_hits_the_entries_that_name_an_endpoint() {
    let package = repo_package("fake");
    let (context, instance) = setup(&package, false, InstanceLifecycle::Stopped);

    let actions = render(&package, &context, &instance, &refs(&["authToken"]));

    for name in ["client", "status"] {
        assert_eq!(
            state_of(&actions, name),
            &OpenActionState::Unavailable(ConnectionUnavailable::PortNotAssigned),
            "{name}"
        );
    }
    for name in ["version", "notes"] {
        assert!(
            matches!(state_of(&actions, name), OpenActionState::Available(_)),
            "{name}"
        );
    }
}

#[test]
fn a_copy_secret_without_a_use_makes_the_entry_package_unavailable() {
    let package = repo_package("fake");
    let (context, instance) = setup(&package, true, InstanceLifecycle::Running);

    let actions = render(&package, &context, &instance, &[]);

    for name in ["client", "version"] {
        assert_eq!(
            state_of(&actions, name),
            &OpenActionState::Unavailable(ConnectionUnavailable::PackageUnavailable),
            "{name}"
        );
    }
    assert!(matches!(
        state_of(&actions, "status"),
        OpenActionState::Available(_)
    ));
}
