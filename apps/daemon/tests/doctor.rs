//! `doctor.run` through the real daemon (spec 0034 step 5, AC-1 to AC-6,
//! AC-22): the fake provider in a test root on the macOS platform, a real
//! `sleep` whose PID the registry records for a running instance, a real
//! TCP listener of this test on a fixed port, and a registry in recovery
//! mode. The doctor reports each, and the process, the lock file, the data,
//! the operations, and the registry stay exactly as they were.
#![cfg(target_os = "macos")]

#[path = "supervisor/support.rs"]
mod support;

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command};

use serde_json::json;
use sha2::{Digest, Sha256};
use stackly_domain::{
    InstanceId, Pid, ProcessIdentity, ProcessObservation, ProcessStart, Registry, WriteBatch,
    WriteOp,
};
use stackly_protocol::doctor::{
    DoctorActionKind, DoctorCheck, DoctorCheckId, DoctorFindingCode, DoctorParams, DoctorReport,
    DoctorSeverity, PortOwnerKind,
};
use stackly_protocol::instance::{IdentityCheckName, IdentityCheckResult, InstanceLifecycle};
use stackly_protocol::operation::ListOperationsParams;
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::allocate_port;
use stacklyd::server::DaemonHandle;
use uuid::Uuid;

use crate::support::{Api, Site};

/// A `sleep` of this test, killed and reaped on drop.
struct Sleeper(Child);

impl Sleeper {
    fn start() -> Self {
        Self(
            Command::new("/bin/sleep")
                .arg("600")
                .spawn()
                .expect("sleep starts"),
        )
    }

    fn pid(&self) -> Pid {
        Pid::new(self.0.id()).expect("a pid is not 0")
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn doctor(api: &Api, params: DoctorParams) -> Result<DoctorReport, Box<ErrorBody>> {
    match api.client.call(Call::DoctorRun(params)).await? {
        Reply::DoctorRun(report) => Ok(*report),
        other => panic!("not a doctor reply: {other:?}"),
    }
}

fn check(report: &DoctorReport, id: DoctorCheckId) -> &DoctorCheck {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the report has {id:?}"))
}

async fn operation_count(api: &Api) -> usize {
    match api
        .client
        .call(Call::OperationList(ListOperationsParams::default()))
        .await
        .expect("operation.list answers")
    {
        Reply::OperationList(page) => page.items.len(),
        other => panic!("not an operation page: {other:?}"),
    }
}

/// The SHA-256 of every regular file under `folder`, by relative path.
fn digests(folder: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = entry.file_type().expect("the entry has a type");
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let bytes = std::fs::read(&path).expect("a file reads");
                let relative = path
                    .strip_prefix(folder)
                    .expect("under the folder")
                    .display()
                    .to_string();
                let digest: String = Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                found.push((relative, digest));
            }
        }
    }
    found.sort();
    found
}

async fn stop(daemon: DaemonHandle) {
    daemon
        .shutdown(
            stackly_protocol::events::ShutdownReason::Requested,
            std::future::pending(),
        )
        .await;
}

// covers: AC-1, AC-2, AC-3, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_recorded_pid_that_a_sleep_now_holds_is_pid_reused_and_nothing_is_touched() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let id = api.create_ready("reused", json!({})).await;
    let running = api.start_ready(id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    let instance = InstanceId::from_uuid(id).expect("an instance id");

    // Record a `sleep` of this test as the instance's process, with a start
    // time one second later than its own, through the daemon's registry.
    let sleeper = Sleeper::start();
    let platform = site.platform.clone();
    let facts = platform
        .processes()
        .inspect(sleeper.pid())
        .expect("sleep can be inspected")
        .expect("sleep runs");
    let registry = daemon.core().registry().expect("the registry is open");
    let original = registry
        .latest_process_observation(instance)
        .expect("the observation reads")
        .value
        .expect("a running instance has an observation");
    let planted = ProcessIdentity {
        pid: sleeper.pid(),
        start: ProcessStart::new(facts.start.seconds() + 1, facts.start.micros())
            .expect("a start time"),
        executable: original.identity.executable.clone(),
        session: facts.session,
        boot: original.identity.boot,
    };
    registry
        .commit(WriteBatch::new(vec![WriteOp::RecordProcessObservation(
            ProcessObservation {
                instance,
                identity: planted.clone(),
                observed_at: stackly_platform::Clock::now(platform.clock()),
            },
        )]))
        .expect("the observation is stored");

    let paths = site.instance_paths(id);
    let lock_before = std::fs::read(&paths.lock).expect("instance.lock reads");
    let data_before = digests(&paths.data);
    let operations_before = operation_count(&api).await;
    let generation_before = registry
        .known_instance_ids()
        .expect("the ids read")
        .generation;

    let report = doctor(&api, DoctorParams::default())
        .await
        .expect("the doctor runs");

    assert!(!report.recovery);
    let ids: Vec<_> = report.checks.iter().map(|check| check.id).collect();
    assert_eq!(
        ids,
        [
            DoctorCheckId::Daemon,
            DoctorCheckId::Registry,
            DoctorCheckId::Permissions,
            DoctorCheckId::DiskSpace,
            DoctorCheckId::Runtimes,
            DoctorCheckId::ProviderPackages,
            DoctorCheckId::Endpoints,
            DoctorCheckId::Processes,
            DoctorCheckId::InstanceFiles,
            DoctorCheckId::InstanceFailures,
            DoctorCheckId::ConfigDrift,
            DoctorCheckId::Secrets,
            DoctorCheckId::Operations,
        ]
    );
    let processes = check(&report, DoctorCheckId::Processes);
    assert_eq!(processes.severity, DoctorSeverity::Error);
    let finding = processes
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::PidReused)
        .expect("pid_reused");
    assert_eq!(
        finding.instance.as_ref().map(|instance| instance.id),
        Some(id)
    );
    assert!(
        finding.title.starts_with("PID reused · reused"),
        "{}",
        finding.title
    );
    let evidence = finding.evidence.as_ref().expect("evidence");
    assert_eq!(evidence.identity.lock, None, "the lock is never probed");
    let result = |name| {
        evidence
            .identity
            .checks
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.result)
    };
    assert_eq!(
        result(IdentityCheckName::PidExists),
        Some(IdentityCheckResult::Passed)
    );
    assert_eq!(
        result(IdentityCheckName::StartMatches),
        Some(IdentityCheckResult::Failed)
    );
    let recorded = evidence.recorded.as_ref().expect("the recorded process");
    let current = evidence.current.as_ref().expect("the process now");
    assert_eq!(recorded.pid, sleeper.pid().get());
    assert_eq!(current.pid, sleeper.pid().get());
    assert_ne!(recorded.started_at, current.started_at);
    assert_eq!(current.executable.as_deref(), Some("/bin/sleep"));
    assert_eq!(current.same_user, Some(true));
    assert_eq!(
        finding.action.as_ref().map(|action| action.kind),
        Some(DoctorActionKind::ShowInstance)
    );
    assert!(u64::from(report.errors) >= 1);

    // Nothing changed: the process, its start, the lock file, the data, the
    // operations, the registry, and the lifecycle.
    let after = platform
        .processes()
        .inspect(sleeper.pid())
        .expect("sleep can be inspected")
        .expect("sleep still runs");
    assert_eq!(after.start, facts.start);
    assert_eq!(
        std::fs::read(&paths.lock).expect("instance.lock reads"),
        lock_before
    );
    assert_eq!(digests(&paths.data), data_before);
    assert_eq!(operation_count(&api).await, operations_before);
    assert_eq!(
        registry
            .known_instance_ids()
            .expect("the ids read")
            .generation,
        generation_before,
        "the doctor writes nothing to the registry"
    );
    let summary = api.get(id).await.expect("the instance reads");
    assert_eq!(summary.lifecycle, InstanceLifecycle::Running);

    // Put the real observation back, so the stop finds its process.
    registry
        .commit(WriteBatch::new(vec![WriteOp::RecordProcessObservation(
            original,
        )]))
        .expect("the observation is restored");
    api.stop_when_free(id).await;
    drop(registry);
    stop(daemon).await;
}

// covers: AC-1, AC-5, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_fixed_port_this_test_listens_on_is_port_in_use_with_this_process_as_owner() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let port = allocate_port().expect("a free port");
    let mut params = api.params("blocked", json!({}));
    params.endpoints = vec![Api::http_port(port)];
    let accepted = api
        .create(params)
        .await
        .expect("the create is accepted")
        .accepted
        .expect("a create starts an operation");
    api.wait_final(accepted.operation.id).await;
    let id: Uuid = accepted
        .operation
        .resource
        .as_ref()
        .map(|resource| resource.id.parse().expect("an instance id"))
        .expect("the operation names the instance");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the test listens");

    let report = doctor(
        &api,
        DoctorParams {
            instance_id: Some(id),
            checks: vec![DoctorCheckId::Endpoints, DoctorCheckId::Daemon],
        },
    )
    .await
    .expect("the doctor runs");
    let ids: Vec<_> = report.checks.iter().map(|check| check.id).collect();
    assert_eq!(
        ids,
        [DoctorCheckId::Daemon, DoctorCheckId::Endpoints],
        "table order"
    );
    assert_eq!(report.scope.as_ref().map(|scope| scope.id), Some(id));
    let endpoints = check(&report, DoctorCheckId::Endpoints);
    let finding = &endpoints.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::PortInUse);
    assert_eq!(
        finding.severity,
        DoctorSeverity::Warning,
        "a stopped instance"
    );
    let owner = finding.port_owner.as_ref().expect("an owner");
    assert_eq!(owner.kind, PortOwnerKind::Process);
    assert_eq!(
        owner.process.as_ref().map(|process| process.pid),
        Some(std::process::id())
    );
    let detail = |key: &str| {
        finding
            .details
            .iter()
            .find(|detail| detail.key == key)
            .map(|detail| detail.value.clone())
    };
    assert_eq!(detail("port"), Some(port.to_string()));
    assert_eq!(detail("problem").as_deref(), Some("in_use"));
    let action = finding.action.as_ref().expect("change_port");
    assert_eq!(action.kind, DoctorActionKind::ChangePort);
    assert_eq!(action.endpoint.as_deref(), Some("http"));
    let suggested = action.port.expect("a suggested port");
    assert!(suggested > port);
    assert_eq!(detail("suggestedPort"), Some(suggested.to_string()));
    assert!(
        finding.explanation.contains("Not owned by Stackly"),
        "{}",
        finding.explanation
    );

    // The doctor never moved the port.
    let summary = api.get(id).await.expect("the instance reads");
    assert!(
        summary
            .endpoints
            .iter()
            .any(|endpoint| endpoint.name == "http" && endpoint.port == Some(port)),
        "{:?}",
        summary.endpoints
    );
    drop(listener);
    stop(daemon).await;
}

// covers: AC-1 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_client_check_a_repeat_or_an_unknown_instance_is_refused() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    for checks in [
        vec![DoctorCheckId::Connection],
        vec![DoctorCheckId::Registry, DoctorCheckId::Registry],
    ] {
        let refused = doctor(
            &api,
            DoctorParams {
                instance_id: None,
                checks,
            },
        )
        .await
        .expect_err("refused");
        assert_eq!(refused.code(), "request.invalid_params", "{refused:?}");
    }
    let unknown = doctor(
        &api,
        DoctorParams {
            instance_id: Some(Uuid::now_v7()),
            checks: Vec::new(),
        },
    )
    .await
    .expect_err("not found");
    assert_eq!(unknown.code(), "resource.not_found", "{unknown:?}");
    stop(daemon).await;
}

// covers: AC-6, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_scratch_folder_without_an_operation_is_a_warning_and_stays() {
    let site = Site::new();
    let daemon = site.start().await;
    let api = site.api().await;
    let folder = site
        .prepared
        .layout()
        .operations_dir()
        .join("0190f000-0000-7000-8000-0000000000aa");
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    // Made an hour ago: a folder modified in the last 10 minutes is skipped.
    let young = site.prepared.layout().operations_dir().join("just-made");
    std::fs::create_dir_all(&young).expect("a young scratch folder");
    std::fs::File::open(&folder)
        .expect("the folder opens")
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .expect("the folder time is set");
    // An hour old too, but a file and a symlink are not scratch folders.
    let ds_store = site.prepared.layout().operations_dir().join(".DS_Store");
    std::fs::write(&ds_store, b"finder").expect("a file in operations/");
    std::fs::File::open(&ds_store)
        .expect("the file opens")
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .expect("the file time is set");
    let link = site.prepared.layout().operations_dir().join("a-link");
    std::os::unix::fs::symlink(&folder, &link).expect("a symlink in operations/");
    let hour_ago = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(1))
        .expect("an hour ago is a time")
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let touched = Command::new("/usr/bin/touch")
        .args(["-h", "-d", &hour_ago])
        .arg(&link)
        .status()
        .expect("touch runs");
    assert!(touched.success(), "touch -h sets the symlink time");
    let report = doctor(
        &api,
        DoctorParams {
            instance_id: None,
            checks: vec![DoctorCheckId::Operations],
        },
    )
    .await
    .expect("the doctor runs");
    let operations = check(&report, DoctorCheckId::Operations);
    assert_eq!(operations.severity, DoctorSeverity::Warning);
    let finding = &operations.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::ScratchLeftover);
    assert_eq!(
        finding.subject.as_deref(),
        Some("operations/0190f000-0000-7000-8000-0000000000aa")
    );
    assert_eq!(
        operations.findings.len(),
        1,
        "the young folder, the file, and the symlink are skipped"
    );
    assert!(folder.is_dir(), "the doctor removes nothing");
    assert!(young.is_dir());
    assert!(ds_store.is_file());
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the symlink stays")
            .file_type()
            .is_symlink()
    );
    stop(daemon).await;
}

// covers: AC-4, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn in_recovery_mode_the_registry_is_an_error_and_the_checks_that_need_it_are_unavailable() {
    let site = Site::new();
    let db = site.prepared.layout().registry_db();
    let mut bytes = std::fs::read(&db).expect("the registry file reads");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&db, &bytes).expect("the registry file is written");
    let before = std::fs::read(&db).expect("the registry file reads");
    let daemon = site.start().await;
    let api = site.api().await;
    let report = doctor(&api, DoctorParams::default())
        .await
        .expect("the doctor answers in recovery mode");
    assert!(report.recovery);
    let registry = check(&report, DoctorCheckId::Registry);
    assert_eq!(registry.severity, DoctorSeverity::Error);
    assert_eq!(registry.findings[0].code, DoctorFindingCode::RecoveryMode);
    for ran in [DoctorCheckId::Daemon, DoctorCheckId::Permissions] {
        assert!(
            check(&report, ran)
                .findings
                .iter()
                .all(|finding| finding.code != DoctorFindingCode::CheckUnavailable),
            "{ran:?} runs"
        );
    }
    for skipped in [
        DoctorCheckId::Runtimes,
        DoctorCheckId::ProviderPackages,
        DoctorCheckId::Endpoints,
        DoctorCheckId::Processes,
        DoctorCheckId::InstanceFiles,
        DoctorCheckId::InstanceFailures,
        DoctorCheckId::ConfigDrift,
        DoctorCheckId::Secrets,
        DoctorCheckId::Operations,
    ] {
        let check = check(&report, skipped);
        assert_eq!(check.severity, DoctorSeverity::Warning);
        assert_eq!(check.findings[0].code, DoctorFindingCode::CheckUnavailable);
        assert!(
            check.findings[0]
                .details
                .iter()
                .any(|detail| detail.key == "reason" && detail.value == "recovery_mode"),
            "{skipped:?}"
        );
    }
    stop(daemon).await;
    assert_eq!(
        std::fs::read(&db).expect("the registry file reads"),
        before,
        "the registry file stays as it was"
    );
}

/// A remedy of the fake package for a start that crashed after it began to
/// listen, with the PID file `fake.pid` in the data folder.
const FAKE_REMEDY: &str = r#"remedies:
  - name: stale_pid
    when:
      failureCodes: [crash_loop, process_exited]
      excerptContains: "listening on"
    evidence:
      pidFile: "${instance.data}/fake.pid"
    title: Fake found an old fake.pid
    steps:
      - text: Stackly never removes this file.
      - text: If the PID below is not a Fake server for this data folder, remove the file yourself.
        path: "${instance.data}/fake.pid"
      - text: Then start the instance again.
        command: "${runtime.executable} --port=${endpoint.http.port}"
"#;

/// `path` with the home folder written `~`, as the report writes it.
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .and_then(|home| home.canonicalize().ok());
    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

// covers: AC-2, AC-9, AC-10, AC-22 (spec 0034)
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_start_carries_the_provider_remedy_and_its_pid_file_evidence() {
    let (_scratch, package) = support::edited_fake_package(|dir| {
        let path = dir.join("provider.yaml");
        let mut text = std::fs::read_to_string(&path).expect("the manifest reads");
        text.push_str(FAKE_REMEDY);
        std::fs::write(&path, text).expect("the manifest writes");
    });
    let site = Site::with_package(&package);
    let mut config = support::fast();
    config.supervisor.restart_base_delay = std::time::Duration::from_millis(20);
    config.supervisor.restart_max_delay = std::time::Duration::from_millis(400);
    let daemon = site.start_with(config).await;
    let api = site.api().await;
    let id = api
        .create_ready(
            "crashy",
            json!({ "crashAfterMs": 150, "crashExitCode": 3, "readyDelayMs": 60_000 }),
        )
        .await;
    let operation = api
        .start(id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start begins an operation")
        .operation
        .id;
    api.wait_final(operation).await;
    let failed = api.get(id).await.expect("the instance reads");
    assert_eq!(failed.lifecycle, InstanceLifecycle::Failed);

    let sleeper = Sleeper::start();
    let paths = site.instance_paths(id);
    let pid_file = paths.data.join("fake.pid");
    let text = format!("{}\n{}\n", sleeper.pid().get(), paths.data.display());
    std::fs::write(&pid_file, &text).expect("the PID file is written");
    let data_before = digests(&paths.data);
    let lock_before = std::fs::read(&paths.lock).expect("instance.lock reads");

    let report = doctor(
        &api,
        DoctorParams {
            instance_id: Some(id),
            checks: vec![DoctorCheckId::InstanceFailures],
        },
    )
    .await
    .expect("the doctor runs");

    let failures = check(&report, DoctorCheckId::InstanceFailures);
    let finding = &failures.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::StartFailed);
    let remedy = finding.remedy.as_ref().expect("the remedy matches");
    assert_eq!(remedy.provider, "testing/fake");
    assert_eq!(remedy.name, "stale_pid");
    assert_eq!(remedy.title, "Fake found an old fake.pid");
    assert_eq!(remedy.steps.len(), 3);
    assert_eq!(
        remedy.steps[1].path.as_deref(),
        Some(tilde(&pid_file).as_str())
    );
    let command = remedy.steps[2].command.as_deref().expect("a command");
    let port = failed.endpoints[0].port.expect("the instance has a port");
    assert!(
        command.ends_with(&format!("stackly-fake-server --port={port}"))
            || command.ends_with(&format!("stackly-fake-server' --port={port}")),
        "{command}"
    );
    assert!(!command.contains('~'), "a command keeps absolute paths");
    let evidence = remedy.pid_file.as_ref().expect("the PID file is read");
    assert_eq!(evidence.path, tilde(&pid_file));
    assert_eq!(evidence.pid, Some(sleeper.pid().get()));
    assert_eq!(
        evidence.state,
        stackly_protocol::doctor::PidFileState::OtherProcess
    );
    let process = evidence.process.as_ref().expect("the sleep is shown");
    assert_eq!(process.executable.as_deref(), Some("/bin/sleep"));
    assert_eq!(process.same_user, Some(true));
    assert!(
        finding.explanation.ends_with(&format!(
            "PID {} now belongs to /bin/sleep, not Fake Provider.",
            sleeper.pid().get()
        )),
        "{}",
        finding.explanation
    );

    assert_eq!(
        std::fs::read_to_string(&pid_file).expect("the PID file stays"),
        text
    );
    assert_eq!(digests(&paths.data), data_before, "the data folder stays");
    assert_eq!(
        std::fs::read(&paths.lock).expect("instance.lock reads"),
        lock_before
    );
    let alive = site
        .platform
        .processes()
        .inspect(sleeper.pid())
        .expect("sleep can be inspected");
    assert!(alive.is_some(), "the sleep still runs");
    stop(daemon).await;
}
