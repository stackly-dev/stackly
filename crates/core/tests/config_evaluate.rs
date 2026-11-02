//! The config evaluator of spec 0029 (*Evaluator config*) over the real
//! fake, PostgreSQL, NATS, and Traefik packages: field codes, the setting
//! diff with keyed lists, the impact table, and the Effective File preview.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use stackly_core::instance::config::{
    ConfigImpact, changed_lines, diff_settings, evaluate_change, preview,
};
use stackly_core::instance::create::FieldCode;
use stackly_core::provider::{
    NameCheck, PackageFiles, RuntimeContext, SettingsValues, ValidatedPackage,
};
use stackly_domain::{
    Endpoint, Instance, InstanceId, InstanceLifecycle, IpEndpoint, NewInstance, OperationId,
    PortRequest, StartupPolicy, Transport,
};
use stackly_platform::{DirectoryLayout, RootSet};

const INSTANCE: &str = "0192f000-0000-7000-8000-00000000000a";

fn package(name: &str) -> ValidatedPackage {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../providers")
        .join(name);
    let mut files = BTreeMap::new();
    collect(&dir, &dir, &mut files);
    ValidatedPackage::from_files(PackageFiles::new(name, files), NameCheck::FolderName)
        .unwrap_or_else(|err| panic!("{err}: {:#?}", err.issues))
}

fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).expect("the fixture is valid") {
        let path = entry.expect("the fixture is valid").path();
        let relative = path
            .strip_prefix(root)
            .expect("the fixture is valid")
            .to_str()
            .expect("the fixture is valid")
            .to_owned();
        if relative == "README.md" || relative == "LICENSE-NOTICES.md" || relative == "tests" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            files.insert(
                relative,
                std::fs::read(&path).expect("the fixture is valid"),
            );
        }
    }
}

fn values(package: &ValidatedPackage, document: &serde_json::Value) -> SettingsValues {
    package
        .settings_schema()
        .validate(document)
        .expect("the fixture is valid")
}

fn with(base: &serde_json::Value, key: &str, value: serde_json::Value) -> serde_json::Value {
    let mut out = base.clone();
    out[key] = value;
    out
}

fn secret(field: &str) -> serde_json::Value {
    format!("local:instance/{INSTANCE}/{field}").into()
}

fn pg_base() -> serde_json::Value {
    serde_json::json!({ "superuserPassword": secret("superuser-password") })
}

fn codes(
    evaluation: &stackly_core::instance::config::ChangeEvaluation,
) -> Vec<(String, FieldCode)> {
    evaluation
        .field_issues
        .iter()
        .map(|issue| (issue.path.clone(), issue.code))
        .collect()
}

#[test]
fn postgresql_reloads_a_reload_field_restarts_a_restart_field_and_refuses_the_secret() {
    let package = package("postgresql");
    let schema = package.settings_schema();
    let active = values(&package, &pg_base());
    let running = InstanceLifecycle::Running;

    let reload = evaluate_change(
        schema,
        &active,
        &with(&pg_base(), "logMinDurationStatementMs", 250.into()),
        running,
        true,
        false,
    );
    assert!(reload.is_valid(), "{:?}", reload.field_issues);
    assert_eq!(reload.impact, ConfigImpact::Reload);
    assert_eq!(reload.changes.len(), 1);
    assert_eq!(reload.changes[0].key, "logMinDurationStatementMs");
    assert_eq!(reload.changes[0].old, Some((-1).into()));
    assert_eq!(reload.changes[0].new, Some(250.into()));

    let both = with(
        &with(&pg_base(), "logMinDurationStatementMs", 250.into()),
        "maxConnections",
        200.into(),
    );
    let restart = evaluate_change(schema, &active, &both, running, true, false);
    assert_eq!(restart.impact, ConfigImpact::Restart);
    let keys: Vec<&str> = restart
        .changes
        .iter()
        .map(|change| change.key.as_str())
        .collect();
    assert_eq!(
        keys,
        ["maxConnections", "logMinDurationStatementMs"],
        "schema order"
    );

    let password = evaluate_change(
        schema,
        &active,
        &serde_json::json!({ "superuserPassword": "local:erp/postgres/admin" }),
        running,
        true,
        false,
    );
    assert!(!password.is_valid());
    assert_eq!(
        codes(&password),
        [(
            "settings.superuserPassword".to_owned(),
            FieldCode::SecretReadOnly
        )]
    );

    let out_of_range = evaluate_change(
        schema,
        &active,
        &with(&pg_base(), "maxConnections", 4.into()),
        running,
        true,
        false,
    );
    assert_eq!(
        codes(&out_of_range),
        [("settings.maxConnections".to_owned(), FieldCode::OutOfRange)]
    );
    assert!(out_of_range.changes.is_empty());
}

#[test]
fn the_impact_follows_the_lifecycle_the_capability_and_the_ports() {
    let package = package("postgresql");
    let schema = package.settings_schema();
    let active = values(&package, &pg_base());
    let reloadable = with(&pg_base(), "logStatement", "all".into());
    let impact = |lifecycle, reload_supported, endpoints_changed, document: &serde_json::Value| {
        evaluate_change(
            schema,
            &active,
            document,
            lifecycle,
            reload_supported,
            endpoints_changed,
        )
        .impact
    };

    for lifecycle in [
        InstanceLifecycle::Stopped,
        InstanceLifecycle::Failed,
        InstanceLifecycle::Uninitialized,
    ] {
        assert_eq!(
            impact(lifecycle, true, false, &reloadable),
            ConfigImpact::NextStart
        );
    }
    assert_eq!(
        impact(InstanceLifecycle::Running, false, false, &reloadable),
        ConfigImpact::Restart,
        "a provider without reload restarts"
    );
    assert_eq!(
        impact(InstanceLifecycle::Running, true, true, &reloadable),
        ConfigImpact::Restart,
        "a port change restarts"
    );
    assert_eq!(
        impact(InstanceLifecycle::Recovering, true, false, &reloadable),
        ConfigImpact::Restart
    );
    assert_eq!(
        impact(InstanceLifecycle::Running, true, false, &pg_base()),
        ConfigImpact::None
    );
    assert_eq!(
        impact(InstanceLifecycle::Stopped, true, true, &pg_base()),
        ConfigImpact::NextStart,
        "ports alone are a change"
    );
}

#[test]
fn nats_refuses_a_managed_field_and_restarts_without_reload() {
    let package = package("nats");
    let schema = package.settings_schema();
    let active = values(&package, &serde_json::json!({}));

    let managed = evaluate_change(
        schema,
        &active,
        &serde_json::json!({ "serverName": "mine" }),
        InstanceLifecycle::Running,
        false,
        false,
    );
    assert!(!managed.is_valid());
    assert_eq!(managed.field_issues[0].path, "settings.serverName");

    let jetstream = evaluate_change(
        schema,
        &active,
        &serde_json::json!({ "jetstream": false }),
        InstanceLifecycle::Running,
        false,
        false,
    );
    assert!(jetstream.is_valid());
    assert_eq!(jetstream.impact, ConfigImpact::Restart);
}

#[test]
fn fake_set_once_and_secret_fields_are_refused_with_their_own_codes() {
    let package = package("fake");
    let schema = package.settings_schema();
    let base = serde_json::json!({ "authToken": secret("auth-token") });
    let active = values(&package, &base);

    let set_once = evaluate_change(
        schema,
        &active,
        &with(&base, "failInit", true.into()),
        InstanceLifecycle::Stopped,
        true,
        false,
    );
    assert_eq!(
        codes(&set_once),
        [(
            "settings.failInit".to_owned(),
            FieldCode::ImmutableAfterInit
        )]
    );
    assert_eq!(set_once.values, None);

    // authToken is secret and set once: secretReadOnly wins.
    let token = evaluate_change(
        schema,
        &active,
        &serde_json::json!({ "authToken": "local:other/token" }),
        InstanceLifecycle::Stopped,
        true,
        false,
    );
    assert_eq!(
        codes(&token),
        [("settings.authToken".to_owned(), FieldCode::SecretReadOnly)]
    );

    let greeting = evaluate_change(
        schema,
        &active,
        &with(&base, "greeting", "hi".into()),
        InstanceLifecycle::Running,
        true,
        false,
    );
    assert!(greeting.is_valid());
    assert_eq!(greeting.impact, ConfigImpact::Reload);
}

#[test]
fn traefik_routes_compare_by_name_so_order_is_no_change() {
    let package = package("traefik");
    let schema = package.settings_schema();
    let route = |name: &str, rule: &str| serde_json::json!({ "name": name, "rule": rule, "backendUrl": "http://127.0.0.1:3000" });
    let two = serde_json::json!({ "routes": [route("api", "Host(`api.localhost`)"), route("web", "Host(`web.localhost`)")] });
    let active = values(&package, &two);

    let swapped = serde_json::json!({ "routes": [route("web", "Host(`web.localhost`)"), route("api", "Host(`api.localhost`)")] });
    assert!(diff_settings(schema, &active, &values(&package, &swapped)).is_empty());

    let changed = serde_json::json!({ "routes": [route("api", "Host(`v2.localhost`)"), route("admin", "Host(`admin.localhost`)")] });
    let evaluation = evaluate_change(
        schema,
        &active,
        &changed,
        InstanceLifecycle::Running,
        true,
        false,
    );
    assert!(evaluation.is_valid(), "{:?}", evaluation.field_issues);
    let keys: Vec<&str> = evaluation
        .changes
        .iter()
        .map(|change| change.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "routes[name=api].rule",
            "routes[name=web]",
            "routes[name=admin]"
        ]
    );
    assert_eq!(evaluation.changes[1].new, None, "web was removed");
    assert_eq!(evaluation.changes[2].old, None, "admin was added");
    assert_eq!(evaluation.impact, ConfigImpact::Reload);

    let duplicate = serde_json::json!({ "routes": [route("api", "a"), route("api", "b")] });
    let refused = evaluate_change(
        schema,
        &active,
        &duplicate,
        InstanceLifecycle::Running,
        true,
        false,
    );
    assert!(!refused.is_valid());
    assert!(
        refused
            .field_issues
            .iter()
            .all(|issue| issue.path.starts_with("settings.routes")),
        "{:?}",
        refused.field_issues
    );
}

fn fake_instance(request: PortRequest) -> Instance {
    Instance::new(NewInstance {
        id: INSTANCE.parse().expect("the fixture is valid"),
        name: "fake-one".parse().expect("the fixture is valid"),
        display_name: None,
        provider_revision: package("fake").key().clone(),
        software: "fake".parse().expect("the fixture is valid"),
        runtime_id: stackly_domain::RuntimeId::generate(),
        data_line: "0.1".parse().expect("the fixture is valid"),
        data_version: "0.1.0".parse().expect("the fixture is valid"),
        endpoints: vec![Endpoint::Ip(IpEndpoint::new(
            "http".parse().expect("the fixture is valid"),
            Transport::Tcp,
            "127.0.0.1".parse().expect("the fixture is valid"),
            request,
            None,
        ))],
        startup: StartupPolicy::Manual,
        created_at: jiff::Timestamp::UNIX_EPOCH,
        created_by: OperationId::generate(),
    })
    .expect("the fixture is valid")
}

#[test]
fn the_preview_shows_only_the_plain_config_and_marks_changed_lines() {
    let package = package("fake");
    let roots = RootSet::new(
        PathBuf::from("/Volumes/T/data"),
        PathBuf::from("/Volumes/T/logs"),
        PathBuf::from("/Volumes/T/cache"),
        PathBuf::from("/private/tmp/skt-config"),
    )
    .expect("the fixture is valid");
    let layout = DirectoryLayout::new(roots);
    let id: InstanceId = INSTANCE.parse().expect("the fixture is valid");
    let paths = layout.instance_paths(id);
    let runtime = RuntimeContext {
        root: PathBuf::from("/Volumes/T/runtime"),
        version: "0.1.0".parse().expect("the fixture is valid"),
        line: "0.1".parse().expect("the fixture is valid"),
    };
    let base = serde_json::json!({ "authToken": secret("auth-token") });
    let instance = fake_instance(PortRequest::Auto);

    let before = preview(
        &package,
        &instance,
        &paths,
        &runtime,
        &values(&package, &base),
    )
    .expect("the fixture is valid");
    assert!(
        before.iter().all(|file| file.path != "fake.json"),
        "the sensitive fake.json never shows"
    );
    let conf = before
        .iter()
        .position(|file| file.path == "fake.conf")
        .expect("the plain fake.conf shows");
    assert!(before[conf].preview);
    assert_eq!(
        before[conf].port_note.as_deref(),
        Some("Automatic ports shown as 18080")
    );
    assert!(
        !before[conf].content.contains("local:"),
        "no secret reference"
    );

    let after = preview(
        &package,
        &instance,
        &paths,
        &runtime,
        &values(&package, &with(&base, "greeting", "hi".into())),
    )
    .expect("the fixture is valid");
    assert_eq!(
        changed_lines(&before[conf].content, &after[conf].content),
        [2]
    );

    let fixed = fake_instance(PortRequest::Fixed(
        stackly_domain::Port::new(18181).expect("the fixture is valid"),
    ));
    let fixed_files = preview(&package, &fixed, &paths, &runtime, &values(&package, &base))
        .expect("the fixture is valid");
    assert_eq!(fixed_files[conf].port_note, None);
}
