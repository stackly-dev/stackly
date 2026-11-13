//! The doctor on fakes (spec 0034 step 4): every daemon check in table
//! order, what each one finds, the limits of the runner, recovery mode, and
//! that a run never probes or takes a lock, signals, spawns, writes a file,
//! or writes the registry.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use stackly_core::doctor::{
    Action, CapacityFailure, CapacityOutcome, DaemonFacts, DegradedSubsystem, DiskReading, Doctor,
    DoctorError, DoctorLimits, DoctorMode, DoctorRequest, DoctorSources, FindingFacts,
    IdentityReason, PortOwnerFacts, PortProblemKind, Report, SummaryFacts,
};
use stackly_core::supervisor::BindOutcome;
use stackly_domain::doctor::{
    CheckUnavailableReason, DAEMON_CHECKS, DoctorCheckId, DoctorFindingCode, DoctorSeverity,
};
use stackly_domain::{
    DesiredState, IdentityCheck, IdentityCheckName, InstanceId, InstanceLifecycle, NewJournal,
    OperationJournal, OperationTrigger, QuickCheck, RecoveryHint, Registry, Transport,
    UnrecognizedValue, WriteBatch, WriteOp,
};
use stackly_platform::RootKind;
use stackly_testkit::{FakeVolume, OperationBuilder, ProcessOp};

use crate::common::{LOOPBACK, Plant, Rig, Spec};

const TB: u64 = 1_000_000_000_000;

/// The process calls a doctor run must never make.
const FORBIDDEN: [ProcessOp; 10] = [
    ProcessOp::ProbeLock,
    ProcessOp::LockInstance,
    ProcessOp::Signal,
    ProcessOp::ForceKill,
    ProcessOp::SpawnMain,
    ProcessOp::SpawnHelper,
    ProcessOp::ContinueStopped,
    ProcessOp::Discard,
    ProcessOp::Resume,
    ProcessOp::TerminateHelperGroup,
];

fn sources(rig: &Rig) -> DoctorSources {
    DoctorSources::new(
        Arc::clone(&rig.platform),
        Arc::clone(&rig.layout),
        DoctorMode::Ready {
            supervisor: rig.sup.clone(),
            schema_version: 12,
        },
        DaemonFacts::default(),
    )
}

/// The free space of the data root, read the way `storage.get_capacity`
/// reads it.
fn capacity(rig: &Rig, total: u64, available: u64) -> CapacityOutcome {
    let data = rig.layout.canonical_root(RootKind::Data).to_path_buf();
    rig.fake
        .fs()
        .set_volume(&data, FakeVolume::new(total, available));
    stackly_core::storage::capacity(rig.platform.fs(), &data, rig.fake_now())
        .map(DiskReading::from)
        .map_err(|error| CapacityFailure::new(CheckUnavailableReason::Io, error.to_string()))
}

async fn run_with(rig: &Rig, doctor: &Doctor, request: DoctorRequest) -> Report {
    let reading = capacity(rig, TB, TB / 2);
    doctor
        .run(sources(rig), request, async move { reading })
        .await
        .expect("the run starts")
}

async fn run(rig: &Rig) -> Report {
    run_with(rig, &Doctor::default(), DoctorRequest::default()).await
}

/// The process calls, file writes, and registry generation, to compare
/// before and after a run.
fn footprint(rig: &Rig) -> (Vec<usize>, usize, u64) {
    let calls = FORBIDDEN
        .iter()
        .map(|op| rig.processes().count(*op))
        .collect();
    let generation = rig
        .registry
        .known_instance_ids()
        .expect("the ids read")
        .generation
        .get();
    (calls, rig.fake.fs().mutations().len(), generation)
}

fn check(report: &Report, id: DoctorCheckId) -> &stackly_core::doctor::CheckReport {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the report has {}", id.code()))
}

fn codes(report: &Report, id: DoctorCheckId) -> Vec<DoctorFindingCode> {
    check(report, id)
        .findings
        .iter()
        .map(|finding| finding.code)
        .collect()
}

/// Sets the modification time of the symlink `path` itself, not its
/// target, to `minutes` before `now`.
fn link_modified_ago(path: &std::path::Path, now: jiff::Timestamp, minutes: i64) {
    let when = now
        .checked_sub(jiff::SignedDuration::from_mins(minutes))
        .expect("a test time is in range")
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let status = std::process::Command::new("/usr/bin/touch")
        .args(["-h", "-d", &when])
        .arg(path)
        .status()
        .expect("touch runs");
    assert!(status.success(), "touch -h sets the symlink time");
}

/// Sets the modification time of the folder `path` to `minutes` before
/// `now`.
fn modified_ago(path: &std::path::Path, now: jiff::Timestamp, minutes: u64) {
    let when = std::time::SystemTime::from(now) - Duration::from_secs(minutes * 60);
    std::fs::File::open(path)
        .expect("the folder opens")
        .set_modified(when)
        .expect("the folder time is set");
}

fn stopped_spec() -> Spec {
    Spec {
        request: Some(Rig::free_port()),
        ..Spec::default()
    }
}

// covers: AC-1, AC-2 (spec 0034)
#[tokio::test]
async fn a_healthy_rig_reports_every_daemon_check_in_table_order_and_changes_nothing() {
    let rig = Rig::new().await;
    rig.instance(&stopped_spec());
    let before = footprint(&rig);
    let report = run(&rig).await;
    let ids: Vec<_> = report.checks.iter().map(|check| check.id).collect();
    assert_eq!(ids, DAEMON_CHECKS);
    for check in &report.checks {
        assert_eq!(
            check.severity,
            DoctorSeverity::Ok,
            "{}: {:?}",
            check.id.code(),
            check.findings
        );
        assert!(check.findings.is_empty());
    }
    assert_eq!(report.tally.errors, 0);
    assert_eq!(report.tally.warnings, 0);
    assert!(!report.recovery);
    assert!(report.started_at <= report.finished_at);
    assert_eq!(
        check(&report, DoctorCheckId::Registry).summary,
        SummaryFacts::Registry {
            schema_version: Some(12)
        }
    );
    assert!(check(&report, DoctorCheckId::DiskSpace).capacity.is_some());
    assert_eq!(
        check(&report, DoctorCheckId::Endpoints).summary,
        SummaryFacts::Checked(1)
    );
    assert_eq!(footprint(&rig), before, "no lock, signal, spawn, or write");
}

// covers: AC-2, AC-3 (spec 0034)
#[tokio::test]
async fn a_recorded_pid_of_another_executable_is_pid_reused_with_passive_evidence() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Running,
        desired: DesiredState::Running,
        ..stopped_spec()
    });
    let identity = rig.plant(
        id,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.processes().update_facts(identity.pid, |facts| {
        facts.executable = "/bin/sleep".into();
    });
    let before = footprint(&rig);
    let report = run(&rig).await;
    let processes = check(&report, DoctorCheckId::Processes);
    assert_eq!(processes.severity, DoctorSeverity::Error);
    let finding = &processes.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::PidReused);
    assert_eq!(
        finding.instance.as_ref().map(|instance| instance.id),
        Some(id)
    );
    assert_eq!(finding.action, Some(Action::ShowInstance(id)));
    let FindingFacts::Identity(facts) = &finding.facts else {
        panic!("identity facts");
    };
    assert_eq!(facts.reason, IdentityReason::PidReused);
    assert_eq!(facts.evidence.lock(), None, "the lock is never probed");
    assert_eq!(
        facts.evidence.check(IdentityCheckName::ExecutableMatches),
        IdentityCheck::Failed
    );
    assert_eq!(
        facts.evidence.check(IdentityCheckName::HoldsLock),
        IdentityCheck::NotChecked
    );
    assert_eq!(
        facts.recorded.as_ref().map(|recorded| recorded.pid),
        Some(identity.pid)
    );
    assert_eq!(
        facts
            .current
            .as_ref()
            .map(|current| current.executable.clone()),
        Some("/bin/sleep".into())
    );
    assert_eq!(footprint(&rig), before);
    assert!(rig.processes().signals(identity.pid).is_empty());
    assert_eq!(
        rig.registry
            .instance(id)
            .expect("the instance reads")
            .value
            .map(|instance| instance.lifecycle()),
        Some(InstanceLifecycle::Running),
        "the lifecycle stays"
    );
}

// covers: AC-3 (spec 0034)
#[tokio::test]
async fn a_gone_process_is_unproven_only_while_the_lifecycle_is_unknown() {
    let rig = Rig::new().await;
    let unknown = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Unknown,
        ..stopped_spec()
    });
    let stopped = rig.instance(&stopped_spec());
    for id in [unknown, stopped] {
        let identity = rig.plant(
            id,
            Plant {
                stopped: false,
                holds_lock: false,
            },
        );
        rig.processes().exit(identity.pid, None);
    }
    let report = run(&rig).await;
    let processes = check(&report, DoctorCheckId::Processes);
    assert_eq!(processes.findings.len(), 1, "{:?}", processes.findings);
    let finding = &processes.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::IdentityUnproven);
    assert_eq!(
        finding.instance.as_ref().map(|instance| instance.id),
        Some(unknown)
    );
    let FindingFacts::Identity(facts) = &finding.facts else {
        panic!("identity facts");
    };
    assert_eq!(facts.reason, IdentityReason::ProcessGone);
}

// covers: AC-3 (spec 0034)
#[tokio::test]
async fn a_matching_process_that_does_not_hold_the_lock_is_unproven_and_a_suspended_one_is_a_warning()
 {
    let rig = Rig::new().await;
    let running = Spec {
        lifecycle: InstanceLifecycle::Running,
        desired: DesiredState::Running,
        ..stopped_spec()
    };
    let loose = rig.instance(&running);
    rig.plant(
        loose,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    let suspended = rig.instance(&running);
    rig.plant(
        suspended,
        Plant {
            stopped: true,
            holds_lock: true,
        },
    );
    let healthy = rig.instance(&running);
    rig.plant(healthy, Plant::live());
    let before = footprint(&rig);
    let report = run(&rig).await;
    let found: Vec<_> = check(&report, DoctorCheckId::Processes)
        .findings
        .iter()
        .map(|finding| {
            (
                finding.instance.as_ref().map(|instance| instance.id),
                finding.code,
                finding.severity,
            )
        })
        .collect();
    assert!(found.contains(&(
        Some(loose),
        DoctorFindingCode::IdentityUnproven,
        DoctorSeverity::Error
    )));
    assert!(found.contains(&(
        Some(suspended),
        DoctorFindingCode::ProcessSuspended,
        DoctorSeverity::Warning
    )));
    assert_eq!(
        found.len(),
        2,
        "the healthy process is no finding: {found:?}"
    );
    assert_eq!(footprint(&rig), before);
}

// covers: AC-5 (spec 0034)
#[tokio::test]
async fn a_held_fixed_port_of_a_failed_instance_is_an_error_with_its_owner_and_a_suggestion() {
    let rig = Rig::new().await;
    let port = Rig::free_port();
    let failed = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Failed,
        request: Some(port),
        last_failure: Some("endpoint_unavailable"),
        ..Spec::default()
    });
    let stopped_port = Rig::free_port();
    let stopped = rig.instance(&Spec {
        request: Some(stopped_port),
        ..Spec::default()
    });
    rig.ports.answer(port, BindOutcome::InUse);
    rig.ports.answer(stopped_port, BindOutcome::InUse);
    // The recorded process of a third instance listens on the first port.
    let owner_instance = rig.instance(&stopped_spec());
    let listener = rig.plant(
        owner_instance,
        Plant {
            stopped: false,
            holds_lock: false,
        },
    );
    rig.processes().update_facts(listener.pid, |facts| {
        facts.executable = "/opt/mysqld".into()
    });
    rig.processes()
        .listen(listener.pid, Transport::Tcp, LOOPBACK, port);
    let before = footprint(&rig);
    let report = run(&rig).await;
    let endpoints = check(&report, DoctorCheckId::Endpoints);
    assert_eq!(endpoints.severity, DoctorSeverity::Error);
    let of = |id: InstanceId| {
        endpoints
            .findings
            .iter()
            .find(|finding| finding.instance.as_ref().map(|instance| instance.id) == Some(id))
            .expect("a finding for the instance")
    };
    let error = of(failed);
    assert_eq!(error.code, DoctorFindingCode::PortInUse);
    assert_eq!(error.severity, DoctorSeverity::Error);
    assert_eq!(error.subject.as_deref(), Some("http"));
    let FindingFacts::Port {
        port: held,
        problem,
        suggestion,
        owner,
        ..
    } = &error.facts
    else {
        panic!("port facts");
    };
    assert_eq!(held.get(), port);
    assert_eq!(*problem, PortProblemKind::Bind(BindOutcome::InUse));
    let suggested = suggestion.expect("a free port is suggested");
    assert!(suggested.get() > port);
    let PortOwnerFacts::StacklyInstance {
        owner: process,
        instance,
    } = owner
    else {
        panic!("the other instance owns it: {owner:?}");
    };
    assert_eq!(process.pid, listener.pid);
    assert_eq!(instance.id, owner_instance);
    assert_eq!(
        process.executable.as_deref(),
        Some(std::path::Path::new("/opt/mysqld"))
    );
    assert_eq!(
        error.action,
        Some(Action::ChangePort {
            instance: failed,
            endpoint: "http".parse().expect("an endpoint name"),
            port: suggested,
        })
    );
    let warning = of(stopped);
    assert_eq!(warning.severity, DoctorSeverity::Warning);
    let FindingFacts::Port { owner, .. } = &warning.facts else {
        panic!("port facts");
    };
    assert_eq!(
        *owner,
        PortOwnerFacts::Unknown,
        "nobody of this user listens"
    );
    assert_eq!(footprint(&rig), before, "the doctor never changes a port");
}

// covers: AC-6 (spec 0034)
#[tokio::test]
async fn an_operation_that_needs_recovery_and_a_folder_without_a_row_are_reported_and_kept() {
    let rig = Rig::new().await;
    let operation = OperationBuilder::new()
        .state(stackly_domain::OperationState::NeedsRecovery)
        .recovery_hint(RecoveryHint::Abandon)
        .failure("extract_failed", false)
        .build();
    let journal = OperationJournal::new(NewJournal {
        operation_id: operation.id(),
        request: None,
        trigger: OperationTrigger::background(),
        resumable: false,
        input: stackly_domain::JournalPayload::empty(),
        expected_revision: None,
        parent: None,
    })
    .expect("a journal");
    rig.registry
        .commit(WriteBatch::new(vec![
            WriteOp::InsertOperation(operation.clone()),
            WriteOp::InsertJournal(journal),
        ]))
        .expect("the operation is stored");
    let folder = rig.layout.layout().operations_dir();
    let stray = folder.join("not-an-operation");
    let orphan = folder.join("0190f000-0000-7000-8000-000000000001");
    let young = folder.join(uuid::Uuid::now_v7().to_string());
    let young_stray = folder.join("just-made");
    let with_row = folder.join(operation.id().to_string());
    for path in [&stray, &orphan, &young, &young_stray, &with_row] {
        std::fs::create_dir_all(path).expect("a scratch folder");
    }
    // Neither a file such as `.DS_Store` nor a symlink is a scratch folder,
    // however old.
    let ds_store = folder.join(".DS_Store");
    std::fs::write(&ds_store, b"finder").expect("a file in operations/");
    let link = folder.join("a-link");
    std::os::unix::fs::symlink(&stray, &link).expect("a symlink in operations/");
    // The age is the folder's own modification time, against the clock of
    // the platform: an hour for the old ones, a minute for the young ones.
    let now = rig.fake_now();
    for (path, minutes) in [
        (&stray, 60),
        (&orphan, 60),
        (&with_row, 60),
        (&young, 1),
        (&young_stray, 1),
        (&ds_store, 60),
    ] {
        modified_ago(path, now, minutes);
    }
    link_modified_ago(&link, now, 60);
    let before = footprint(&rig);
    let report = run(&rig).await;
    let operations = check(&report, DoctorCheckId::Operations);
    let recovery = operations
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::NeedsRecovery)
        .expect("needs_recovery");
    assert_eq!(recovery.action, Some(Action::ShowOperation(operation.id())));
    let FindingFacts::NeedsRecovery {
        operation: stored,
        error_code,
        ..
    } = &recovery.facts
    else {
        panic!("recovery facts");
    };
    assert_eq!(stored.recovery_hint(), Some(RecoveryHint::Abandon));
    assert_eq!(error_code.as_deref(), Some("extract_failed"));
    let mut leftovers: Vec<_> = operations
        .findings
        .iter()
        .filter(|finding| finding.code == DoctorFindingCode::ScratchLeftover)
        .map(|finding| finding.subject.clone().unwrap_or_default())
        .collect();
    leftovers.sort();
    assert_eq!(
        leftovers,
        [
            "operations/0190f000-0000-7000-8000-000000000001",
            "operations/not-an-operation"
        ]
    );
    for path in [&stray, &orphan, &young, &young_stray, &with_row] {
        assert!(path.is_dir(), "{} stays", path.display());
    }
    assert!(ds_store.is_file(), "the file stays");
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the symlink stays")
            .file_type()
            .is_symlink()
    );
    assert_eq!(footprint(&rig), before);
}

// covers: AC-8 (spec 0034)
#[tokio::test]
async fn registry_findings_come_from_quick_check_and_the_scan_of_coded_columns() {
    let rig = Rig::new().await;
    rig.registry.inner.set_quick_check(QuickCheck::Failed {
        messages: vec!["CHECK constraint failed in instances".to_owned()],
    });
    rig.registry
        .inner
        .set_unrecognized(vec![UnrecognizedValue::new(
            "operations",
            7,
            "kind",
            "retain_instance",
        )]);
    let report = run(&rig).await;
    assert_eq!(
        codes(&report, DoctorCheckId::Registry),
        [
            DoctorFindingCode::QuickCheckFailed,
            DoctorFindingCode::UnrecognizedValue
        ]
    );
    let unrecognized = &check(&report, DoctorCheckId::Registry).findings[1];
    assert_eq!(unrecognized.subject.as_deref(), Some("operations.kind#7"));

    rig.registry
        .inner
        .fail_health_checks(Some(stackly_domain::RegistryError::Unavailable));
    let report = run(&rig).await;
    let registry = check(&report, DoctorCheckId::Registry);
    assert_eq!(registry.severity, DoctorSeverity::Warning);
    let FindingFacts::Unavailable { reason, .. } = &registry.findings[0].facts else {
        panic!("check_unavailable");
    };
    assert_eq!(*reason, CheckUnavailableReason::Io);
}

// covers: AC-4 (spec 0034)
#[tokio::test]
async fn recovery_mode_runs_four_checks_and_never_reads_the_registry() {
    let rig = Rig::new().await;
    rig.instance(&stopped_spec());
    let calls = rig.registry.inner.health_check_calls();
    let mut sources = sources(&rig);
    sources.mode = DoctorMode::Recovery {
        reason: "schema_newer".to_owned(),
        message: "the registry was written by a newer Stackly".to_owned(),
    };
    let reading = capacity(&rig, TB, TB / 2);
    let report = Doctor::default()
        .run(sources, DoctorRequest::default(), async move { reading })
        .await
        .expect("the run starts");
    assert!(report.recovery);
    assert_eq!(
        codes(&report, DoctorCheckId::Registry),
        [DoctorFindingCode::RecoveryMode]
    );
    for ran in [
        DoctorCheckId::Daemon,
        DoctorCheckId::Permissions,
        DoctorCheckId::DiskSpace,
    ] {
        assert_eq!(
            check(&report, ran).severity,
            DoctorSeverity::Ok,
            "{}",
            ran.code()
        );
    }
    for skipped in DAEMON_CHECKS
        .iter()
        .filter(|check| !check.runs_in_recovery())
    {
        let finding = &check(&report, *skipped).findings[0];
        assert_eq!(finding.code, DoctorFindingCode::CheckUnavailable);
        assert!(matches!(
            finding.facts,
            FindingFacts::Unavailable {
                reason: CheckUnavailableReason::RecoveryMode,
                ..
            }
        ));
    }
    assert_eq!(rig.registry.inner.health_check_calls(), calls);
}

// covers: AC-1 (spec 0034)
#[tokio::test]
async fn a_check_past_its_limit_is_a_timeout_and_a_later_run_does_not_start_it_again() {
    let rig = Rig::new().await;
    rig.instance(&stopped_spec());
    let limit = Duration::from_millis(300);
    let doctor = Doctor::new(DoctorLimits::default().with_check(limit));
    let request = DoctorRequest::new(None, vec![DoctorCheckId::Secrets, DoctorCheckId::Daemon]);
    // The read ignores the stop flag, as a stuck syscall would.
    let hold = rig.registry.hold_instances();
    let begun = std::time::Instant::now();
    let report = run_with(&rig, &doctor, request.clone()).await;
    assert!(begun.elapsed() < Duration::from_secs(3), "the run returns");
    assert!(hold.entered());
    let secrets = check(&report, DoctorCheckId::Secrets);
    assert_eq!(secrets.severity, DoctorSeverity::Warning);
    assert_eq!(
        secrets.summary,
        SummaryFacts::Unavailable(CheckUnavailableReason::Timeout)
    );
    assert_eq!(
        check(&report, DoctorCheckId::Daemon).severity,
        DoctorSeverity::Ok,
        "the other checks still answer"
    );

    // The task of the first run still waits: the next run reports the
    // timeout at once instead of starting a second task.
    let begun = std::time::Instant::now();
    let report = run_with(&rig, &doctor, request.clone()).await;
    assert!(begun.elapsed() < limit, "no new task, no wait");
    assert_eq!(
        check(&report, DoctorCheckId::Secrets).summary,
        SummaryFacts::Unavailable(CheckUnavailableReason::Timeout)
    );

    hold.release();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let report = run_with(&rig, &doctor, request.clone()).await;
        if check(&report, DoctorCheckId::Secrets).severity == DoctorSeverity::Ok {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the check runs again once its task ended"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// covers: AC-1, AC-8 (spec 0034)
#[tokio::test]
async fn the_stop_flag_of_a_timed_out_registry_check_ends_its_sqlite_call() {
    let rig = Rig::new().await;
    let doctor = Doctor::new(DoctorLimits::default().with_check(Duration::from_millis(300)));
    let request = DoctorRequest::new(None, vec![DoctorCheckId::Registry]);
    rig.registry.inner.hold_health_checks(true);
    let report = run_with(&rig, &doctor, request.clone()).await;
    assert_eq!(
        report.checks[0].summary,
        SummaryFacts::Unavailable(CheckUnavailableReason::Timeout)
    );
    let calls = rig.registry.inner.health_check_calls();
    // The held call returns once the runner set the flag, so a later run
    // starts the check again and makes a new call.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while rig.registry.inner.health_check_calls() == calls {
        assert!(
            std::time::Instant::now() < deadline,
            "the check never ran again"
        );
        let _ = run_with(&rig, &doctor, request.clone()).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    rig.registry.inner.hold_health_checks(false);
}

// covers: AC-1 (spec 0034)
#[tokio::test]
async fn a_scope_limits_the_instance_checks_and_an_unknown_one_is_not_found() {
    let rig = Rig::new().await;
    let one = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Failed,
        last_failure: Some("process_exited"),
        ..stopped_spec()
    });
    let other = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Failed,
        last_failure: Some("process_exited"),
        ..stopped_spec()
    });
    let report = run_with(
        &rig,
        &Doctor::default(),
        DoctorRequest::new(Some(other), Vec::new()),
    )
    .await;
    assert_eq!(report.scope.as_ref().map(|scope| scope.id), Some(other));
    let failures = check(&report, DoctorCheckId::InstanceFailures);
    assert_eq!(failures.findings.len(), 1);
    assert_eq!(
        failures.findings[0]
            .instance
            .as_ref()
            .map(|instance| instance.id),
        Some(other)
    );
    assert_eq!(failures.findings[0].code, DoctorFindingCode::StartFailed);
    assert_eq!(
        report.checks.len(),
        DAEMON_CHECKS.len(),
        "global checks run"
    );
    let _ = one;

    let missing = InstanceId::generate();
    let reading = capacity(&rig, TB, TB / 2);
    let refused = Doctor::default()
        .run(
            sources(&rig),
            DoctorRequest::new(Some(missing), Vec::new()),
            async move { reading },
        )
        .await;
    assert_eq!(refused.err(), Some(DoctorError::InstanceNotFound(missing)));
}

// covers: AC-7, AC-9 (spec 0034)
#[tokio::test]
async fn lost_data_a_lost_executable_and_a_closed_folder_are_reported() {
    let rig = Rig::new().await;
    let lost = rig.instance(&stopped_spec());
    std::fs::remove_file(rig.paths(lost).data.join("FAKE_VERSION")).expect("the marker goes");
    let closed = rig.instance(&stopped_spec());
    let inner = rig.paths(closed).data.join("base");
    std::fs::create_dir(&inner).expect("a data folder");
    std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o500))
        .expect("the folder is closed");
    let before = footprint(&rig);
    let report = run(&rig).await;
    std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o700))
        .expect("the folder opens again");
    assert!(
        check(&report, DoctorCheckId::InstanceFiles)
            .findings
            .iter()
            .any(|finding| finding.code == DoctorFindingCode::DataMissing
                && finding.instance.as_ref().map(|instance| instance.id) == Some(lost))
    );
    let permissions = check(&report, DoctorCheckId::Permissions);
    let finding = permissions
        .findings
        .iter()
        .find(|finding| finding.instance.as_ref().map(|instance| instance.id) == Some(closed))
        .expect("the closed folder");
    assert_eq!(finding.code, DoctorFindingCode::PathNotWritable);
    assert_eq!(finding.subject.as_deref(), Some("data/base"));
    assert_eq!(footprint(&rig), before);

    let stackly_domain::InstallMode::Managed { artifact_sha256 } = rig.runtime.install() else {
        unreachable!("the rig installs a managed runtime");
    };
    let folder = rig.layout.layout().runtime_dir(
        rig.runtime.software(),
        rig.runtime.version(),
        rig.runtime.target(),
        artifact_sha256,
    );
    std::fs::remove_file(folder.join("stackly-fake-server")).expect("the executable goes");
    let report = run(&rig).await;
    assert!(
        codes(&report, DoctorCheckId::InstanceFiles).contains(&DoctorFindingCode::RuntimeMissing)
    );
}

// covers: AC-9 (spec 0034)
#[tokio::test]
async fn a_failed_instance_and_a_low_volume_are_reported_with_their_actions() {
    let rig = Rig::new().await;
    let failed = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Failed,
        last_failure: Some("process_exited"),
        ..stopped_spec()
    });
    let reading = capacity(&rig, TB, 3_000_000_000);
    let report = Doctor::default()
        .run(
            sources(&rig),
            DoctorRequest::default(),
            async move { reading },
        )
        .await
        .expect("the run starts");
    let failures = check(&report, DoctorCheckId::InstanceFailures);
    let finding = &failures.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::StartFailed);
    assert_eq!(finding.action, Some(Action::ShowInstance(failed)));
    let FindingFacts::Failure(snapshot) = &finding.facts else {
        panic!("failure facts");
    };
    assert_eq!(
        snapshot.instance.last_failure().map(|code| code.as_str()),
        Some("process_exited")
    );
    let disk = check(&report, DoctorCheckId::DiskSpace);
    assert_eq!(
        codes(&report, DoctorCheckId::DiskSpace),
        [DoctorFindingCode::VolumeLow]
    );
    assert_eq!(disk.findings[0].action, Some(Action::OpenStorageSettings));
    assert_eq!(report.tally.errors, 1);
    assert_eq!(report.tally.warnings, 1);
}

// covers: AC-1 (spec 0034)
#[tokio::test]
async fn degraded_subsystems_are_warnings_and_a_failed_reading_is_unavailable() {
    let rig = Rig::new().await;
    let mut sources = sources(&rig);
    sources.daemon = DaemonFacts::new(vec![DegradedSubsystem::new(
        "log_sink".to_owned(),
        "degraded".to_owned(),
        Some("a write failed".to_owned()),
    )]);
    let report = Doctor::default()
        .run(
            sources,
            DoctorRequest::new(None, vec![DoctorCheckId::DiskSpace, DoctorCheckId::Daemon]),
            async {
                Err(CapacityFailure::new(
                    CheckUnavailableReason::Io,
                    "statfs failed".to_owned(),
                ))
            },
        )
        .await
        .expect("the run starts");
    let ids: Vec<_> = report.checks.iter().map(|check| check.id).collect();
    assert_eq!(ids, [DoctorCheckId::Daemon, DoctorCheckId::DiskSpace]);
    assert_eq!(
        codes(&report, DoctorCheckId::Daemon),
        [DoctorFindingCode::SubsystemDegraded]
    );
    assert_eq!(
        check(&report, DoctorCheckId::Daemon).findings[0]
            .subject
            .as_deref(),
        Some("log_sink")
    );
    assert_eq!(
        codes(&report, DoctorCheckId::DiskSpace),
        [DoctorFindingCode::CheckUnavailable]
    );
}

// covers: AC-7 (spec 0034)
#[tokio::test]
async fn a_pinned_package_that_does_not_load_is_one_error_with_every_instance_that_pins_it() {
    let rig = Rig::new().await;
    let first = rig.instance(&stopped_spec());
    let second = rig.instance(&stopped_spec());
    let manifest = rig
        .layout
        .layout()
        .provider_revision_dir(rig.key())
        .join("provider.yaml");
    std::fs::write(&manifest, "apiVersion: nope\n").expect("the manifest is broken");
    let report = run(&rig).await;
    let packages = check(&report, DoctorCheckId::ProviderPackages);
    assert_eq!(packages.findings.len(), 1);
    let finding = &packages.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::PackageUnreadable);
    assert_eq!(finding.severity, DoctorSeverity::Error);
    let FindingFacts::Package { key, used_by, .. } = &finding.facts else {
        panic!("package facts");
    };
    assert_eq!(key, rig.key());
    assert_eq!(used_by.len(), 2);
    let named: Vec<_> = [first, second]
        .iter()
        .map(|id| {
            rig.registry
                .instance(*id)
                .expect("the instance reads")
                .value
                .expect("it exists")
                .name()
                .clone()
        })
        .collect();
    assert!(named.iter().all(|name| used_by.contains(name)));
    assert!(matches!(finding.action, Some(Action::ShowInstance(_))));
}

// covers: AC-9 (spec 0034)
#[tokio::test]
async fn a_rendered_file_that_differs_from_its_revision_copy_is_drift_with_review() {
    let rig = Rig::new().await;
    let id = rig.instance(&stopped_spec());
    let paths = rig.paths(id);
    let history = paths.config_history.join(rig.active(id).id.to_string());
    std::fs::create_dir_all(&history).expect("the copy folder");
    std::fs::write(history.join("fake.conf"), "port = 1\n").expect("the copy");
    std::fs::write(paths.config.join("fake.conf"), "port = 2\n").expect("the edited file");
    let before = footprint(&rig);
    let report = run(&rig).await;
    let drift = check(&report, DoctorCheckId::ConfigDrift);
    assert_eq!(drift.severity, DoctorSeverity::Warning);
    let finding = &drift.findings[0];
    assert_eq!(finding.code, DoctorFindingCode::ConfigDrift);
    assert_eq!(finding.action, Some(Action::ReviewConfig(id)));
    let FindingFacts::Drift { files } = &finding.facts else {
        panic!("drift facts");
    };
    assert!(files.iter().any(|file| file == "fake.conf"), "{files:?}");
    assert_eq!(
        std::fs::read_to_string(paths.config.join("fake.conf")).expect("the file reads"),
        "port = 2\n",
        "the doctor keeps the edit"
    );
    assert_eq!(footprint(&rig), before);
}

/// Two remedies for the fake provider: one for readiness, which a failure
/// never matches, then one for an old PID file in the data folder.
const FAKE_REMEDIES: &str = r#"remedies:
  - name: tls_hint
    when: { readinessDetailContains: "tls" }
    title: Readiness needs TLS
    steps:
      - text: Turn TLS off.
  - name: stale_pid
    when:
      failureCodes: [crash_loop, process_exited]
      excerptContains: 'lock file "fake.pid" already exists'
    evidence:
      pidFile: "${instance.data}/fake.pid"
    title: Fake found an old fake.pid
    steps:
      - text: Stackly never removes this file.
      - text: If the PID below is not a Fake server for this data folder, remove the file yourself.
        path: "${instance.data}/fake.pid"
      - text: Then start the instance again.
        command: "${runtime.executable} --data=${instance.data} --port=${endpoint.http.port}"
"#;

/// The excerpt line the stale PID remedy looks for.
const PID_LINE: &str = "FATAL:  lock file \"fake.pid\" already exists";

/// A rig whose pinned fake package carries [`FAKE_REMEDIES`].
async fn remedy_rig() -> Rig {
    Rig::patched(|text| format!("{text}{FAKE_REMEDIES}")).await
}

/// A failed instance with `last_failure`, a fixed port, and `lines` as the
/// output of its last start.
fn failed_with(rig: &Rig, last_failure: Option<&'static str>, lines: &[&str]) -> InstanceId {
    let id = rig.instance(&Spec {
        lifecycle: InstanceLifecycle::Failed,
        last_failure,
        ..stopped_spec()
    });
    let batch = stackly_core::supervisor::LogBatch::new(
        stackly_platform::OutputStream::Stderr,
        rig.fake_now(),
        lines
            .iter()
            .map(|line| stackly_core::supervisor::LogLine::new(*line))
            .collect(),
    );
    stackly_core::supervisor::LogSink::accept(&*rig.deps.tail, id, batch);
    id
}

/// The canonical path of the runtime's main executable.
fn runtime_executable(rig: &Rig) -> std::path::PathBuf {
    let stackly_domain::InstallMode::Managed { artifact_sha256 } = rig.runtime.install() else {
        unreachable!("the rig installs a managed runtime");
    };
    rig.layout
        .layout()
        .runtime_dir(
            rig.runtime.software(),
            rig.runtime.version(),
            rig.runtime.target(),
            artifact_sha256,
        )
        .canonicalize()
        .expect("the runtime folder exists")
        .join("stackly-fake-server")
}

/// Adds a live process with `executable` and `uid`, and returns its PID.
fn live_process(rig: &Rig, pid: u32, executable: &str, uid: stackly_platform::UserId) -> u32 {
    let pid = stackly_domain::Pid::new(pid).expect("a test PID is not 0");
    rig.processes().insert(stackly_platform::ProcessFacts {
        pid,
        parent: stackly_domain::Pid::new(1).ok(),
        group: pid,
        session: pid,
        uid,
        start: stackly_domain::ProcessStart::new(1_760_000_000, 7).expect("a start time"),
        executable: executable.into(),
        stopped: false,
    });
    pid.get()
}

fn start_failed(report: &Report) -> &stackly_core::doctor::Finding {
    check(report, DoctorCheckId::InstanceFailures)
        .findings
        .iter()
        .find(|finding| finding.code == DoctorFindingCode::StartFailed)
        .expect("a start_failed finding")
}

// covers: AC-9, AC-10 (spec 0034)
#[tokio::test]
async fn a_failed_instance_gets_the_first_matching_remedy_rendered_for_it() {
    let rig = remedy_rig().await;
    let id = failed_with(&rig, Some("process_exited"), &["starting", PID_LINE]);
    let report = run(&rig).await;

    let remedy = start_failed(&report)
        .remedy
        .as_deref()
        .expect("the stale PID remedy matches");
    assert_eq!(
        remedy.package,
        *rig.key(),
        "the pinned package without a catalog"
    );
    assert_eq!(remedy.display_name, rig.package.manifest().display_name());
    assert_eq!(remedy.remedy.name, "stale_pid");
    assert_eq!(remedy.remedy.title, "Fake found an old fake.pid");
    let data = rig.layout.instance_paths(id).data;
    let pid_file = data.join("fake.pid");
    let steps: Vec<_> = remedy
        .remedy
        .steps
        .iter()
        .map(|step| (step.text.as_str(), step.command.clone(), step.path.clone()))
        .collect();
    let port = rig
        .registry
        .instance(id)
        .expect("the instance reads")
        .value
        .expect("the instance exists")
        .endpoints()[0]
        .clone();
    let stackly_domain::Endpoint::Ip(port) = port else {
        panic!("an IP endpoint");
    };
    let port = port.effective_port().expect("a fixed port").get();
    let quote = |path: &std::path::Path| {
        let text = path.display().to_string();
        if text.contains(' ') {
            format!("'{text}'")
        } else {
            text
        }
    };
    assert_eq!(
        steps,
        [
            ("Stackly never removes this file.", None, None),
            (
                "If the PID below is not a Fake server for this data folder, remove the file yourself.",
                None,
                Some(pid_file.display().to_string()),
            ),
            (
                "Then start the instance again.",
                Some(format!(
                    "{} --data={} --port={port}",
                    quote(&runtime_executable(&rig)),
                    quote(&data)
                )),
                None,
            ),
        ]
    );
    let read = remedy.pid_file.as_ref().expect("the PID file is read");
    assert_eq!(read.path, pid_file);
    assert_eq!(read.state, stackly_core::doctor::PidFileState::Missing);
    assert_eq!(read.pid, None);
}

// covers: AC-10 (spec 0034)
#[tokio::test]
async fn a_remedy_matches_only_when_every_condition_holds() {
    let rig = remedy_rig().await;
    failed_with(&rig, Some("spawn_failed"), &[PID_LINE]);
    let other_code = run(&rig).await;
    assert!(
        start_failed(&other_code).remedy.is_none(),
        "the code differs"
    );

    let rig = remedy_rig().await;
    failed_with(&rig, Some("process_exited"), &["FATAL:  something else"]);
    let other_line = run(&rig).await;
    assert!(
        start_failed(&other_line).remedy.is_none(),
        "no line matches"
    );

    let rig = remedy_rig().await;
    failed_with(&rig, None, &[PID_LINE]);
    let unknown = run(&rig).await;
    assert!(
        start_failed(&unknown).remedy.is_none(),
        "a failure the daemon no longer knows matches no failureCodes"
    );

    let rig = Rig::new().await;
    failed_with(&rig, Some("process_exited"), &[PID_LINE]);
    let without = run(&rig).await;
    assert!(
        start_failed(&without).remedy.is_none(),
        "a package without remedies has none"
    );
}

// covers: AC-2, AC-10 (spec 0034)
#[tokio::test]
async fn the_pid_file_is_read_passively_and_its_pid_looked_up_without_a_signal() {
    use stackly_core::doctor::PidFileState;

    let rig = remedy_rig().await;
    let id = failed_with(&rig, Some("crash_loop"), &[PID_LINE]);
    let file = rig.layout.instance_paths(id).data.join("fake.pid");
    let uid = rig.processes().user();
    let runtime = runtime_executable(&rig);
    let stackly_platform::UserId::Unix(raw) = uid else {
        unreachable!("the fake runs as a Unix user");
    };
    let other = stackly_platform::UserId::Unix(raw + 1);
    let cases: [(&str, Option<u32>, PidFileState, bool); 5] = [
        ("61001\n/data\n", None, PidFileState::NotRunning, false),
        (
            "",
            Some(live_process(
                &rig,
                61002,
                runtime.to_str().expect("UTF-8"),
                uid.clone(),
            )),
            PidFileState::RuntimeProcess,
            false,
        ),
        (
            "",
            Some(live_process(&rig, 61003, "/bin/sleep", uid.clone())),
            PidFileState::OtherProcess,
            false,
        ),
        (
            "",
            Some(live_process(&rig, 61004, "/usr/sbin/other", other)),
            PidFileState::OtherProcess,
            true,
        ),
        ("not a pid\n", None, PidFileState::Unreadable, false),
    ];
    for (text, pid, state, other_user) in cases {
        let text = pid.map_or_else(|| text.to_owned(), |pid| format!("{pid}\n/data\n"));
        std::fs::write(&file, &text).expect("the PID file is written");
        let before = footprint(&rig);
        let report = run(&rig).await;
        let read = start_failed(&report)
            .remedy
            .as_deref()
            .and_then(|remedy| remedy.pid_file.clone())
            .expect("the PID file is read");
        assert_eq!(
            (read.state, read.other_user),
            (state, other_user),
            "{text:?}"
        );
        let expected_pid = text
            .lines()
            .next()
            .and_then(|line| line.parse::<u32>().ok());
        assert_eq!(read.pid.map(stackly_domain::Pid::get), expected_pid);
        assert_eq!(read.process.is_some(), pid.is_some(), "{text:?}");
        assert_eq!(footprint(&rig), before, "a read writes and signals nothing");
        assert_eq!(
            std::fs::read_to_string(&file).expect("the PID file stays"),
            text,
            "the PID file is never changed"
        );
        if let Some(pid) = pid {
            let pid = stackly_domain::Pid::new(pid).expect("a PID");
            assert!(rig.processes().signals(pid).is_empty());
        }
    }
}

// covers: AC-10 (spec 0034)
#[tokio::test]
async fn a_pid_file_that_is_a_symlink_or_outside_the_instance_is_unreadable() {
    use stackly_core::doctor::PidFileState;

    let rig = remedy_rig().await;
    let id = failed_with(&rig, Some("process_exited"), &[PID_LINE]);
    let data = rig.layout.instance_paths(id).data;
    let elsewhere = rig.root.path().join("elsewhere.pid");
    std::fs::write(&elsewhere, "61010\n").expect("a file outside the instance");
    std::os::unix::fs::symlink(&elsewhere, data.join("fake.pid")).expect("the symlink is made");

    let report = run(&rig).await;
    let read = start_failed(&report)
        .remedy
        .as_deref()
        .and_then(|remedy| remedy.pid_file.clone())
        .expect("the PID file is looked at");
    assert_eq!(read.state, PidFileState::Unreadable);
    assert_eq!(read.pid, None, "the target of a symlink is never read");
}

// covers: AC-10 (spec 0034)
#[tokio::test]
async fn the_catalog_revision_of_the_provider_comes_before_the_pinned_package() {
    use stackly_core::catalog::{Catalog, CatalogEntry, ProviderEntry, TileColor, TileHint};
    use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};

    let rig = Rig::new().await;
    failed_with(&rig, Some("process_exited"), &[PID_LINE]);
    let mut files = std::collections::BTreeMap::new();
    for (path, bytes) in rig.package.files().iter() {
        files.insert(path.to_owned(), bytes.to_vec());
    }
    let manifest = files.get_mut("provider.yaml").expect("a manifest");
    let newer = String::from_utf8(manifest.clone())
        .expect("a UTF-8 manifest")
        .replacen("revision: ", "revision: \"9.9.9\"\n  # was: ", 1);
    *manifest = format!("{newer}{FAKE_REMEDIES}").into_bytes();
    let package = ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::Skip)
        .expect("the newer revision is valid");
    assert_ne!(package.key(), rig.key());
    let catalog = Catalog::for_tests(&[CatalogEntry::Provider(ProviderEntry {
        package: Arc::new(package.clone()),
        tile: TileHint {
            mark: "Fk".to_owned(),
            tile: TileColor::Blue,
        },
        notes: Vec::new(),
        origin: stackly_core::catalog::EntryOrigin::Catalog,
    })]);

    let reading = capacity(&rig, TB, TB / 2);
    let report = Doctor::default()
        .run(
            sources(&rig).with_catalog(Some(Arc::new(catalog))),
            DoctorRequest::default(),
            async move { reading },
        )
        .await
        .expect("the run starts");
    let remedy = start_failed(&report)
        .remedy
        .as_deref()
        .expect("the catalog revision has the remedy the pinned one lacks");
    assert_eq!(remedy.package, *package.key());
    assert_eq!(remedy.remedy.name, "stale_pid");
}

// covers: AC-10 (spec 0034); AC-4, AC-11 (spec 0037, review Major 1): a
// local entry that replaced the revision an instance pins never supplies the
// remedy, whatever its approval, so bytes nobody approved never become
// advice in Stackly's voice
#[tokio::test]
async fn a_local_entry_of_another_revision_never_supplies_the_remedy_of_a_pinned_instance() {
    use stackly_core::catalog::local::{package_bytes, package_sha256, permissions};
    use stackly_core::catalog::{
        Catalog, CatalogEntry, EntryOrigin, LocalInfo, ProviderEntry, TileColor, TileHint,
    };
    use stackly_core::provider::{NameCheck, PackageFiles, ValidatedPackage};
    use stackly_domain::{ImportDecision, ProviderImportRecord, TrustDecision};

    let rig = remedy_rig().await;
    failed_with(&rig, Some("process_exited"), &[PID_LINE]);
    let mut files = std::collections::BTreeMap::new();
    for (path, bytes) in rig.package.files().iter() {
        files.insert(path.to_owned(), bytes.to_vec());
    }
    let manifest = files.get_mut("provider.yaml").expect("a manifest");
    let replaced = String::from_utf8(manifest.clone())
        .expect("a UTF-8 manifest")
        .replacen("revision: ", "revision: \"9.9.9\"\n  # was: ", 1)
        .replace(
            "title: Fake found an old fake.pid",
            "title: Run this command nobody approved",
        );
    *manifest = replaced.into_bytes();
    let package = ValidatedPackage::from_files(PackageFiles::new("fake", files), NameCheck::Skip)
        .expect("the replacing revision is valid");
    assert_ne!(package.key(), rig.key());

    let decisions = [
        None,
        Some(TrustDecision::Revoked),
        Some(TrustDecision::Approved),
    ];
    for decision in decisions {
        let key = package.key();
        let mut record = ProviderImportRecord::new(
            key.provider().clone(),
            "0199f000-0000-7000-8000-000000000010"
                .parse()
                .expect("an id"),
            key.revision().clone(),
            package.software().clone(),
            *key.manifest_sha256(),
            *key.manifest_sha256(),
            package_sha256(package.files()),
            package_bytes(package.files()),
            "~/acme".to_owned(),
            jiff::Timestamp::UNIX_EPOCH,
        );
        if let Some(decision) = decision {
            let (digest, revision) = (record.package_sha256, record.provider_revision.clone());
            record = record.with_decision(ImportDecision::new(
                decision,
                digest,
                revision,
                "{}".to_owned(),
                jiff::Timestamp::UNIX_EPOCH,
            ));
        }
        let entry = CatalogEntry::Provider(ProviderEntry {
            package: Arc::new(package.clone()),
            tile: TileHint {
                mark: "Fk".to_owned(),
                tile: TileColor::Gray,
            },
            notes: Vec::new(),
            origin: EntryOrigin::Local(Box::new(LocalInfo::new(record, permissions(&package)))),
        });
        let catalog = Catalog::for_tests(&[]).with_local(vec![entry]);

        let reading = capacity(&rig, TB, TB / 2);
        let report = Doctor::default()
            .run(
                sources(&rig).with_catalog(Some(Arc::new(catalog))),
                DoctorRequest::default(),
                async move { reading },
            )
            .await
            .expect("the run starts");

        let remedy = start_failed(&report)
            .remedy
            .as_deref()
            .expect("the pinned package has the remedy");
        assert_eq!(remedy.package, *rig.key(), "{decision:?}");
        assert_eq!(
            remedy.remedy.title, "Fake found an old fake.pid",
            "{decision:?}"
        );
    }
}
