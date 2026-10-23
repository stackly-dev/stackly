//! The rebind check on the real `providers/postgresql` package over fakes
//! (spec 0025 AC-6, AC-8): the order of the refusals, the data marker, the
//! shared answer with start, and no write on any path.
#![cfg(unix)]

#[path = "create_evaluate/common.rs"]
#[allow(dead_code)]
mod common;

use std::fs;
use std::path::Path;

use stackly_core::instance::data_compat::check_data_compatibility;
use stackly_core::instance::rebind::{RebindCheckError, RebindVerdict, check};
use stackly_core::operation::OperationError;
use stackly_core::{create_instance_layout, daemon_info};
use stackly_domain::{
    InstallMode, Instance, InstanceFilter, InstanceId, InstanceLifecycle, InstanceManifest,
    ManifestRuntime, NewInstanceManifest, OperationId, Registry, Runtime, RuntimeId, RuntimeState,
    Versioned, WriteBatch, WriteOp,
};
use stackly_evidence::{Kind, evidence};
use stackly_platform::Clock;
use stackly_testkit::{InstanceBuilder, RuntimeBuilder};

use common::{Site, commit_runtime, provider_dir};

const ALLOW_17_ON_18: &str = "  lineCompatibility:\n    \"17\": [\"18\"]\n";

/// The `compatibility` section of a copy of the package allows data of line
/// 17 on runtimes of line 18.
fn allow_17_on_18(dir: &Path) {
    let path = dir.join("provider.yaml");
    let text = fs::read_to_string(&path).expect("the manifest reads");
    let anchor = "  dataMarker:";
    assert!(text.contains(anchor));
    fs::write(
        &path,
        text.replacen(anchor, &format!("{ALLOW_17_ON_18}{anchor}"), 1),
    )
    .expect("the manifest writes");
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("the target folder can be made");
    for entry in fs::read_dir(from).expect("the source folder reads") {
        let entry = entry.expect("an entry reads");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the type reads").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a file copies");
        }
    }
}

/// A site of the PostgreSQL package, its package pinned in the data folder.
fn site() -> Site {
    let site = Site::with_provider("postgresql");
    pin(&site, |_| {});
    site
}

fn pin(site: &Site, edit: impl FnOnce(&Path)) {
    let target = site
        .prepared
        .layout()
        .provider_revision_dir(site.package.key());
    copy_dir(&provider_dir("postgresql"), &target);
    edit(&target);
}

/// Commits a PostgreSQL runtime of `version` and `line`, installed.
fn runtime_of(site: &Site, version: &str, line: &str, state: RuntimeState) -> Runtime {
    let built = RuntimeBuilder::new()
        .software("postgresql")
        .version(version, line)
        .state(state)
        .installed_at(site.fake.clock().now())
        .build();
    let id = commit_runtime(site, built);
    site.registry
        .runtime(id)
        .expect("the registry reads")
        .value
        .expect("the runtime is stored")
}

/// What the data folder of a test instance holds as its marker.
#[derive(Clone, Copy)]
enum Marker {
    Text(&'static str),
    Missing,
    /// A folder where the file belongs: not a regular file.
    Folder,
}

/// Commits an instance of the package on `runtime` with its folders and
/// `marker`, in `lifecycle`.
fn instance_on(
    site: &Site,
    runtime: &Runtime,
    name: &str,
    lifecycle: InstanceLifecycle,
    marker: Marker,
) -> Instance {
    let instance = InstanceBuilder::new(runtime, site.package.key())
        .name(name)
        .lifecycle(lifecycle)
        .build();
    site.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertInstance(
            instance.clone(),
        )]))
        .expect("the instance commits");
    let InstallMode::Managed { artifact_sha256 } = runtime.install() else {
        unreachable!("the test runtimes are managed");
    };
    let manifest = InstanceManifest::new(NewInstanceManifest {
        instance_id: instance.id(),
        name: instance.name().clone(),
        display_name: None,
        software: runtime.software().clone(),
        provider: site.package.key().clone(),
        runtime: ManifestRuntime::managed(
            runtime.id(),
            runtime.version().clone(),
            runtime.line().clone(),
            runtime.target(),
            *artifact_sha256,
        ),
        created_at: site.fake.clock().now(),
        created_by_operation: OperationId::generate(),
        stackly_version: daemon_info().version.to_owned(),
    });
    let paths = create_instance_layout(&site.platform, &site.prepared, &manifest, &[])
        .expect("the instance folders can be made");
    let file = paths.data.join("PG_VERSION");
    match marker {
        Marker::Text(text) => fs::write(&file, text).expect("the marker is written"),
        Marker::Missing => {}
        Marker::Folder => fs::create_dir(&file).expect("a folder as the marker"),
    }
    instance
}

/// A stopped instance on the site's own runtime (line 18, the first
/// artifact of the package).
fn stopped(site: &Site, name: &str, marker: Marker) -> Instance {
    instance_on(
        site,
        &site.runtime,
        name,
        InstanceLifecycle::Stopped,
        marker,
    )
}

/// Everything the check could change: the rows and their generation, and
/// the files and the processes of the platform.
#[derive(Debug, PartialEq)]
struct Snapshot {
    instances: Versioned<Vec<Instance>>,
    runtimes: Versioned<Vec<Runtime>>,
    mutations: usize,
    spawns: usize,
}

fn snapshot(site: &Site) -> Snapshot {
    Snapshot {
        instances: site
            .registry
            .instances(InstanceFilter {
                include_retained: true,
            })
            .expect("the registry reads"),
        runtimes: site.registry.runtimes().expect("the registry reads"),
        mutations: site.fake.fs().mutations().len(),
        spawns: site.fake.processes().spawns().len(),
    }
}

/// Runs the check and asserts it wrote nothing.
fn checked(
    site: &Site,
    id: InstanceId,
    target: RuntimeId,
) -> Result<RebindVerdict, RebindCheckError> {
    let before = snapshot(site);
    let verdict = check(&site.registry, &site.platform, &site.prepared, id, target);
    assert_eq!(snapshot(site), before, "the check wrote something");
    verdict
}

fn code_and_details(error: &RebindCheckError) -> (String, Vec<(String, String)>) {
    (
        error.code().to_string(),
        error
            .details()
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

fn detail(error: &RebindCheckError, key: &str) -> Option<String> {
    code_and_details(error)
        .1
        .into_iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

#[test]
fn rebind_check_refuses_another_postgresql_major() {
    // covers: AC-6 (another major, both ways, Current, Compatible, no write)
    let ev = evidence!(
        covers = ["REQ-043", "AC-043", "BR-009"],
        kind = Kind::Fixture,
        expected = "data of line 17 is refused by a runtime of line 18 from the real PostgreSQL package with supervisor.data_incompatible, outcome incompatible, data 17 and runtime 18, the other direction is refused the same way, the same runtime is Current, a second runtime of line 17 is Compatible, and nothing is written; fixture only, the user actions of AC-043 belong to feature 58",
    );
    let site = site();
    let eighteen = &site.runtime;
    assert_eq!(eighteen.line().to_string(), "18");
    let seventeen = runtime_of(&site, "17.11", "17", RuntimeState::Installed);
    let later_seventeen = runtime_of(&site, "17.12", "17", RuntimeState::Installed);
    let old = instance_on(
        &site,
        &seventeen,
        "pg-old",
        InstanceLifecycle::Stopped,
        Marker::Text("17\n"),
    );
    let new = instance_on(
        &site,
        eighteen,
        "pg-new",
        InstanceLifecycle::Stopped,
        Marker::Text("18\n"),
    );

    for (instance, target, data, runtime) in
        [(&old, eighteen, "17", "18"), (&new, &seventeen, "18", "17")]
    {
        let error = checked(&site, instance.id(), target.id()).expect_err("another major");
        let (code, details) = code_and_details(&error);
        assert_eq!(code, "supervisor.data_incompatible");
        assert_eq!(
            details,
            [
                ("outcome".to_owned(), "incompatible".to_owned()),
                ("data".to_owned(), data.to_owned()),
                ("runtime".to_owned(), runtime.to_owned()),
            ]
        );
    }

    assert_eq!(
        checked(&site, old.id(), seventeen.id()).expect("the current runtime"),
        RebindVerdict::Current
    );
    assert_eq!(
        checked(&site, old.id(), later_seventeen.id()).expect("the same line"),
        RebindVerdict::Compatible {
            runtime: later_seventeen.id(),
            line: later_seventeen.line().clone(),
        }
    );
    ev.pass();
}

#[test]
fn a_package_that_allows_the_pair_gives_compatible() {
    // covers: AC-6 (lineCompatibility gives Compatible)
    let site = Site::with_provider_edit("postgresql", allow_17_on_18);
    pin(&site, allow_17_on_18);
    let seventeen = runtime_of(&site, "17.11", "17", RuntimeState::Installed);
    let instance = instance_on(
        &site,
        &seventeen,
        "pg-old",
        InstanceLifecycle::Stopped,
        Marker::Text("17\n"),
    );
    let eighteen = &site.runtime;

    assert_eq!(
        checked(&site, instance.id(), eighteen.id()).expect("the package allows 17 on 18"),
        RebindVerdict::Compatible {
            runtime: eighteen.id(),
            line: eighteen.line().clone(),
        }
    );
    // The pair is one way: data of 18 does not go back to 17.
    let new = instance_on(
        &site,
        eighteen,
        "pg-new",
        InstanceLifecycle::Stopped,
        Marker::Text("18\n"),
    );
    assert!(checked(&site, new.id(), seventeen.id()).is_err());
}

#[test]
fn a_missing_changed_or_unreadable_marker_is_refused_with_its_outcome() {
    // covers: AC-6 (marker_missing, marker_mismatch, marker_unreadable)
    let ev = evidence!(
        covers = ["REQ-043", "AC-043", "BR-009"],
        kind = Kind::Fixture,
        expected = "a PG_VERSION that is missing, holds 16, or is not a regular file is marker_missing, marker_mismatch, or marker_unreadable from the rebind check, and the check writes nothing; fixture only",
    );
    let site = site();
    let seventeen = runtime_of(&site, "17.11", "17", RuntimeState::Installed);
    let later = runtime_of(&site, "17.12", "17", RuntimeState::Installed);
    for (marker, outcome) in [
        (Marker::Missing, "marker_missing"),
        (Marker::Text("16\n"), "marker_mismatch"),
        (Marker::Folder, "marker_unreadable"),
    ] {
        let name = outcome.replace('_', "-");
        let instance = instance_on(&site, &seventeen, &name, InstanceLifecycle::Stopped, marker);

        let error = checked(&site, instance.id(), later.id()).expect_err(outcome);
        assert_eq!(error.code().to_string(), "supervisor.data_incompatible");
        assert_eq!(detail(&error, "outcome").as_deref(), Some(outcome));
        assert_eq!(detail(&error, "data"), None, "{outcome}");
        assert_eq!(
            detail(&error, "runtime").as_deref(),
            Some("17"),
            "{outcome}"
        );
    }
    ev.pass();
}

#[test]
fn the_refusals_come_in_the_order_of_the_spec() {
    // covers: AC-6 (each row of the table, one at a time)
    let site = site();
    let instance = stopped(&site, "pg-a", Marker::Text("18\n"));
    let seventeen = runtime_of(&site, "17.12", "17", RuntimeState::Installed);

    // 1: no such instance.
    let error = checked(&site, InstanceId::generate(), seventeen.id()).expect_err("unknown");
    assert_eq!(error.code().to_string(), "resource.not_found");

    // 4: no such target runtime.
    let unknown = RuntimeId::generate();
    let error = checked(&site, instance.id(), unknown).expect_err("no runtime");
    assert_eq!(error.code().to_string(), "instance.runtime_not_found");
    assert_eq!(detail(&error, "runtimeId"), Some(unknown.to_string()));

    // 6: another software.
    let other = RuntimeBuilder::new()
        .software("nats")
        .version("2.12.1", "2.12")
        .state(RuntimeState::Installed)
        .installed_at(site.fake.clock().now())
        .build();
    let other = commit_runtime(&site, other);
    let error = checked(&site, instance.id(), other).expect_err("another software");
    assert_eq!(error.code().to_string(), "instance.runtime_mismatch");
    assert_eq!(detail(&error, "runtimeSoftware").as_deref(), Some("nats"));
    assert_eq!(
        detail(&error, "providerSoftware").as_deref(),
        Some("postgresql")
    );

    // 7: the target is not installed.
    let broken = runtime_of(&site, "17.13", "17", RuntimeState::Failed);
    let error = checked(&site, instance.id(), broken.id()).expect_err("not installed");
    assert_eq!(error.code().to_string(), "supervisor.runtime_unavailable");
    assert_eq!(detail(&error, "state").as_deref(), Some("failed"));

    // 3: data that was never initialized.
    let fresh = site.add_instance(|builder| {
        builder
            .name("pg-fresh")
            .lifecycle(InstanceLifecycle::Uninitialized)
    });
    let error = checked(&site, fresh.id(), seventeen.id()).expect_err("uninitialized");
    assert_eq!(error.code().to_string(), "supervisor.not_initialized");

    // 2: a retained instance, before the initialization check.
    let retained = site.add_instance(|builder| {
        builder
            .name("pg-old")
            .retained(OperationId::generate(), site.fake.clock().now())
    });
    let error = checked(&site, retained.id(), seventeen.id()).expect_err("retained");
    assert_eq!(error.code().to_string(), "supervisor.instance_retained");

    // 8: the pinned package is gone (a second site without a pin).
    let unpinned = Site::with_provider("postgresql");
    let lone = stopped(&unpinned, "pg-lone", Marker::Text("17\n"));
    let target = runtime_of(&unpinned, "17.12", "17", RuntimeState::Installed);
    let error = checked(&unpinned, lone.id(), target.id()).expect_err("no pinned package");
    assert_eq!(error.code().to_string(), "supervisor.package_unavailable");

    // 9: the instance folder is open to others.
    {
        use std::os::unix::fs::PermissionsExt;
        let paths = site.prepared.layout().instance_paths(instance.id());
        fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o755))
            .expect("the permissions change");
    }
    let error = checked(&site, instance.id(), seventeen.id()).expect_err("open folder");
    assert_eq!(error.code().to_string(), "supervisor.data_unavailable");
}

/// The code of the refusal of `check`.
fn refused_code(site: &Site, id: InstanceId, target: RuntimeId) -> String {
    checked(site, id, target)
        .expect_err("the check refuses")
        .code()
        .to_string()
}

#[test]
fn the_earlier_row_wins_when_two_requirements_fail_at_once() {
    // covers: AC-6 (the table *Pemeriksaan rebind*: the first refusal wins)
    let site = site();
    let seventeen = runtime_of(&site, "17.12", "17", RuntimeState::Installed);
    let now = site.fake.clock().now();

    // 1 over 4: no such instance and no such runtime.
    assert_eq!(
        refused_code(&site, InstanceId::generate(), RuntimeId::generate()),
        "resource.not_found"
    );

    // 2 over 4: a retained instance, and a target runtime that is not there.
    let retained = site.add_instance(|builder| {
        builder
            .name("pg-retained")
            .retained(OperationId::generate(), now)
    });
    assert_eq!(
        refused_code(&site, retained.id(), RuntimeId::generate()),
        "supervisor.instance_retained"
    );

    // 2 over 3: a retained instance whose data never initialized.
    let retained_fresh = site.add_instance(|builder| {
        builder
            .name("pg-retained-fresh")
            .retained(OperationId::generate(), now)
    });
    assert!(retained_fresh.data_initialized_at().is_none());
    assert_eq!(
        refused_code(&site, retained_fresh.id(), seventeen.id()),
        "supervisor.instance_retained"
    );

    // 3 over 4: data that never initialized, and a target runtime that is not
    // there.
    let fresh = site.add_instance(|builder| {
        builder
            .name("pg-fresh")
            .lifecycle(InstanceLifecycle::Uninitialized)
    });
    assert_eq!(
        refused_code(&site, fresh.id(), RuntimeId::generate()),
        "supervisor.not_initialized"
    );

    // 2 over 5, 3 over 5: the instance's own runtime is no shortcut for a
    // retained or uninitialized instance.
    assert_eq!(
        refused_code(&site, retained.id(), site.runtime.id()),
        "supervisor.instance_retained"
    );
    assert_eq!(
        refused_code(&site, fresh.id(), site.runtime.id()),
        "supervisor.not_initialized"
    );

    // 5 over 10, and over 7: a marker that disagrees with the registry, or a
    // runtime that is no longer installed, does not matter for the runtime the
    // instance runs on now.
    let wrong_marker = stopped(&site, "pg-wrong-marker", Marker::Text("16\n"));
    assert_eq!(
        checked(&site, wrong_marker.id(), site.runtime.id()).expect("the current runtime"),
        RebindVerdict::Current
    );
    let on_broken = runtime_of(&site, "18.7", "18", RuntimeState::Failed);
    let stranded = instance_on(
        &site,
        &on_broken,
        "pg-stranded",
        InstanceLifecycle::Stopped,
        Marker::Text("18\n"),
    );
    assert_eq!(
        checked(&site, stranded.id(), on_broken.id()).expect("the current runtime"),
        RebindVerdict::Current
    );

    // 6 over 7: another software that is not installed either.
    let other = RuntimeBuilder::new()
        .software("nats")
        .version("2.12.1", "2.12")
        .state(RuntimeState::Failed)
        .installed_at(now)
        .build();
    let other = commit_runtime(&site, other);
    let on_eighteen = stopped(&site, "pg-a", Marker::Text("18\n"));
    assert_eq!(
        refused_code(&site, on_eighteen.id(), other),
        "instance.runtime_mismatch"
    );

    // 7 over 8 and 9 and 10: a target that is not installed, on an instance
    // with no pinned package, an open folder, and a marker of another major.
    let broken = runtime_of(&site, "17.13", "17", RuntimeState::Failed);
    assert_eq!(
        refused_code(&site, on_eighteen.id(), broken.id()),
        "supervisor.runtime_unavailable"
    );

    // 8 over 9 and 10: no pinned package, an open folder, and 17 data.
    let unpinned = Site::with_provider("postgresql");
    let lone = stopped(&unpinned, "pg-lone", Marker::Text("17\n"));
    let target = runtime_of(&unpinned, "17.12", "17", RuntimeState::Installed);
    open_folder(&unpinned, lone.id());
    assert_eq!(
        refused_code(&unpinned, lone.id(), target.id()),
        "supervisor.package_unavailable"
    );

    // 9 over 10: an open folder, and data of another major.
    let old = instance_on(
        &site,
        &seventeen,
        "pg-old",
        InstanceLifecycle::Stopped,
        Marker::Text("17\n"),
    );
    open_folder(&site, old.id());
    assert_eq!(
        refused_code(&site, old.id(), site.runtime.id()),
        "supervisor.data_unavailable"
    );
}

fn open_folder(site: &Site, id: InstanceId) {
    use std::os::unix::fs::PermissionsExt;
    let paths = site.prepared.layout().instance_paths(id);
    fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o755))
        .expect("the permissions change");
}

#[test]
fn a_running_instance_is_checked_like_a_stopped_one() {
    // covers: AC-6 (lifecycle running is not refused)
    let site = site();
    let seventeen = runtime_of(&site, "17.11", "17", RuntimeState::Installed);
    let later = runtime_of(&site, "17.12", "17", RuntimeState::Installed);
    // Only retained and uninitialized data are refused by the lifecycle; the
    // marker does not change while the server runs. A `failed` instance whose
    // data never initialized is refused as `not_initialized`.
    for lifecycle in [InstanceLifecycle::Running, InstanceLifecycle::Stopped] {
        let instance = instance_on(
            &site,
            &seventeen,
            &format!("pg-{}", lifecycle.as_code()),
            lifecycle,
            Marker::Text("17\n"),
        );
        let verdict = checked(&site, instance.id(), later.id());
        assert!(
            matches!(verdict, Ok(RebindVerdict::Compatible { .. })),
            "{lifecycle:?}: {verdict:?}"
        );
    }
}

#[test]
fn rebind_and_start_share_the_answer_of_the_data_check() {
    // covers: AC-8 (the check and start give one answer)
    let site = site();
    let seventeen = runtime_of(&site, "17.11", "17", RuntimeState::Installed);
    let instance = instance_on(
        &site,
        &seventeen,
        "pg-old",
        InstanceLifecycle::Stopped,
        Marker::Text("17\n"),
    );
    let eighteen = &site.runtime;
    let error = checked(&site, instance.id(), eighteen.id()).expect_err("incompatible");
    let direct = check_data_compatibility(
        site.platform.fs(),
        &site.package,
        &site.prepared.layout().instance_paths(instance.id()),
        instance.data_line(),
        eighteen.line(),
    )
    .expect_err("the shared check refuses");
    let from_start = stackly_core::supervisor::SupervisorError::from(direct);
    assert_eq!(
        code_and_details(&error),
        (
            from_start.code().to_string(),
            from_start
                .details()
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect()
        )
    );
}
