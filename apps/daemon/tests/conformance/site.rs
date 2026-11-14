//! Root preparation and the end of a run (spec 0036, *Persiapan root* and
//! *Root dan kepemilikan*): the only part of a run that does not go
//! through the socket. It picks the artifact, installs the runtime into a
//! marked test root the way `Site::open` of the NATS harness does, starts
//! the daemon in this process with the package as its whole catalog, and
//! at the end stops every instance, shuts the daemon down, reaps what is
//! left, and removes the root through `remove_checked`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
use stackly_core::provider::scenarios::Scenario;
use stackly_core::provider::{ArtifactStatus, PackageCheck, ValidatedPackage, VersionArtifact};
use stackly_core::runtime::fixture::install_cached_artifact;
use stackly_core::supervisor::fixture::install_fake_runtime;
use stackly_core::{PreparedLayout, daemon_info, prepare_layout};
use stackly_domain::{Registry, WriteBatch, WriteOp};
use stackly_ipc::{CallOptions, Client, ConnectPolicy};
use stackly_platform::{Clock, Platform, RootSet, SystemClock, host_target};
use stackly_platform_macos::MacosPlatform;
use stackly_protocol::handshake::{ClientInfo, ClientKind};
use stackly_protocol::{Call, ErrorBody, Timestamp};
use stackly_storage::{OpenOptions, RegistryOpen};
use stackly_testkit::{
    FakeSecretStore, ProcessTable, ReapGuard, RuntimeCache, TestRoot, fake_server,
};
use stacklyd::catalog::CatalogOverride;
use stacklyd::lock::{DaemonLock, LockOwner};
use stacklyd::server::{self, DaemonConfig, DaemonHandle};
use uuid::Uuid;

use super::report::RuntimeFacts;
use super::steps::{self, Ctx, Limits};
use super::{
    Failure, Report, RunRequest, SCENARIO_LIMIT, ScenarioResult, StepResult, StepStatus,
    interrupted,
};

/// The name of the data root: a space and non ASCII letters, like the
/// native harnesses, so every path a package writes has to survive them.
pub const ROOT_NAME: &str = "Test Root Ünï cödé";
/// Who can fill the runtime cache (spec 0002).
pub const CACHE_OWNER: &str = "runtime maintainer";

/// The daemon config of the harness: the supervisor constants of the
/// in process tests, so waits stay short.
#[must_use]
pub fn fast_config() -> DaemonConfig {
    let mut config = DaemonConfig::default();
    let supervisor = &mut config.supervisor;
    supervisor.helper_term_grace = Duration::from_millis(200);
    supervisor.readiness_refresh_interval = Duration::from_millis(200);
    supervisor.login_secret_retry_interval = Duration::from_millis(200);
    supervisor.restart_base_delay = Duration::from_millis(50);
    supervisor.restart_max_delay = Duration::from_millis(400);
    supervisor.spool_drain_interval = Duration::from_millis(50);
    config
}

/// The artifact a run installs.
struct Picked<'a> {
    artifact: &'a VersionArtifact,
    native: bool,
}

/// Picks the artifact for the host target (*Persiapan root* step 2), or
/// says why the run is blocked.
fn pick<'a>(package: &'a ValidatedPackage, line: Option<&str>) -> Result<Picked<'a>, String> {
    let target = host_target().ok_or_else(|| "this host is not a modeled target".to_owned())?;
    let on_host: Vec<&VersionArtifact> = package
        .versions()
        .artifacts()
        .iter()
        .filter(|artifact| artifact.target == target)
        .collect();
    let runnable = |artifact: &&&VersionArtifact| {
        matches!(
            artifact.status,
            ArtifactStatus::Fixture { .. } | ArtifactStatus::Available { .. }
        )
    };
    let artifact = match line {
        Some(line) => *on_host
            .iter()
            .find(|artifact| artifact.line.to_string() == line)
            .ok_or_else(|| format!("the package has no artifact of line {line} for this host"))?,
        None => {
            let candidates: Vec<&&VersionArtifact> = on_host.iter().filter(runnable).collect();
            match candidates.as_slice() {
                [one] => **one,
                [] => match on_host.first() {
                    Some(artifact) => *artifact,
                    None => return Err("the package has no artifact for this host".to_owned()),
                },
                many => {
                    let lines: Vec<String> = many
                        .iter()
                        .map(|artifact| artifact.line.to_string())
                        .collect();
                    return Err(format!(
                        "the package has more than one runnable line; pick one of {}",
                        lines.join(", ")
                    ));
                }
            }
        }
    };
    match &artifact.status {
        ArtifactStatus::Fixture { .. } => Ok(Picked {
            artifact,
            native: false,
        }),
        ArtifactStatus::Available { .. } => Ok(Picked {
            artifact,
            native: true,
        }),
        _ => Err(format!("artifact {} is blocked", artifact.id)),
    }
}

/// A typed connection to the daemon that takes JSON params and answers
/// JSON, so the steps read like the contract they check.
pub struct Daemon {
    client: Client,
}

impl Daemon {
    /// Calls `method` with `content` (none for a method without params).
    ///
    /// # Errors
    ///
    /// The daemon's error, or `harness.bad_call` when the params do not
    /// make a call of this build.
    pub async fn call(
        &self,
        method: &str,
        content: Option<Value>,
    ) -> Result<Value, Box<ErrorBody>> {
        self.call_with(method, content, None).await
    }

    /// [`call`](Self::call) with a fresh idempotency key, for a method that
    /// starts an operation.
    ///
    /// # Errors
    ///
    /// The errors of [`call`](Self::call).
    pub async fn call_keyed(&self, method: &str, content: Value) -> Result<Value, Box<ErrorBody>> {
        let key = format!("conformance-{}", Uuid::now_v7());
        self.call_with(method, Some(content), Some(key)).await
    }

    async fn call_with(
        &self,
        method: &str,
        content: Option<Value>,
        key: Option<String>,
    ) -> Result<Value, Box<ErrorBody>> {
        let call = typed_call(method, content).map_err(|err| {
            Box::new(harness_error(&format!(
                "the params of {method} do not decode: {err}"
            )))
        })?;
        let options = match key {
            Some(key) => CallOptions::default().with_idempotency_key(key),
            None => CallOptions::default(),
        };
        let reply = self.client.call_detailed(call, options).await?;
        let value = serde_json::to_value(&reply)
            .map_err(|err| Box::new(harness_error(&format!("a reply does not encode: {err}"))))?;
        Ok(value.get("content").cloned().unwrap_or(Value::Null))
    }
}

/// The typed call of `method` with `content`.
///
/// Without `content` the method is read as one without params first and
/// then with empty params, as the wire reads the absent `content` of a
/// method whose params are optional (`decode_optional_params`). The plain
/// decode of [`Call`] has no such rule, so a method that gains optional
/// params would otherwise stop decoding here while it still works on the
/// socket.
fn typed_call(method: &str, content: Option<Value>) -> Result<Call, serde_json::Error> {
    match content {
        Some(content) => serde_json::from_value(json!({ "type": method, "content": content })),
        None => serde_json::from_value(json!({ "type": method }))
            .or_else(|_| serde_json::from_value(json!({ "type": method, "content": {} }))),
    }
}

/// An error the harness makes itself, shaped like a daemon error.
fn harness_error(message: &str) -> ErrorBody {
    serde_json::from_value(json!({
        "code": "harness.bad_call",
        "category": "internal",
        "message": message,
        "retryable": false,
        "resource": null,
        "fields": [],
    }))
    .expect("a harness error decodes as an error body")
}

/// Prepares the root, runs every scenario, and runs the end of run order
/// (spec 0036, *Persiapan root*, *Langkah skenario*, *Root dan
/// kepemilikan*).
pub async fn run_prepared(
    request: &RunRequest,
    check: &PackageCheck,
    scenarios: &[Scenario],
    run_id: Uuid,
    report: &mut Report,
) {
    let package = check
        .package
        .as_ref()
        .expect("run_prepared runs only for a publishable package");
    let picked = match pick(package, request.line.as_deref()) {
        Ok(picked) => picked,
        Err(reason) => {
            report.block(&reason, "provider owner");
            return;
        }
    };
    report.set_kind(if picked.native { "native" } else { "fixture" });
    let cache_file = if picked.native {
        let ArtifactStatus::Available { sha256, size, .. } = &picked.artifact.status else {
            unreachable!("a native pick is available");
        };
        match RuntimeCache::open().and_then(|cache| cache.entry(&sha256.to_string(), *size)) {
            Some(file) => Some(file),
            None => {
                report.block(
                    &format!(
                        "artifact {} is not in the runtime cache",
                        picked.artifact.id
                    ),
                    CACHE_OWNER,
                );
                return;
            }
        }
    } else {
        if package.software().as_str() != "fake" {
            report.block(
                &format!(
                    "a fixture artifact runs only on software fake, not {}",
                    package.software()
                ),
                "provider owner",
            );
            return;
        }
        None
    };
    let root = match TestRoot::new_marked(&run_id.to_string()) {
        Ok(root) => root,
        Err(err) => {
            report.block(&format!("cannot create the test root: {err}"), "harness");
            return;
        }
    };
    report.root_folders = vec![root.path().to_path_buf(), root.run_dir().to_path_buf()];
    let data_root = root.path().join(ROOT_NAME);
    let made = std::fs::create_dir(&data_root).and_then(|()| {
        std::fs::set_permissions(&data_root, std::fs::Permissions::from_mode(0o700))
    });
    let site = match made {
        Ok(()) => open(&root, &data_root, package, &picked, cache_file.as_deref()).await,
        Err(err) => Err(format!("cannot create the data root: {err}")),
    };
    match site {
        Ok(site) => {
            report.runtime = Some(site.runtime.clone());
            run_scenarios(&site, scenarios, report).await;
            end_of_run(site, report).await;
        }
        Err(reason) => report.block(&reason, "harness"),
    }
    remove_root(root, request.keep, report);
}

/// A prepared root with a running daemon.
struct Site {
    /// Reaps the processes under the data root on drop.
    reap: ReapGuard,
    /// The two folders of the test root, whose ASCII names `ps` prints as
    /// they are.
    folders: [PathBuf; 2],
    handle: DaemonHandle,
    _lock: DaemonLock,
    ctx: Ctx,
    runtime: RuntimeFacts,
}

/// *Persiapan root* steps 3 to 5.
async fn open(
    root: &TestRoot,
    data_root: &Path,
    package: &ValidatedPackage,
    picked: &Picked<'_>,
    cache_file: Option<&Path>,
) -> Result<Site, String> {
    let secrets = Arc::new(FakeSecretStore::new());
    let roots = RootSet::under_override(data_root, root.run_dir())
        .map_err(|err| format!("the test root is not a valid root override: {err}"))?;
    let platform: Platform = MacosPlatform::with_roots(roots)
        .map_err(|err| format!("the platform cannot be built: {err}"))?
        .with_secrets(Arc::clone(&secrets) as Arc<dyn stackly_platform::SecretStore>);
    let prepared =
        prepare_layout(&platform).map_err(|err| format!("the layout cannot be prepared: {err}"))?;
    // `ps` writes the non ASCII bytes of the data root name escaped, so the
    // guard matches on the ASCII folder of the test root that holds it.
    let reap = ReapGuard::new(root.path(), prepared.layout().roots().runtime());
    let (runtime_id, sha256) = install(&platform, &prepared, package, picked, cache_file)?;
    let owner = LockOwner::new(
        std::process::id(),
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        "0.1.0",
    );
    let lock = DaemonLock::acquire(&prepared.layout().daemon_lock(), owner)
        .map_err(|err| format!("the daemon lock is taken: {err}"))?;
    let log = stacklyd::log_sink::LogSink::start(
        Box::new(std::io::sink()),
        stacklyd::log_sink::LogSinkConfig::DAEMON,
    )
    .map_err(|err| format!("the log writer does not start: {err}"))?;
    let facts = stacklyd::map::StartFacts::new(
        Uuid::now_v7(),
        Timestamp::from_jiff(SystemClock.now()),
        true,
        std::process::id(),
    );
    let mut config = fast_config();
    let entry = CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package.clone()),
        tile: TileHint {
            mark: "Pg".to_owned(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
        origin: stackly_core::catalog::EntryOrigin::Catalog,
    });
    config.catalog = Some(CatalogOverride::Preloaded(Catalog::for_tests(&[entry])));
    let deps = server::DaemonDeps::new(platform.clone(), prepared.clone(), facts, Arc::new(log))
        .with_config(config);
    let handle = server::start(deps)
        .await
        .map_err(|err| format!("the daemon does not start: {err}"))?;
    handle.started().await;
    let (client, _) = Client::connect(
        &prepared.layout().daemon_socket(),
        ConnectPolicy::same_user(own_uid()),
        ClientInfo {
            kind: ClientKind::Cli,
            version: "0.1.0".to_owned(),
            pid: Some(std::process::id()),
            interactive: Some(false),
        },
    )
    .await
    .map_err(|err| format!("the client does not connect: {err}"))?;
    let runtime = RuntimeFacts {
        software: package.software().to_string(),
        version: picked.artifact.version.to_string(),
        sha256,
        source: if picked.native { "managed" } else { "fixture" },
    };
    let ctx = Ctx {
        daemon: Daemon { client },
        package: package.clone(),
        runtime_id,
        layout: prepared,
        limits: Limits::of(package),
        artifact_version: picked.artifact.version.to_string(),
    };
    Ok(Site {
        reap,
        folders: [root.path().to_path_buf(), root.run_dir().to_path_buf()],
        handle,
        _lock: lock,
        ctx,
        runtime,
    })
}

/// Installs the runtime and closes the registry; returns the runtime ID and
/// the SHA-256 of what was installed.
fn install(
    platform: &Platform,
    prepared: &PreparedLayout,
    package: &ValidatedPackage,
    picked: &Picked<'_>,
    cache_file: Option<&Path>,
) -> Result<(Uuid, String), String> {
    let RegistryOpen::Ready { registry, .. } = OpenOptions::new()
        .open(platform, prepared.layout(), daemon_info().version)
        .map_err(|err| format!("the registry does not open: {err}"))?
    else {
        return Err("a fresh registry does not open ready".to_owned());
    };
    let (runtime, record) = match cache_file {
        Some(file) => install_cached_artifact(
            platform,
            prepared,
            &registry,
            package,
            &picked.artifact.id,
            file,
        )
        .map_err(|err| format!("the cached artifact does not install: {err}"))?,
        None => {
            let (runtime, record) = install_fake_runtime(
                prepared.layout(),
                package,
                &fake_server::binary(),
                platform.clock().now(),
            )
            .map_err(|err| format!("the fake runtime does not install: {err}"))?;
            registry
                .commit(WriteBatch::new(vec![
                    WriteOp::InsertArtifact(record.clone()),
                    WriteOp::InsertRuntime(runtime.clone()),
                ]))
                .map_err(|err| format!("the runtime cannot be committed: {err}"))?;
            (runtime, record)
        }
    };
    // The registry closes here, so the daemon opens it alone.
    drop(registry);
    Ok((runtime.id().as_uuid(), record.sha256.to_string()))
}

/// Runs every scenario, each within [`SCENARIO_LIMIT`].
async fn run_scenarios(site: &Site, scenarios: &[Scenario], report: &mut Report) {
    for scenario in scenarios {
        if interrupted() {
            report.interrupted = true;
            break;
        }
        let result =
            match tokio::time::timeout(SCENARIO_LIMIT, steps::run_scenario(&site.ctx, scenario))
                .await
            {
                Ok(result) => result,
                Err(_) => {
                    // The steps that ran are lost with the future; the stop of
                    // the end of run order still ends every instance.
                    ScenarioResult {
                        name: scenario.name.to_string(),
                        expect: scenario.expect,
                        steps: vec![StepResult {
                            name: "scenario",
                            status: StepStatus::Fail,
                            duration_ms: u64::try_from(SCENARIO_LIMIT.as_millis())
                                .unwrap_or(u64::MAX),
                            detail: None,
                            failure: Some(Failure::reason(
                                "scenario_timeout",
                                "the scenario ran past 15 minutes",
                            )),
                        }],
                    }
                }
            };
        if report.handover.is_none()
            && let Some(step) = result.step("handover")
            && let Some(state) = step
                .detail
                .as_ref()
                .and_then(|detail| detail["handover"].as_str())
        {
            report.handover = Some(if state == "proven" {
                "proven"
            } else {
                "unproven"
            });
        }
        report.scenarios.push(result);
        if interrupted() {
            report.interrupted = true;
            break;
        }
    }
}

/// End of run steps 1 to 3: stop every instance, shut the daemon down, and
/// reap what still names the root.
async fn end_of_run(site: Site, report: &mut Report) {
    steps::stop_everything(&site.ctx).await;
    site.handle
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
    if let Ok(table) = ProcessTable::snapshot() {
        let mut seen = Vec::new();
        for entry in site.folders.iter().flat_map(|folder| table.under(folder)) {
            if seen.contains(&entry.pid) {
                continue;
            }
            seen.push(entry.pid);
            let command = super::process::command_name(entry.pid);
            report
                .cleanup
                .processes_left
                .push(json!({ "pid": entry.pid, "command": command }));
        }
    }
    let reaped = site
        .reap
        .reap()
        .and_then(|_| ProcessTable::reap_under(&site.folders[1], stackly_testkit::REAP_GRACE));
    if let Err(err) = reaped {
        report.cleanup.refused = Some(format!("a process outlived the reap: {err}"));
    }
    drop(site.reap);
}

/// End of run step 5: `remove_checked`, or `keep_data` for `--keep`.
fn remove_root(root: TestRoot, keep: bool, report: &mut Report) {
    if keep {
        match root.keep_data() {
            Ok(path) => report.cleanup.kept = Some(path),
            Err(refused) => report.cleanup.refused = Some(refused.to_string()),
        }
        return;
    }
    match root.remove_checked() {
        Ok(()) => report.cleanup.root_removed = true,
        Err(refused) => report.cleanup.refused = Some(refused.to_string()),
    }
}

/// The uid of this process, read from the metadata of the package folder
/// of the daemon crate, without `unsafe`.
fn own_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(env!("CARGO_MANIFEST_DIR"))
        .map(|meta| meta.uid())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The end of run stop calls `instance.preview_stop_all` without params.
    // When that stopped decoding (protocol 4.6 gave it optional params), the
    // harness stopped nothing and an interrupted run left its process to
    // the reap.
    #[test]
    fn a_call_without_content_decodes_for_a_method_with_optional_params() {
        let call = typed_call("instance.preview_stop_all", None).expect("the call decodes");

        assert!(matches!(call, Call::InstancePreviewStopAll(params) if !params.override_leases));
    }

    #[test]
    fn a_call_without_content_still_decodes_for_a_method_without_params() {
        assert!(typed_call("daemon.restart_preview", None).is_ok());
    }

    #[test]
    fn a_call_without_content_does_not_decode_for_a_method_with_required_params() {
        assert!(typed_call("instance.force_stop", None).is_err());
    }
}
