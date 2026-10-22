//! `instance::detail::build` (spec 0021 AC-4): every field of the detail
//! comes from the snapshot it names in the *Value sourcing* table, with the
//! `fake-command` fixture package pinned, and nothing reads a secret value.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use stackly_core::instance::connection::{ConnectionState, ConnectionUnavailable};
use stackly_core::instance::detail::{DetailError, DetailSnapshot, build};
use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
use stackly_domain::{
    Endpoint, EnvironmentId, Instance, InstanceId, InstancePathEntry, InstancePathKind, IpEndpoint,
    LogicalRoot, Ownership, PortRequest, RelativePath, Runtime, SecretFieldName, SecretRef,
    SecretStatus, SecretUse, SecretUser, StartupPolicy, Transport, UnixEndpoint,
};
use stackly_platform::{DirectoryLayout, RootSet};
use stackly_testkit::{EnvironmentBuilder, InstanceBuilder, LeaseBuilder, RuntimeBuilder};

const ID: &str = "0192f000-0000-7000-8000-00000000000a";
const URI: &str = "fake://user:${secret.authToken}@localhost/db?port=${endpoint.http.port}";

fn package(with_connection: bool) -> ValidatedPackage {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/fake-command");
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
    if with_connection {
        let block = format!(
            "\nconnection:\n  - name: uri\n    title: Connection string\n    template: {}\n",
            serde_json::to_string(URI).expect("a string serializes")
        );
        files
            .get_mut("provider.yaml")
            .expect("a manifest")
            .extend_from_slice(block.as_bytes());
    }
    ValidatedPackage::from_files(
        PackageFiles::new("fake-command", files),
        NameCheck::FolderName,
    )
    .unwrap_or_else(|err| panic!("the package is valid: {:#?}", err.issues))
}

fn layout() -> DirectoryLayout {
    DirectoryLayout::new(
        RootSet::new(
            PathBuf::from("/Volumes/Test Root/data"),
            PathBuf::from("/Volumes/Test Root/logs"),
            PathBuf::from("/Volumes/Test Root/cache"),
            PathBuf::from("/private/tmp/skt-detail/stackly-0000000a"),
        )
        .expect("the roots are valid"),
    )
}

fn id() -> InstanceId {
    ID.parse().expect("a UUID v7")
}

fn instance(package: &ValidatedPackage, runtime: &Runtime) -> Instance {
    let endpoints = vec![
        Endpoint::Ip(IpEndpoint::new(
            "http".parse().expect("endpoint name"),
            Transport::Tcp,
            "127.0.0.1".parse().expect("an address"),
            PortRequest::Auto,
            Some(stackly_domain::Port::new(14000).expect("a port")),
        )),
        Endpoint::Unix(UnixEndpoint::new(
            "control".parse().expect("endpoint name"),
            "fake.14000".parse().expect("socket name"),
        )),
    ];
    InstanceBuilder::new(runtime, package.key())
        .with_id(id())
        .name("orders-db")
        .endpoints(endpoints)
        .startup(StartupPolicy::AtLogin)
        .revision(7)
        .build()
}

fn data_row() -> Vec<InstancePathEntry> {
    vec![InstancePathEntry::new(
        InstancePathKind::Data,
        LogicalRoot::Data,
        RelativePath::new(format!("instances/{ID}/data")).expect("a relative path"),
    )]
}

fn token_use(instance: InstanceId) -> SecretUse {
    SecretUse::new(
        format!("local:instance/{ID}/auth-token")
            .parse()
            .expect("a reference"),
        SecretUser::Instance(instance),
        "authToken".parse().expect("a field name"),
    )
}

struct Site {
    package: ValidatedPackage,
    runtime: Runtime,
    instance: Instance,
    layout: DirectoryLayout,
    paths: Vec<InstancePathEntry>,
}

impl Site {
    fn new() -> Self {
        let package = package(true);
        let runtime = RuntimeBuilder::new().build();
        let instance = instance(&package, &runtime);
        Self {
            package,
            runtime,
            instance,
            layout: layout(),
            paths: data_row(),
        }
    }

    fn snapshot<'a>(
        &'a self,
        uses: &'a [SecretUse],
        statuses: &'a [(SecretRef, SecretStatus)],
        environments: &'a [stackly_domain::Environment],
        leases: &'a [stackly_domain::Lease],
        package: Option<&'a ValidatedPackage>,
    ) -> DetailSnapshot<'a> {
        DetailSnapshot::new(
            &self.instance,
            &self.runtime,
            &self.paths,
            uses,
            statuses,
            environments,
            leases,
            package,
            &self.layout,
        )
    }
}

#[test]
fn every_field_comes_from_its_source() {
    let site = Site::new();
    let uses = [token_use(id())];
    let statuses = [(uses[0].reference.clone(), SecretStatus::Present)];

    let detail = build(&site.snapshot(&uses, &statuses, &[], &[], Some(&site.package)))
        .expect("the detail builds");

    assert_eq!(detail.instance_id, id());
    assert_eq!(detail.revision, site.instance.revision());
    assert_eq!(detail.provider.key, *site.package.key());
    assert_eq!(
        detail.provider.display_name.as_deref(),
        Some("Fake Command Provider")
    );
    assert!(detail.provider.package_readable);
    assert_eq!(detail.runtime.id, site.runtime.id());
    assert_eq!(detail.runtime.version, site.runtime.version().to_string());
    assert_eq!(detail.runtime.target, site.runtime.target());
    assert!(detail.runtime.managed);
    assert!(detail.runtime.artifact_sha256.is_some());
    assert_eq!(
        detail.data_path,
        PathBuf::from(format!("/Volumes/Test Root/data/instances/{ID}/data"))
    );
    assert_eq!(detail.startup, StartupPolicy::AtLogin);
    assert_eq!(detail.secrets.len(), 1);
    assert_eq!(detail.secrets[0].field.as_str(), "authToken");
    assert_eq!(detail.secrets[0].title, "Auth token");
    assert_eq!(detail.secrets[0].status, SecretStatus::Present);
    assert!(detail.dependents.is_empty());
    assert_eq!(detail.connections.len(), 1);
    let ConnectionState::Available { masked, .. } = &detail.connections[0].state else {
        panic!(
            "the connection is available: {:?}",
            detail.connections[0].state
        );
    };
    assert_eq!(masked, "fake://user:****@localhost/db?port=14000");
}

#[test]
fn a_secret_without_a_cached_status_is_unknown_and_other_users_are_ignored() {
    let site = Site::new();
    let other = SecretUse::new(
        format!("local:environment/{ID}/other")
            .parse()
            .expect("a reference"),
        SecretUser::Environment(EnvironmentId::generate()),
        "other".parse::<SecretFieldName>().expect("a field name"),
    );
    let uses = [other, token_use(id())];

    let detail = build(&site.snapshot(&uses, &[], &[], &[], Some(&site.package)))
        .expect("the detail builds");

    assert_eq!(detail.secrets.len(), 1);
    assert_eq!(detail.secrets[0].status, SecretStatus::Unknown);
}

#[test]
fn secrets_are_ordered_by_field_and_a_field_outside_the_schema_keeps_its_name() {
    let site = Site::new();
    let extra = SecretUse::new(
        format!("local:instance/{ID}/api-key")
            .parse()
            .expect("a reference"),
        SecretUser::Instance(id()),
        "apikey".parse().expect("a field name"),
    );
    let uses = [token_use(id()), extra];

    let detail = build(&site.snapshot(&uses, &[], &[], &[], Some(&site.package)))
        .expect("the detail builds");

    let fields: Vec<(&str, &str)> = detail
        .secrets
        .iter()
        .map(|secret| (secret.field.as_str(), secret.title.as_str()))
        .collect();
    assert_eq!(fields, [("apikey", "apikey"), ("authToken", "Auth token")]);
}

#[test]
fn an_unreadable_package_gives_no_display_name_no_connections_and_field_names_as_titles() {
    let site = Site::new();
    let uses = [token_use(id())];

    let detail =
        build(&site.snapshot(&uses, &[], &[], &[], None)).expect("an unreadable package is fine");

    assert_eq!(detail.provider.display_name, None);
    assert!(!detail.provider.package_readable);
    assert!(detail.connections.is_empty());
    assert_eq!(detail.secrets[0].title, "authToken");
    assert_eq!(
        detail.provider.key,
        *site.package.key(),
        "the key is the registry's"
    );
}

#[test]
fn a_package_that_is_not_the_pinned_one_counts_as_unreadable() {
    let site = Site::new();
    let other = package(false);

    let detail =
        build(&site.snapshot(&[], &[], &[], &[], Some(&other))).expect("the detail builds");

    assert_eq!(detail.provider.display_name, None);
    assert!(!detail.provider.package_readable);
    assert!(detail.connections.is_empty());
}

#[test]
fn a_readable_package_without_a_connection_block_is_readable_with_no_connections() {
    let site = Site::new();
    let plain = package(false);
    let runtime = RuntimeBuilder::new().build();
    let instance = instance(&plain, &runtime);
    let snapshot = DetailSnapshot::new(
        &instance,
        &runtime,
        &site.paths,
        &[],
        &[],
        &[],
        &[],
        Some(&plain),
        &site.layout,
    );

    let detail = build(&snapshot).expect("the detail builds");

    assert!(detail.provider.package_readable);
    assert!(detail.provider.display_name.is_some());
    assert!(detail.connections.is_empty());
}

#[test]
fn a_connection_waits_for_its_port_while_the_endpoint_has_none() {
    let mut site = Site::new();
    let package = package(true);
    let runtime = site.runtime.clone();
    let endpoints = vec![
        Endpoint::Ip(IpEndpoint::new(
            "http".parse().expect("endpoint name"),
            Transport::Tcp,
            "127.0.0.1".parse().expect("an address"),
            PortRequest::Auto,
            None,
        )),
        Endpoint::Unix(UnixEndpoint::new(
            "control".parse().expect("endpoint name"),
            "fake.14000".parse().expect("socket name"),
        )),
    ];
    site.instance = InstanceBuilder::new(&runtime, package.key())
        .with_id(id())
        .name("orders-db")
        .endpoints(endpoints)
        .build();
    let uses = [token_use(id())];

    let detail =
        build(&site.snapshot(&uses, &[], &[], &[], Some(&package))).expect("the detail builds");

    assert_eq!(
        detail.connections[0].state,
        ConnectionState::Unavailable(ConnectionUnavailable::PortNotAssigned)
    );
    assert!(detail.connections[0].port_may_change);
}

#[test]
fn dependents_are_the_environments_with_the_instance_by_name_then_id() {
    let site = Site::new();
    let low = EnvironmentId::generate();
    let high = EnvironmentId::generate();
    let (first, second) = if low < high { (low, high) } else { (high, low) };
    let environments = [
        EnvironmentBuilder::new()
            .with_id(second)
            .name("beta")
            .member("db", id(), Ownership::Shared)
            .build(),
        EnvironmentBuilder::new()
            .with_id(first)
            .name("beta")
            .member("db", id(), Ownership::Dedicated)
            .build(),
        EnvironmentBuilder::new()
            .name("alpha")
            .member("db", id(), Ownership::Shared)
            .build(),
        EnvironmentBuilder::new()
            .name("gamma")
            .member("db", InstanceId::generate(), Ownership::Shared)
            .build(),
    ];
    let leases = [
        LeaseBuilder::new(id()).environment(first).build(),
        LeaseBuilder::new(id()).manual(None).build(),
        LeaseBuilder::new(id())
            .environment(second)
            .released("2026-10-22T04:00:00Z".parse().expect("a time"))
            .build(),
    ];

    let detail = build(&site.snapshot(&[], &[], &environments, &leases, Some(&site.package)))
        .expect("the detail builds");

    let rows: Vec<(String, EnvironmentId, Ownership, bool)> = detail
        .dependents
        .iter()
        .map(|row| {
            (
                row.name.to_string(),
                row.environment_id,
                row.ownership,
                row.lease_active,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "alpha".to_owned(),
                environments[2].id(),
                Ownership::Shared,
                false
            ),
            ("beta".to_owned(), first, Ownership::Dedicated, true),
            ("beta".to_owned(), second, Ownership::Shared, false),
        ]
    );
}

#[test]
fn an_external_runtime_has_no_artifact_digest() {
    let mut site = Site::new();
    site.runtime = RuntimeBuilder::new().external("/opt/fake").build();
    site.instance = instance(&site.package, &site.runtime);

    let detail =
        build(&site.snapshot(&[], &[], &[], &[], Some(&site.package))).expect("the detail builds");

    assert!(!detail.runtime.managed);
    assert_eq!(detail.runtime.artifact_sha256, None);
}

#[test]
fn a_missing_data_row_is_an_error() {
    let mut site = Site::new();
    site.paths.clear();

    let result = build(&site.snapshot(&[], &[], &[], &[], Some(&site.package)));

    assert_eq!(result.unwrap_err(), DetailError::MissingDataPath);
}
