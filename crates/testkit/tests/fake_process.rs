//! Behavior of `FakeProcessSupervision`, the process port of
//! `FakePlatform` (spec 0010 build step 4): suspended spawns, the identity
//! check before every signal, exit notices, real locks and spools in the
//! test root, the call log, and injected failures.
#![cfg(unix)]

use std::cell::Cell;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use stackly_domain::{BootId, LockState, Pid, ProcessIdentity, ProcessStart};
use stackly_platform::{
    CapturedOutput, ChildRole, ExitListener, ExitNotice, ExitStatus, HoldsFile, InstanceLock,
    LockOutcome, OutputStream, OutputTarget, ProcessError, ProcessFacts, ProcessSupervision,
    ProviderSignal, RequestPart, SecretValue, SignalTarget, SpawnRequest, SpoolWriter, StdinSource,
    SuspendedChild, UserId,
};
use stackly_testkit::{
    FakePlatform, FakeProcessState, FakeProcessSupervision, ProcessOp, TestRoot,
};

/// Collects every exit notice it hears.
#[derive(Default)]
struct Notices(Mutex<Vec<ExitNotice>>);

impl Notices {
    fn taken(&self) -> Vec<ExitNotice> {
        std::mem::take(
            &mut *self
                .0
                .lock()
                .expect("no test thread panics while holding the notices"),
        )
    }
}

impl ExitListener for Notices {
    fn exited(&self, notice: ExitNotice) {
        self.0
            .lock()
            .expect("no test thread panics while holding the notices")
            .push(notice);
    }
}

struct Setup {
    root: TestRoot,
    fake: FakeProcessSupervision,
    spool_sets: Cell<usize>,
}

impl Setup {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root can be created");
        let fake = FakeProcessSupervision::new(&root);
        Self {
            root,
            fake,
            spool_sets: Cell::new(0),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn lock(&self) -> InstanceLock {
        match self
            .fake
            .lock_instance(&self.path("instance.lock"))
            .expect("the lock file in the test root can be opened")
        {
            LockOutcome::Acquired(lock) => lock,
            LockOutcome::Held => panic!("the instance lock is free"),
        }
    }

    /// Creates `stdout.spool` and `stderr.spool` in the run folder the
    /// first time, then a numbered pair each time after.
    fn spools(&self) -> (SpoolWriter, SpoolWriter) {
        let set = self.spool_sets.replace(self.spool_sets.get() + 1);
        let name = |stream: &str| match set {
            0 => format!("{stream}.spool"),
            n => format!("{stream}-{n}.spool"),
        };
        let run = self.root.run_dir();
        (
            self.fake
                .create_spool(&run.join(name("stdout")))
                .expect("a new stdout spool can be created in the run folder"),
            self.fake
                .create_spool(&run.join(name("stderr")))
                .expect("a new stderr spool can be created in the run folder"),
        )
    }

    /// Spawns a main process with the lock and both spools.
    fn spawn_main(&self, lock: &InstanceLock) -> SuspendedChild {
        let (stdout, stderr) = self.spools();
        self.fake
            .spawn_main(SpawnRequest {
                program: Path::new("/runtimes/fake/bin/stackly-fake-server"),
                args: &["serve".to_owned(), "$(touch pwned) ü".to_owned()],
                env: &[("FAKE_LABEL".to_owned(), "a b".to_owned())],
                cwd: self.root.path(),
                stdin: StdinSource::Null,
                output: OutputTarget::Spool { stdout, stderr },
                lock: Some(lock),
                role: ChildRole::Main,
            })
            .expect("a valid main request spawns")
    }

    fn spawn_helper(&self, role: ChildRole, stdin: StdinSource) -> Pid {
        self.fake
            .spawn_helper(SpawnRequest {
                program: Path::new("/runtimes/fake/bin/stackly-fake-server"),
                args: &["init".to_owned()],
                env: &[],
                cwd: self.root.path(),
                stdin,
                output: OutputTarget::Capture,
                lock: None,
                role,
            })
            .expect("a valid helper request spawns")
            .pid()
    }
}

fn identity_of(facts: &ProcessFacts, boot: BootId) -> ProcessIdentity {
    ProcessIdentity {
        pid: facts.pid,
        start: facts.start,
        executable: facts.executable.clone(),
        session: facts.session,
        boot,
    }
}

fn pid(number: u32) -> Pid {
    Pid::new(number).expect("a test PID is from 1 to i32::MAX")
}

#[test]
fn a_main_process_starts_suspended_with_the_exact_request_and_runs_once_resumed() {
    let setup = Setup::new();
    let lock = setup.lock();

    let child = setup.spawn_main(&lock);
    let pid = child.pid();
    let facts = child.facts().clone();

    assert!(facts.stopped);
    assert_eq!((facts.group, facts.session), (pid, pid));
    assert_eq!(facts.uid, setup.fake.user());
    assert_eq!(
        setup.fake.process_state(pid),
        Some(FakeProcessState::Suspended)
    );
    let spawn = &setup.fake.spawns()[0];
    assert_eq!(spawn.args, ["serve", "$(touch pwned) ü"]);
    assert_eq!(spawn.env, [("FAKE_LABEL".to_owned(), "a b".to_owned())]);
    assert_eq!(spawn.lock, Some(lock.identity()));
    assert_eq!(spawn.stdin_len, None);

    let handle = setup.fake.resume(child).unwrap();
    assert_eq!(handle.pid(), pid);
    let running = setup.fake.inspect(pid).unwrap().unwrap();
    assert!(!running.stopped);
    assert_eq!(running.start, facts.start);
    assert_eq!(
        setup.fake.process_state(pid),
        Some(FakeProcessState::Running)
    );
}

#[test]
fn a_child_keeps_the_inherited_lock_until_it_exits() {
    let setup = Setup::new();
    let path = setup.path("instance.lock");
    let lock = setup.lock();
    let child = setup.spawn_main(&lock);
    let pid = child.pid();
    setup.fake.resume(child).unwrap();

    drop(lock);
    assert_eq!(setup.fake.probe_lock(&path).unwrap(), LockState::Held);
    assert!(matches!(
        setup.fake.lock_instance(&path).unwrap(),
        LockOutcome::Held
    ));

    setup.fake.exit(pid, Some(ExitStatus::Exited(0)));
    assert_eq!(setup.fake.probe_lock(&path).unwrap(), LockState::Free);
    assert_eq!(setup.fake.inspect(pid).unwrap(), None);
}

#[test]
fn only_a_suspended_main_process_can_be_discarded() {
    let setup = Setup::new();
    let lock = setup.lock();
    let discarded = setup.spawn_main(&lock);
    let discarded_pid = discarded.pid();
    let notices = Arc::new(Notices::default());
    setup
        .fake
        .watch_exit(discarded_pid, notices.clone())
        .unwrap();

    assert_eq!(setup.fake.discard(discarded).unwrap(), None);
    assert_eq!(
        notices.taken(),
        [ExitNotice {
            pid: discarded_pid,
            status: Some(ExitStatus::Signaled(9)),
            captured: None,
        }]
    );

    let running = setup.spawn_main(&lock);
    let facts = running.facts().clone();
    setup.fake.resume(running).unwrap();
    let forged = SuspendedChild::new(facts.clone());
    assert_eq!(
        setup.fake.discard(forged),
        Err(ProcessError::NotFound { pid: facts.pid })
    );
    assert_eq!(
        setup.fake.process_state(facts.pid),
        Some(FakeProcessState::Running)
    );
}

#[test]
fn a_suspended_main_process_that_died_on_its_own_is_discarded_with_its_status() {
    let setup = Setup::new();
    let lock = setup.lock();
    let child = setup.spawn_main(&lock);
    let facts = child.facts().clone();

    setup.fake.exit(facts.pid, Some(ExitStatus::Signaled(9)));
    assert_eq!(setup.fake.discard(child), Ok(Some(ExitStatus::Signaled(9))));
    assert!(setup.fake.signals(facts.pid).is_empty());
    assert_eq!(
        setup.fake.discard(SuspendedChild::new(facts.clone())),
        Err(ProcessError::NotFound { pid: facts.pid })
    );

    let notices = Arc::new(Notices::default());
    setup.fake.watch_exit(facts.pid, notices.clone()).unwrap();
    assert_eq!(
        notices.taken(),
        [ExitNotice {
            pid: facts.pid,
            status: None,
            captured: None,
        }],
        "the discard reaped the child, so a later watch knows nothing of it"
    );
}

#[test]
fn a_signal_reaches_only_the_identity_it_targets() {
    let setup = Setup::new();
    let lock = setup.lock();
    let child = setup.spawn_main(&lock);
    let facts = child.facts().clone();
    setup.fake.resume(child).unwrap();
    let boot = setup.fake.boot_id().unwrap();
    let target = SignalTarget::new(identity_of(&facts, boot));

    setup.fake.signal(&target, ProviderSignal::Term).unwrap();
    assert_eq!(setup.fake.signals(facts.pid), [ProviderSignal::Term]);

    setup.fake.update_facts(facts.pid, |facts| {
        facts.start = ProcessStart::new(facts.start.seconds() + 1, 0).unwrap();
    });
    assert_eq!(
        setup.fake.signal(&target, ProviderSignal::Int),
        Err(ProcessError::IdentityMismatch { pid: facts.pid })
    );
    setup.fake.update_facts(facts.pid, |now| {
        now.start = facts.start;
        now.uid = UserId::Unix(0);
    });
    assert_eq!(
        setup.fake.signal(&target, ProviderSignal::Hup),
        Err(ProcessError::IdentityMismatch { pid: facts.pid })
    );
    assert_eq!(setup.fake.signals(facts.pid), [ProviderSignal::Term]);

    setup.fake.exit(facts.pid, None);
    assert_eq!(
        setup.fake.signal(&target, ProviderSignal::Term),
        Err(ProcessError::NotFound { pid: facts.pid })
    );
}

#[test]
fn continue_stopped_resumes_a_suspended_process_after_the_same_check() {
    let setup = Setup::new();
    let boot = setup.fake.boot_id().unwrap();
    let facts = ProcessFacts {
        pid: pid(4242),
        parent: None,
        group: pid(4242),
        session: pid(4242),
        uid: setup.fake.user(),
        start: ProcessStart::new(1_700_000_000, 0).unwrap(),
        executable: PathBuf::from("/runtimes/fake/bin/stackly-fake-server"),
        stopped: true,
    };
    setup.fake.insert(facts.clone());
    let mut wrong = identity_of(&facts, boot);
    wrong.session = pid(1);

    assert_eq!(
        setup.fake.continue_stopped(&SignalTarget::new(wrong)),
        Err(ProcessError::IdentityMismatch { pid: facts.pid })
    );
    assert!(setup.fake.inspect(facts.pid).unwrap().unwrap().stopped);
    setup
        .fake
        .continue_stopped(&SignalTarget::new(identity_of(&facts, boot)))
        .unwrap();
    assert!(!setup.fake.inspect(facts.pid).unwrap().unwrap().stopped);
    assert!(setup.fake.signals(facts.pid).is_empty());
}

#[test]
fn an_exit_reaches_the_watcher_once_or_the_next_watch_at_once() {
    let setup = Setup::new();
    let notices = Arc::new(Notices::default());
    let watched = setup.spawn_helper(ChildRole::Probe, StdinSource::Null);
    let unwatched = setup.spawn_helper(ChildRole::StopCommand, StdinSource::Null);
    let output = CapturedOutput {
        stdout: b"ok\n".to_vec(),
        stderr: Vec::new(),
    };

    setup.fake.watch_exit(watched, notices.clone()).unwrap();
    setup
        .fake
        .exit_with_output(watched, Some(ExitStatus::Exited(0)), output.clone());
    setup.fake.exit(watched, Some(ExitStatus::Exited(1)));
    setup.fake.exit(unwatched, Some(ExitStatus::Signaled(15)));
    assert_eq!(notices.taken().len(), 1);

    setup.fake.watch_exit(unwatched, notices.clone()).unwrap();
    setup.fake.watch_exit(pid(4242), notices.clone()).unwrap();
    let heard: Vec<_> = notices
        .taken()
        .into_iter()
        .map(|notice| (notice.pid, notice.status, notice.captured))
        .collect();
    assert_eq!(
        heard,
        [
            (unwatched, Some(ExitStatus::Signaled(15)), None),
            (pid(4242), None, None),
        ]
    );
}

#[test]
fn holds_file_answers_from_the_files_a_process_holds_and_its_user() {
    let setup = Setup::new();
    let lock = setup.lock();
    let identity = lock.identity();
    let child = setup.spawn_main(&lock);
    let main = child.pid();
    let other = setup.spawn_helper(ChildRole::Probe, StdinSource::Null);

    assert_eq!(setup.fake.holds_file(main, identity), Ok(HoldsFile::Yes));
    assert_eq!(setup.fake.holds_file(other, identity), Ok(HoldsFile::No));
    setup.fake.set_holds(other, HoldsFile::Unreadable);
    assert_eq!(
        setup.fake.holds_file(other, identity),
        Ok(HoldsFile::Unreadable)
    );
    setup
        .fake
        .update_facts(main, |facts| facts.uid = UserId::Unix(0));
    assert_eq!(
        setup.fake.holds_file(main, identity),
        Ok(HoldsFile::Unreadable)
    );
    setup.fake.exit(other, None);
    assert_eq!(
        setup.fake.holds_file(other, identity),
        Err(ProcessError::NotFound { pid: other })
    );
}

#[test]
fn a_handed_lock_stays_held_by_the_process_that_took_it() {
    let setup = Setup::new();
    let path = setup.path("instance.lock");
    let holder = pid(4243);
    setup.fake.insert(ProcessFacts {
        pid: holder,
        parent: None,
        group: holder,
        session: holder,
        uid: setup.fake.user(),
        start: ProcessStart::new(1_700_000_000, 1).unwrap(),
        executable: PathBuf::from("/bin/sleep"),
        stopped: false,
    });
    let lock = setup.lock();
    let identity = lock.identity();

    setup.fake.hand_lock(holder, lock);
    assert_eq!(setup.fake.probe_lock(&path).unwrap(), LockState::Held);
    assert_eq!(setup.fake.holds_file(holder, identity), Ok(HoldsFile::Yes));
    setup.fake.exit(holder, None);
    assert_eq!(setup.fake.probe_lock(&path).unwrap(), LockState::Free);
}

#[test]
fn group_members_list_the_live_processes_left_in_a_group() {
    let setup = Setup::new();
    let lock = setup.lock();
    let child = setup.spawn_main(&lock);
    let main = child.pid();
    setup.fake.resume(child).unwrap();
    let leftover = pid(main.get() + 100);
    setup.fake.insert(ProcessFacts {
        pid: leftover,
        parent: Some(main),
        group: main,
        session: main,
        uid: setup.fake.user(),
        start: ProcessStart::new(1_800_000_000, 0).unwrap(),
        executable: PathBuf::from("/runtimes/fake/bin/stackly-fake-server"),
        stopped: false,
    });

    assert_eq!(setup.fake.group_members(main), Ok(vec![main, leftover]));
    setup.fake.exit(main, Some(ExitStatus::Exited(0)));
    assert_eq!(setup.fake.group_members(main), Ok(vec![leftover]));
}

#[test]
fn helpers_take_a_secret_on_stdin_and_only_their_group_is_terminated() {
    let setup = Setup::new();
    let secret = SecretValue::new("not-a-real-secret-value".to_owned()).unwrap();
    let helper = setup.spawn_helper(ChildRole::InitStep, StdinSource::Bytes(secret));
    let lock = setup.lock();
    let main = setup.spawn_main(&lock);

    assert_eq!(setup.fake.spawns()[0].stdin_len, Some(23));
    let taken = setup.fake.take_stdin(helper).unwrap();
    assert_eq!(taken.len(), 23);
    assert!(setup.fake.take_stdin(helper).is_none());
    let handle = stackly_platform::HelperHandle::new(helper, ChildRole::InitStep);
    setup.fake.terminate_helper_group(&handle, false).unwrap();
    setup.fake.terminate_helper_group(&handle, true).unwrap();
    let forged = stackly_platform::HelperHandle::new(main.pid(), ChildRole::Probe);
    assert_eq!(
        setup.fake.terminate_helper_group(&forged, true),
        Err(ProcessError::NotFound { pid: main.pid() })
    );
    let forces: Vec<_> = setup
        .fake
        .calls()
        .into_iter()
        .filter(|call| call.op == ProcessOp::TerminateHelperGroup)
        .map(|call| (call.pid, call.force, call.error.is_none()))
        .collect();
    assert_eq!(
        forces,
        [
            (Some(helper), Some(false), true),
            (Some(helper), Some(true), true),
            (Some(main.pid()), Some(true), false),
        ]
    );
}

#[test]
fn invalid_requests_spawn_nothing() {
    let setup = Setup::new();
    let result = setup.fake.spawn_helper(SpawnRequest {
        program: Path::new("/bin/true"),
        args: &["nul\0inside".to_owned()],
        env: &[],
        cwd: setup.root.path(),
        stdin: StdinSource::Null,
        output: OutputTarget::Capture,
        lock: None,
        role: ChildRole::Probe,
    });

    assert_eq!(
        result.unwrap_err(),
        ProcessError::InvalidArgument {
            position: RequestPart::Arg(0)
        }
    );
    assert!(setup.fake.spawns().is_empty());
    let call = &setup.fake.calls()[0];
    assert_eq!((call.op, call.pid), (ProcessOp::SpawnHelper, None));
}

#[test]
fn locks_refuse_symlinks_and_paths_outside_the_test_roots() {
    let setup = Setup::new();
    let target = setup.path("elsewhere.lock");
    std::fs::write(&target, b"").unwrap();
    let link = setup.path("instance.lock");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert_eq!(
        setup.fake.probe_lock(&link),
        Err(ProcessError::LockFileInvalid {
            path: link.clone(),
            reason: "is a symlink"
        })
    );
    assert!(matches!(
        setup.fake.lock_instance(&link),
        Err(ProcessError::LockFileInvalid {
            reason: "is a symlink",
            ..
        })
    ));
    assert_eq!(
        setup.fake.probe_lock(&setup.path("missing.lock")),
        Ok(LockState::Free)
    );
    let outside = TestRoot::new().unwrap();
    assert!(matches!(
        setup.fake.probe_lock(&outside.path().join("instance.lock")),
        Err(ProcessError::Io { op: "confine", .. })
    ));
    assert!(matches!(
        setup
            .fake
            .create_spool(&setup.root.path().join("..").join("x.spool")),
        Err(ProcessError::Io { op: "confine", .. })
    ));
    assert_eq!(setup.fake.violations(), 2);
}

#[test]
fn spools_read_what_the_process_wrote_and_free_whole_units_only() {
    let setup = Setup::new();
    setup.fake.set_punch_unit(NonZeroU64::new(8).unwrap());
    let lock = setup.lock();
    let child = setup.spawn_main(&lock);
    let main = child.pid();
    setup.fake.resume(child).unwrap();
    let path = setup.root.run_dir().join("stdout.spool");

    assert!(matches!(
        setup.fake.create_spool(&path),
        Err(ProcessError::Io {
            op: "create_spool",
            ..
        })
    ));
    setup
        .fake
        .write_output(main, OutputStream::Stdout, b"first line\nsecond\n")
        .unwrap();
    let mut reader = setup.fake.open_spool(&path).unwrap().unwrap();
    assert_eq!(reader.len(), Ok(18));
    assert_eq!(reader.first_data_offset(), Ok(Some(0)));
    assert_eq!(reader.punch_unit().get(), 8);

    reader.release_below(11).unwrap();
    assert_eq!(reader.first_data_offset(), Ok(Some(8)));
    let mut buf = [0xff; 18];
    assert_eq!(reader.read_at(0, &mut buf), Ok(18));
    assert_eq!(&buf, b"\0\0\0\0\0\0\0\0ne\nsecond\n");
    reader.release_below(500).unwrap();
    assert_eq!(reader.first_data_offset(), Ok(Some(16)));
    drop(reader);

    let reopened = setup.fake.open_spool(&path).unwrap().unwrap();
    assert_eq!(reopened.first_data_offset(), Ok(Some(16)));
    assert!(
        setup
            .fake
            .open_spool(&setup.root.run_dir().join("none.spool"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn injected_failures_are_logged_and_leave_no_effect() {
    let setup = Setup::new();
    let pid = pid(4242);
    let broken = ProcessError::Io {
        op: "proc_pidinfo",
        path: None,
        kind: std::io::ErrorKind::Other,
        message: "injected".to_owned(),
    };
    setup.fake.fail_next(ProcessOp::Inspect, broken.clone());
    setup.fake.fail_always(ProcessOp::BootId, broken.clone());
    setup.fake.fail_next(
        ProcessOp::SpawnMain,
        ProcessError::SpawnFailed {
            kind: std::io::ErrorKind::PermissionDenied,
            errno: 13,
        },
    );
    let lock = setup.lock();
    let (stdout, stderr) = setup.spools();

    assert_eq!(setup.fake.inspect(pid), Err(broken.clone()));
    assert_eq!(setup.fake.inspect(pid), Ok(None));
    assert_eq!(setup.fake.boot_id(), Err(broken.clone()));
    assert_eq!(setup.fake.boot_id(), Err(broken.clone()));
    let spawned = setup.fake.spawn_main(SpawnRequest {
        program: Path::new("/bin/true"),
        args: &[],
        env: &[],
        cwd: setup.root.path(),
        stdin: StdinSource::Null,
        output: OutputTarget::Spool { stdout, stderr },
        lock: Some(&lock),
        role: ChildRole::Main,
    });
    assert!(matches!(
        spawned,
        Err(ProcessError::SpawnFailed { errno: 13, .. })
    ));
    assert!(setup.fake.spawns().is_empty());
    setup.fake.clear_faults();
    assert!(setup.fake.boot_id().is_ok());

    let ops: Vec<_> = setup
        .fake
        .calls()
        .into_iter()
        .map(|call| (call.op, call.error.is_some()))
        .collect();
    assert_eq!(
        ops,
        [
            (ProcessOp::LockInstance, false),
            (ProcessOp::CreateSpool, false),
            (ProcessOp::CreateSpool, false),
            (ProcessOp::Inspect, true),
            (ProcessOp::Inspect, false),
            (ProcessOp::BootId, true),
            (ProcessOp::BootId, true),
            (ProcessOp::SpawnMain, true),
            (ProcessOp::BootId, false),
        ]
    );
    assert_eq!(setup.fake.count(ProcessOp::BootId), 3);
}

#[test]
fn a_new_boot_id_is_what_the_next_call_reads() {
    let setup = Setup::new();
    let before = setup.fake.boot_id().unwrap();
    let after: BootId = "6f9619ff-8b86-d011-b42d-00c04fc964ff".parse().unwrap();

    setup.fake.set_boot_id(after);
    assert_ne!(before, after);
    assert_eq!(setup.fake.boot_id(), Ok(after));
}

#[test]
fn fake_platform_installs_the_fake_process_port_and_shares_it() {
    let root = TestRoot::new().unwrap();
    let fake = FakePlatform::new(&root);
    let platform = fake.platform();
    let path = root.path().join("instance.lock");

    let LockOutcome::Acquired(lock) = platform.processes().lock_instance(&path).unwrap() else {
        panic!("a new lock file is free");
    };
    assert_eq!(fake.processes().probe_lock(&path), Ok(LockState::Held));
    drop(lock);
    assert_eq!(fake.processes().count(ProcessOp::LockInstance), 1);
    assert_eq!(fake.processes().count(ProcessOp::ProbeLock), 1);
}
