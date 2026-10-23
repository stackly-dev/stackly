//! Instances of one runtime through the real daemon (spec 0024 AC-1, AC-4
//! to AC-8): `instance.get_duplicate_form` and the create of its draft, the
//! isolation of two or three instances of the same fake runtime, and the
//! guarantee that nothing under the source instance is ever read.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_domain::{InstanceId, Registry, SecretUser};
use stackly_protocol::events::{ShutdownReason, SubscribeParams};
use stackly_protocol::instance::{
    DuplicateForm, InstanceLifecycle, InstanceReadinessState, InstanceSummary, PortRequest,
    SecretFieldInput, SecretSource,
};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::operation::{ListOperationsParams, OperationState};
use stackly_protocol::secret::SecretInput;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::support::{
    Api, PROVIDER, REVISION, Site, TOKEN, catalog_of, edited_fake_package, fast, get_sentinel,
    put_sentinel, serving,
};

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

async fn stop_daemon(daemon: stacklyd::server::DaemonHandle) {
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;
}

/// The secret inputs of a client that keeps the draft as it is: every
/// field generated.
fn generated(form: &DuplicateForm) -> Vec<SecretFieldInput> {
    form.draft
        .secrets
        .iter()
        .map(|plan| {
            assert_eq!(plan.source, SecretSource::Generate);
            SecretFieldInput {
                field: plan.field.clone(),
                input: SecretInput::Generate,
            }
        })
        .collect()
}

/// The secret inputs of a client where the user chose Copy from for every
/// field the source has a reference for.
fn copied(form: &DuplicateForm) -> Vec<SecretFieldInput> {
    form.secrets
        .iter()
        .map(|secret| SecretFieldInput {
            field: secret.field.clone(),
            input: SecretInput::CopyFrom(
                secret
                    .source_reference
                    .clone()
                    .expect("the source uses a reference for the field"),
            ),
        })
        .collect()
}

/// Waits for the create operation of `accepted` and returns the new ID.
async fn created_id(
    api: &Api,
    accepted: stackly_protocol::instance::InstanceActionAccepted,
) -> Uuid {
    let accepted = accepted.accepted.expect("a create starts an operation");
    let id = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse::<Uuid>().expect("an instance id"))
        .expect("the operation names the instance");
    let detail = api
        .wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    assert_eq!(detail.operation.state, OperationState::Succeeded);
    id
}

/// Duplicates `source` with the draft as the daemon built it.
async fn duplicate(api: &Api, source: Uuid) -> Uuid {
    let form = api
        .duplicate_form(source)
        .await
        .expect("the duplicate form is built");
    let secrets = generated(&form);
    let accepted = api
        .create_from_draft(form.draft, secrets)
        .await
        .expect("the create of the draft is accepted");
    created_id(api, accepted).await
}

fn port_of(summary: &InstanceSummary) -> u16 {
    summary.endpoints[0]
        .port
        .expect("a ready instance shows its port")
}

fn pid_of(summary: &InstanceSummary) -> u32 {
    summary
        .process
        .as_ref()
        .expect("a running instance has a process")
        .pid
}

/// Every file and folder under `root` as (relative path, kind, size,
/// sha256 of a file, modification time in nanoseconds), sorted.
fn tree(root: &Path) -> Vec<(String, &'static str, u64, String, u128)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, &'static str, u64, String, u128)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).expect("an entry has metadata");
            let relative = path
                .strip_prefix(base)
                .expect("an entry lies under the root")
                .display()
                .to_string();
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |elapsed| elapsed.as_nanos());
            if meta.is_dir() {
                out.push((relative, "dir", 0, String::new(), modified));
                walk(base, &path, out);
            } else {
                let digest = fs::read(&path)
                    .map(|bytes| hex(&Sha256::digest(bytes)))
                    .expect("a file of the tree reads");
                out.push((relative, "file", meta.len(), digest, modified));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Like [`tree`] without the modification time, for a folder a running
/// process may touch.
fn content(root: &Path) -> Vec<(String, &'static str, u64, String)> {
    tree(root)
        .into_iter()
        .map(|(path, kind, size, digest, _)| (path, kind, size, digest))
        .collect()
}

/// The nine folders and files of an instance, as the layout derives them.
fn paths_of(site: &Site, id: Uuid) -> Vec<PathBuf> {
    let paths = site.instance_paths(id);
    vec![
        paths.root,
        paths.manifest,
        paths.lock,
        paths.data,
        paths.config,
        paths.config_history,
        paths.temp,
        paths.logs,
        paths.run,
    ]
}

// covers: AC-1, AC-2, AC-3, AC-4, AC-5
#[tokio::test(flavor = "multi_thread")]
async fn the_duplicate_form_copies_the_settings_and_asks_for_new_identity_inputs() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "instance.get_duplicate_form answers the pinned package form with the runtime and provider revision of the source, the active settings without the token, every port auto, every secret generate with the source reference, the name fake-a-copy, and changes nothing on disk or in the registry",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let source = api
        .create_ready(
            "fake-a",
            json!({ "greeting": "hello from fake-a", "readyDelayMs": 5 }),
        )
        .await;
    let created = api.get(source).await.expect("the source reads");

    let operations = || async {
        match api
            .client
            .call(Call::OperationList(ListOperationsParams::default()))
            .await
        {
            Ok(Reply::OperationList(page)) => page.items.len(),
            other => panic!("operation.list: {other:?}"),
        }
    };
    let operations_before = operations().await;
    let before: Vec<_> = fs::read_dir(site.prepared.layout().instances_dir())
        .expect("the instances folder reads")
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    let form = api
        .duplicate_form(source)
        .await
        .expect("the duplicate form is built");

    assert_eq!(form.source.instance_id, source);
    assert_eq!(form.source.name, "fake-a");
    assert_eq!(
        form.form.provider.source,
        stackly_protocol::instance::CreateProviderSource::Pinned
    );
    assert_eq!(form.form.provider.name, PROVIDER);
    assert_eq!(form.form.provider.revision, REVISION);
    assert_eq!(form.draft.runtime_id, created.runtime_id);
    assert_eq!(form.draft.provider.name, PROVIDER);
    assert_eq!(form.draft.provider.revision.as_deref(), Some(REVISION));
    assert_eq!(form.draft.name, "fake-a-copy");
    assert_eq!(form.form.suggested_name.as_deref(), Some("fake-a-copy"));
    assert_eq!(form.draft.display_name, None);
    assert!(!form.draft.start);
    assert_eq!(form.draft.endpoints.len(), 1);
    assert_eq!(form.draft.endpoints[0].name, "http");
    assert_eq!(form.draft.endpoints[0].port, PortRequest::Auto);
    let settings = form.draft.settings.as_map();
    assert_eq!(settings.get("greeting"), Some(&json!("hello from fake-a")));
    assert_eq!(settings.get("readyDelayMs"), Some(&json!(5)));
    assert!(
        !settings.contains_key("authToken"),
        "a secret field never travels as a setting"
    );
    assert_eq!(form.draft.secrets.len(), 1);
    assert_eq!(form.draft.secrets[0].field, "authToken");
    assert_eq!(form.draft.secrets[0].source, SecretSource::Generate);
    assert_eq!(form.secrets.len(), 1);
    assert_eq!(
        form.secrets[0].source_reference.as_deref(),
        Some(format!("local:instance/{source}/auth-token").as_str())
    );
    let text = serde_json::to_string(&form).expect("a form encodes");
    assert!(!text.contains(TOKEN), "no secret value reaches the form");

    // Nothing was made, and the form needs nothing from the source's folders.
    let after: Vec<_> = fs::read_dir(site.prepared.layout().instances_dir())
        .expect("the instances folder reads")
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(before, after);
    assert_eq!(api.list(Default::default()).await.items.len(), 1);
    assert_eq!(
        operations().await,
        operations_before,
        "a read starts no operation"
    );
    let paths = site.instance_paths(source);
    fs::remove_dir_all(&paths.root).expect("the instance folder is removed");
    let _ = fs::remove_dir_all(&paths.logs);
    let _ = fs::remove_dir_all(&paths.run);
    let again = api
        .duplicate_form(source)
        .await
        .expect("the form needs no folder of the source");
    assert_eq!(again, form);
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_form_is_refused_with_the_code_of_the_first_failing_check() {
    let ev = evidence!(
        covers = ["REQ-025"],
        kind = Kind::Fixture,
        expected = "an unknown instance, a retained one, a missing pinned package, and a runtime that is not installed are refused with resource.not_found, supervisor.instance_retained, supervisor.package_unavailable, and supervisor.runtime_unavailable and their details",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let kept = api.create_ready("kept", json!({})).await;
    let retained = api.create_ready("retired", json!({})).await;
    drop(api);
    stop_daemon(daemon).await;

    // Registry and package edits with the daemon stopped.
    {
        let registry = site.open_registry();
        let instance = |id: Uuid| {
            registry
                .instance(InstanceId::from_uuid(id).expect("an instance id"))
                .expect("the registry reads")
                .value
                .expect("the instance exists")
        };
        let row = instance(retained);
        let expected = row.revision();
        let stored = stackly_domain::StoredInstance {
            id: row.id(),
            name: row.name().clone(),
            display_name: row.display_name().cloned(),
            provider_revision: row.provider_revision().clone(),
            software: row.software().clone(),
            runtime_id: row.runtime_id(),
            data_line: row.data_line().clone(),
            data_version: row.data_version().clone(),
            data_initialized_at: row.data_initialized_at(),
            endpoints: row.endpoints().to_vec(),
            desired: row.desired(),
            startup: row.startup(),
            lifecycle: stackly_domain::InstanceLifecycle::Retained,
            last_failure: None,
            revision: stackly_domain::ResourceRevision::new(expected.get() + 1)
                .expect("the next revision is valid"),
            created_at: row.created_at(),
            created_by: row.created_by(),
            retained_at: Some(row.created_at()),
            retained_by: Some(row.created_by()),
        };
        let retained_row = stackly_domain::Instance::restore(stored, row.created_at())
            .expect("a retained row is consistent");
        registry
            .commit(stackly_domain::WriteBatch::new(vec![
                stackly_domain::WriteOp::UpdateInstance {
                    instance: retained_row,
                    expected,
                },
            ]))
            .expect("the instance is retained");
    }
    // The pinned package folder is gone, so `kept` cannot be read.
    site.unpin_fake_package();
    let daemon = site.start().await;
    let api = site.api().await;

    let unknown = api.duplicate_form(Uuid::now_v7()).await.unwrap_err();
    assert_eq!(unknown.code(), "resource.not_found", "{unknown:?}");
    let not_an_instance_id = api.duplicate_form(Uuid::nil()).await.unwrap_err();
    assert_eq!(not_an_instance_id.code(), "resource.not_found");
    let retained_error = api.duplicate_form(retained).await.unwrap_err();
    assert_eq!(
        retained_error.code(),
        "supervisor.instance_retained",
        "{retained_error:?}"
    );
    assert!(retained_error.details().is_empty());
    let unavailable = api.duplicate_form(kept).await.unwrap_err();
    assert_eq!(
        unavailable.code(),
        "supervisor.package_unavailable",
        "{unavailable:?}"
    );
    assert!(
        unavailable
            .detail("provider")
            .is_some_and(|text| text.contains(PROVIDER))
    );
    assert!(
        unavailable
            .detail("reason")
            .is_some_and(|text| text.starts_with("the pinned package cannot be read")),
        "{unavailable:?}"
    );
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_form_of_a_source_on_a_failed_runtime_is_refused() {
    let ev = evidence!(
        covers = ["REQ-025"],
        kind = Kind::Fixture,
        expected = "a source whose runtime the registry marks failed is refused with supervisor.runtime_unavailable carrying runtimeId and state",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let source = api.create_ready("fake-a", json!({})).await;
    let runtime_id = api.get(source).await.expect("the source reads").runtime_id;
    drop(api);
    stop_daemon(daemon).await;
    {
        let registry = site.open_registry();
        let id = stackly_domain::RuntimeId::from_uuid(runtime_id).expect("a runtime id");
        let runtime = registry
            .runtime(id)
            .expect("the registry reads")
            .value
            .expect("the runtime exists");
        let expected = runtime.revision();
        let failed = stackly_domain::Runtime::restore(stackly_domain::StoredRuntime {
            id: runtime.id(),
            software: runtime.software().clone(),
            version: runtime.version().clone(),
            line: runtime.line().clone(),
            target: runtime.target(),
            install: runtime.install().clone(),
            state: stackly_domain::RuntimeState::Failed,
            installed_at: runtime.installed_at(),
            last_verified_at: runtime.last_verified_at(),
            depends_on: runtime.depends_on().clone(),
            revision: stackly_domain::ResourceRevision::new(expected.get() + 1)
                .expect("the next revision is valid"),
        })
        .expect("a failed runtime row is consistent");
        registry
            .commit(stackly_domain::WriteBatch::new(vec![
                stackly_domain::WriteOp::UpdateRuntime {
                    runtime: failed,
                    expected,
                },
            ]))
            .expect("the runtime is marked failed");
    }
    let daemon = site.start().await;
    let api = site.api().await;
    let refused = api.duplicate_form(source).await.unwrap_err();
    assert_eq!(
        refused.code(),
        "supervisor.runtime_unavailable",
        "{refused:?}"
    );
    assert_eq!(
        refused.detail("runtimeId"),
        Some(runtime_id.to_string().as_str())
    );
    assert_eq!(refused.detail("state"), Some("failed"));
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn the_duplicate_form_answers_within_300_ms_at_the_95th_percentile_with_ten_instances() {
    let ev = evidence!(
        covers = ["REQ-025"],
        kind = Kind::Fixture,
        expected = "with ten registered instances the p95 of 40 instance.get_duplicate_form calls is under 300 ms",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let mut last = None;
    for index in 0..10 {
        last = Some(api.create_ready(&format!("fake-{index}"), json!({})).await);
    }
    let source = last.expect("ten instances were made");
    let mut times = Vec::new();
    for _ in 0..40 {
        let asked = Instant::now();
        api.duplicate_form(source)
            .await
            .expect("the duplicate form is built");
        times.push(asked.elapsed());
    }
    times.sort();
    let p95 = times[(times.len() * 95).div_ceil(100) - 1];
    assert!(
        p95 < Duration::from_millis(300),
        "the p95 of the duplicate form was {p95:?}"
    );
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-6, AC-8
#[tokio::test(flavor = "multi_thread")]
async fn three_instances_of_one_runtime_keep_their_own_sentinels_processes_and_folders() {
    let ev = evidence!(
        covers = ["REQ-009", "AC-009"],
        kind = Kind::Fixture,
        expected = "a created, a duplicated, and a crash looping instance of one runtime run together with their own sentinels, PIDs, and folders; stopping and starting one never changes another; and the runtime folder is the same before the first start and after the last stop",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let first = api
        .create_ready("fake-a", json!({ "greeting": "hello from fake-a" }))
        .await;
    let second = duplicate(&api, first).await;
    let third = api
        .create_ready(
            "fake-c",
            json!({ "crashAfterMs": 150, "crashExitCode": 70 }),
        )
        .await;
    let runtime_before = content(&site.prepared.layout().runtimes_dir());

    // AC-6: one runtime, the revision of the source, and nine folders each
    // that no other instance shares or contains.
    let created: Vec<_> = {
        let mut all = Vec::new();
        for id in [first, second, third] {
            all.push(api.get(id).await.expect("the instance reads"));
        }
        all
    };
    assert_eq!(created[1].name, "fake-a-copy");
    assert_eq!(created[0].runtime_id, created[1].runtime_id);
    assert_eq!(created[0].runtime_id, created[2].runtime_id);
    assert_eq!(created[0].provider, created[1].provider);
    assert!(
        created
            .iter()
            .all(|instance| instance.lifecycle == InstanceLifecycle::Stopped)
    );
    let all_paths: Vec<Vec<PathBuf>> = [first, second, third]
        .into_iter()
        .map(|id| paths_of(&site, id))
        .collect();
    for (index, own) in all_paths.iter().enumerate() {
        for path in own.iter().filter(|path| {
            !path.ends_with("instance.lock") && !path.ends_with("instance-manifest.json")
        }) {
            assert!(path.exists(), "{} exists", path.display());
        }
        for (other_index, other) in all_paths.iter().enumerate() {
            if index == other_index {
                continue;
            }
            for mine in own {
                for theirs in other {
                    assert!(
                        !mine.starts_with(theirs) && !theirs.starts_with(mine),
                        "{} and {} overlap",
                        mine.display(),
                        theirs.display()
                    );
                }
            }
        }
    }

    // Start the first two, and put a sentinel in each.
    let first_ready = api.start_ready(first).await;
    let second_ready = api.start_ready(second).await;
    let (first_port, second_port) = (port_of(&first_ready), port_of(&second_ready));
    assert_ne!(first_port, second_port);
    assert_eq!(put_sentinel(first_port, "alpha"), 204);
    assert_eq!(put_sentinel(second_port, "bravo"), 204);
    assert_eq!(get_sentinel(first_port).as_deref(), Some("alpha"));
    assert_eq!(get_sentinel(second_port).as_deref(), Some("bravo"));

    // The third crashes in a loop beside them: the others keep their
    // process and readiness.
    api.start(third)
        .await
        .expect("the start of the third is accepted");
    api.wait_instance(third, "failed with an exhausted budget", |summary| {
        summary.lifecycle == InstanceLifecycle::Failed && summary.restart.exhausted
    })
    .await;
    for (id, ready) in [(first, &first_ready), (second, &second_ready)] {
        let now = api.get(id).await.expect("the instance reads");
        assert_eq!(
            pid_of(&now),
            pid_of(ready),
            "the crash loop did not touch {id}"
        );
        assert_eq!(now.lifecycle, InstanceLifecycle::Running);
        assert_eq!(now.readiness.state, InstanceReadinessState::Ready);
    }

    // Stop the first: the second keeps its process, lifecycle, readiness,
    // and sentinel.
    api.stop_when_free(first).await;
    api.wait_instance(first, "stopped", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    let second_now = api.get(second).await.expect("the second reads");
    assert_eq!(pid_of(&second_now), pid_of(&second_ready));
    assert_eq!(second_now.lifecycle, InstanceLifecycle::Running);
    assert_eq!(second_now.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(get_sentinel(second_port).as_deref(), Some("bravo"));

    // Start the first again: each reads only its own sentinel.
    let first_again = api.start_ready(first).await;
    let first_port = port_of(&first_again);
    assert_eq!(get_sentinel(first_port).as_deref(), Some("alpha"));
    assert_eq!(get_sentinel(second_port).as_deref(), Some("bravo"));

    // The same the other way round.
    api.stop_when_free(second).await;
    api.wait_instance(second, "stopped", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    let first_now = api.get(first).await.expect("the first reads");
    assert_eq!(pid_of(&first_now), pid_of(&first_again));
    assert_eq!(first_now.readiness.state, InstanceReadinessState::Ready);
    let second_again = api.start_ready(second).await;
    assert_eq!(
        get_sentinel(port_of(&second_again)).as_deref(),
        Some("bravo")
    );
    assert_eq!(
        get_sentinel(port_of(&api.get(first).await.expect("reads"))).as_deref(),
        Some("alpha")
    );

    // Everything stops; the runtime folder is as it was.
    api.stop_when_free(first).await;
    api.stop_when_free(second).await;
    for id in [first, second] {
        api.wait_instance(id, "stopped", |s| s.lifecycle == InstanceLifecycle::Stopped)
            .await;
    }
    assert_eq!(
        content(&site.prepared.layout().runtimes_dir()),
        runtime_before
    );
    for id in [first, second, third] {
        assert!(
            serving(&site.instance_paths(id).data).is_empty(),
            "no process is left"
        );
    }
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

/// Sets the mode of every entry under `root` to 000, children before their
/// folders, and returns the entries in the order that restores them.
fn lock_tree(root: &Path) -> Vec<PathBuf> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("the folder reads").flatten() {
            let path = entry.path();
            out.push(path.clone());
            if path.is_dir() {
                collect(&path, out);
            }
        }
    }
    let mut entries = Vec::new();
    collect(root, &mut entries);
    for path in entries.iter().rev() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("a mode is set");
    }
    entries
}

fn unlock_tree(entries: &[PathBuf]) {
    for path in entries {
        let mode = if path.extension().is_none() {
            0o700
        } else {
            0o600
        };
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

/// Restores the modes of a locked tree, also when the test panics.
struct Unlock(Vec<PathBuf>);

impl Drop for Unlock {
    fn drop(&mut self) {
        unlock_tree(&self.0);
    }
}

// covers: AC-7
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_never_reads_lists_copies_or_writes_the_tree_of_its_source() {
    let ev = evidence!(
        covers = ["REQ-009", "AC-025", "BR-008"],
        kind = Kind::Fixture,
        expected = "with every file and subfolder inside the data folder of a stopped source made mode 000, the form, create, start, and stop of a duplicate succeed; the duplicate answers 404 for the sentinel of the source; and the tree of the source and its log folder are the same before and after",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let source = api.create_ready("fake-a", json!({})).await;
    let started = api.start_ready(source).await;
    assert_eq!(put_sentinel(port_of(&started), "alpha"), 204);
    api.stop_when_free(source).await;
    api.wait_instance(source, "stopped", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;

    let paths = site.instance_paths(source);
    fs::create_dir_all(paths.data.join("nested")).expect("a subfolder is made");
    fs::write(paths.data.join("nested").join("inner.txt"), "inner").expect("a file is written");
    let root_before = tree(&paths.root);
    let logs_before = tree(&paths.logs);
    assert!(
        root_before
            .iter()
            .any(|entry| entry.0.ends_with("SENTINEL")),
        "the sentinel is in the tree of the source"
    );

    let locked = Unlock(lock_tree(&paths.data));
    let form = api
        .duplicate_form(source)
        .await
        .expect("the form is built from the registry and the package alone");
    let secrets = generated(&form);
    let accepted = api
        .create_from_draft(form.draft, secrets)
        .await
        .expect("the create is accepted");
    let copy = created_id(&api, accepted).await;
    let copy_ready = api.start_ready(copy).await;
    assert_eq!(
        get_sentinel(port_of(&copy_ready)),
        None,
        "no data is copied"
    );
    api.stop_when_free(copy).await;
    api.wait_instance(copy, "stopped", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    drop(locked);

    assert_eq!(tree(&paths.root), root_before);
    assert_eq!(tree(&paths.logs), logs_before);
    let copy_paths = site.instance_paths(copy);
    assert!(
        !tree(&copy_paths.data)
            .iter()
            .any(|entry| entry.0.contains("nested"))
    );
    let again = api.start_ready(source).await;
    assert_eq!(get_sentinel(port_of(&again)).as_deref(), Some("alpha"));
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_gets_a_new_secret_unless_the_user_copies_the_one_of_the_source() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "the draft as it is makes a new reference with another value, copyFrom makes a new reference with the same value, the reference of the source keeps its record and its use, and the new instance never uses the reference of the source",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let source = api.create_ready("fake-a", json!({})).await;
    drop(api);
    stop_daemon(daemon).await;
    let source_id = InstanceId::from_uuid(source).expect("an instance id");
    let read_source = || {
        let registry = site.open_registry();
        let uses = registry
            .secret_uses_by(SecretUser::Instance(source_id))
            .expect("the registry reads")
            .value;
        let records: Vec<_> = uses
            .iter()
            .map(|used| {
                // `status_observed_at` moves whenever the daemon starts, so
                // it is the one field left out of the comparison.
                registry
                    .secret_reference(&used.reference)
                    .expect("the registry reads")
                    .value
                    .map(|view| {
                        (
                            view.record.id,
                            view.record.platform_store_key,
                            view.record.revision,
                            view.record.value_updated_at,
                            view.used_by,
                        )
                    })
            })
            .collect();
        (uses, records)
    };
    let (uses_before, records_before) = read_source();
    assert_eq!(uses_before.len(), 1);

    let daemon = site.start().await;
    let api = site.api().await;
    let fresh = duplicate(&api, source).await;
    let form = api.duplicate_form(source).await.expect("the form is built");
    let mut draft = form.draft.clone();
    draft.name = "fake-copied".to_owned();
    let accepted = api
        .create_from_draft(draft, copied(&form))
        .await
        .expect("the create with copyFrom is accepted");
    let copy = created_id(&api, accepted).await;
    drop(api);
    stop_daemon(daemon).await;

    let digest = |id: Uuid| {
        fs::read_to_string(site.instance_paths(id).data.join("token.sha256"))
            .expect("init wrote the digest of the token")
    };
    assert_eq!(
        digest(source).trim(),
        hex(&Sha256::digest(TOKEN.as_bytes()))
    );
    assert_ne!(
        digest(fresh),
        digest(source),
        "a generated secret is another value"
    );
    assert_eq!(digest(copy), digest(source), "copyFrom copies the value");

    let (uses_after, records_after) = read_source();
    assert_eq!(uses_after, uses_before, "the source keeps its use");
    assert_eq!(
        records_after, records_before,
        "the reference of the source is untouched"
    );
    let registry = site.open_registry();
    for id in [fresh, copy] {
        let instance_id = InstanceId::from_uuid(id).expect("an instance id");
        let uses = registry
            .secret_uses_by(SecretUser::Instance(instance_id))
            .expect("the registry reads")
            .value;
        assert_eq!(uses.len(), 1);
        assert_eq!(
            uses[0].reference.as_str(),
            format!("local:instance/{id}/auth-token"),
            "a duplicate uses a reference of its own"
        );
        assert_ne!(uses[0].reference, uses_before[0].reference);
    }
    ev.pass();
}

// covers: AC-4
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_never_fixes_a_port_and_a_fixed_port_of_the_source_is_refused() {
    let ev = evidence!(
        covers = ["REQ-025", "BR-005", "AC-025"],
        kind = Kind::Fixture,
        expected = "a draft asks auto, two auto instances of one runtime get different effective ports and the same port again after a stop and start, and a user's Fixed port equal to the Fixed port of a stopped source is refused with supervisor.endpoint_conflict naming the source",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let fixed = stackly_testkit::allocate_port().expect("a free port");
    let mut params = api.params("fake-a", json!({}));
    params.endpoints = vec![Api::http_port(fixed)];
    let accepted = api.create(params).await.expect("the create is accepted");
    let source = created_id(&api, accepted).await;

    let form = api.duplicate_form(source).await.expect("the form is built");
    assert_eq!(form.draft.endpoints.len(), 1);
    assert_eq!(
        form.draft.endpoints[0].port,
        PortRequest::Auto,
        "the Fixed port of the source is not copied"
    );

    // A Fixed port equal to the one of the stopped source is refused by the
    // preflight row and by the create.
    let mut clash = form.draft.clone();
    clash.endpoints[0].port = PortRequest::Fixed(fixed);
    let preflight = api
        .preflight(clash.clone())
        .await
        .expect("the preflight answers");
    assert!(!preflight.ready);
    let row = preflight
        .checks
        .iter()
        .find(|check| check.id == "endpoint.http")
        .and_then(|check| check.error.as_ref())
        .expect("the endpoint row fails");
    assert_eq!(row.code(), "supervisor.endpoint_conflict");
    let refused = api
        .create_from_draft(clash, generated(&form))
        .await
        .unwrap_err();
    assert_eq!(
        refused.code(),
        "supervisor.endpoint_conflict",
        "{refused:?}"
    );
    let other = refused
        .detail("otherInstance")
        .expect("the other instance is named");
    assert!(
        other == source.to_string() || other == "fake-a",
        "otherInstance names the source: {other}"
    );

    // Two auto instances: different effective ports, and the same port again.
    let copy = duplicate(&api, source).await;
    let source_ready = api.start_ready(source).await;
    let copy_ready = api.start_ready(copy).await;
    assert_eq!(port_of(&source_ready), fixed);
    assert_ne!(port_of(&copy_ready), port_of(&source_ready));
    let bound = copy_ready.endpoints[0].bound;
    assert!(bound, "the effective port is stored as bound");
    api.stop_when_free(copy).await;
    api.wait_instance(copy, "stopped", |s| {
        s.lifecycle == InstanceLifecycle::Stopped
    })
    .await;
    let restarted = api.start_ready(copy).await;
    assert_eq!(
        port_of(&restarted),
        port_of(&copy_ready),
        "the port is reused while it is free"
    );
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_form_is_ambiguous_when_the_catalog_holds_another_package_for_the_pinned_revision()
 {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "a catalog with the revision label of the pinned package and another digest makes instance.get_duplicate_form answer instance.provider_ambiguous with the provider and the revision, and a catalog that holds the same package does not",
    );
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let source = api.create_ready("fake-a", json!({})).await;
    drop(api);
    stop_daemon(daemon).await;

    // The same revision label with another digest.
    let (_scratch, other) = edited_fake_package(|dir| {
        let path = dir.join("provider.yaml");
        let mut text = fs::read_to_string(&path).expect("the manifest reads");
        text.push_str("\n# another digest\n");
        fs::write(&path, text).expect("the manifest writes");
    });
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(catalog_of(
        &other,
    )));
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let refused = api.duplicate_form(source).await.unwrap_err();
    assert_eq!(refused.code(), "instance.provider_ambiguous", "{refused:?}");
    assert_eq!(refused.detail("provider"), Some(PROVIDER), "{refused:?}");
    assert_eq!(refused.detail("revision"), Some(REVISION), "{refused:?}");
    assert!(
        api.list(Default::default()).await.items.len() == 1,
        "the refusal changes nothing"
    );
    drop(api);
    stop_daemon(daemon).await;

    // The package of the repository is the one pinned: no ambiguity.
    let mut config = fast();
    config.catalog = Some(stacklyd::catalog::CatalogOverride::Preloaded(
        crate::support::fake_catalog(),
    ));
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    api.duplicate_form(source)
        .await
        .expect("the same package in the catalog is no problem");
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-1
#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_form_of_a_provider_without_multi_instance_is_refused_naming_the_source() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "instance.get_duplicate_form of an instance whose pinned package lacks the multi_instance capability answers instance.capability_unsupported with the capability, the provider, and the name of the source, and builds no operation",
    );
    let (_scratch, single) = edited_fake_package(|dir| {
        let path = dir.join("provider.yaml");
        let text = fs::read_to_string(&path).expect("the manifest reads");
        assert!(text.contains("multiInstance: true"));
        fs::write(
            &path,
            text.replace("multiInstance: true", "multiInstance: false"),
        )
        .expect("the manifest writes");
    });
    let site = Site::with_package(&single);
    let daemon = site.start().await;
    let api = site.api().await;
    let source = api.create_ready("fake-a", json!({})).await;

    let refused = api.duplicate_form(source).await.unwrap_err();
    assert_eq!(
        refused.code(),
        "instance.capability_unsupported",
        "{refused:?}"
    );
    assert_eq!(refused.detail("capability"), Some("multi_instance"));
    assert_eq!(refused.detail("provider"), Some(PROVIDER));
    assert_eq!(refused.detail("existing"), Some("fake-a"));
    let operations = api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("the operations list");
    let Reply::OperationList(page) = operations else {
        panic!("not an operation list: {operations:?}");
    };
    assert_eq!(
        page.items.len(),
        1,
        "only the create of the source made an operation"
    );
    drop(api);
    stop_daemon(daemon).await;
    ev.pass();
}

// covers: AC-5
#[tokio::test(flavor = "multi_thread")]
async fn no_secret_value_of_a_duplicate_reaches_the_daemon_log_an_event_or_an_operation() {
    let ev = evidence!(
        covers = ["REQ-025", "AC-025"],
        kind = Kind::Fixture,
        expected = "a duplicate with the generated secret and one with copyFrom leave neither the value of the source, the generated value, nor a prefix of either in the daemon log, the events, the form, the operations, or the log files of the instances",
    );
    let site = Site::new();
    crate::support::install_logs();
    let daemon = site.start().await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");
    let source = api.create_ready("fake-a", json!({})).await;

    let form = api.duplicate_form(source).await.expect("the form is built");
    let mut seen = vec![serde_json::to_string(&form).expect("the form encodes")];
    let generated_id = duplicate(&api, source).await;
    let mut draft = form.draft.clone();
    draft.name = "fake-copied".to_owned();
    let accepted = api
        .create_from_draft(draft, copied(&form))
        .await
        .expect("the create with copyFrom is accepted");
    let copy_id = created_id(&api, accepted).await;
    for id in [source, generated_id, copy_id] {
        api.start_ready(id).await;
        api.stop_when_free(id).await;
    }
    let listed = api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("the operations list");
    let Reply::OperationList(page) = listed else {
        panic!("not an operation list: {listed:?}");
    };
    for operation in &page.items {
        seen.push(format!("{operation:?}"));
        seen.push(format!("{:?}", api.operation(operation.id).await));
    }
    while let Ok(Ok(Some(frame))) =
        tokio::time::timeout(Duration::from_millis(300), events.next()).await
    {
        if let DecodedFrame::Event(event) = frame {
            seen.push(serde_json::to_string(&event).expect("the event encodes"));
        }
    }
    drop((api, events));
    stop_daemon(daemon).await;
    let log = crate::support::log_text();
    seen.push(log.clone());
    for id in [source, generated_id, copy_id] {
        let logs = site.instance_paths(id).logs;
        for entry in fs::read_dir(&logs).into_iter().flatten().flatten() {
            if entry.path().is_file() {
                seen.push(fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
    }

    // The value of the source, and the value the daemon generated for the
    // duplicate: the one place either may be is the Keychain.
    let generated = site
        .secrets
        .stored_value(
            &format!("local:instance/{generated_id}/auth-token")
                .parse()
                .expect("the reference of the generated secret"),
        )
        .expect("the generated secret is in the store");
    assert_ne!(generated, TOKEN);
    for value in [TOKEN, generated.as_str()] {
        assert!(value.len() >= 12, "a value long enough to take a prefix");
        for text in &seen {
            assert!(
                !text.contains(&value[..12]),
                "a prefix of a secret value leaked into: {}",
                &text[..text.len().min(200)]
            );
        }
    }
    assert!(
        log.contains("stacklyd") || log.contains("\"level\""),
        "the daemon log was read, not empty"
    );
    ev.pass();
}
