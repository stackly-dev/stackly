//! `MacosProcessSupervision` against real processes (spec 0010 build step
//! 3): `stackly-fake-server` as the main process, system tools as helpers,
//! and PID 1 as a process of another user. Every test has its own
//! `TestRoot` and ports, and a guard that kills and reaps whatever child is
//! left, through the adapter's own watcher.
#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use stackly_domain::{LockState, Pid, ProcessIdentity, ProcessStart};
use stackly_platform::{
    CapabilityStatus, ChildRole, ExitListener, ExitNotice, ExitStatus, HELPER_CAPTURE_MAX,
    HoldsFile, InstanceLock, LockOutcome, OutputTarget, PlatformCapability, ProcessError,
    ProcessFacts, ProcessSupervision, ProviderSignal, RequestPart, SecretValue, SignalTarget,
    SpawnRequest, StdinSource, SuspendedChild, UserId, UserIdentity,
};
use stackly_platform_macos::{MacosIdentity, MacosPlatform, MacosProcessSupervision};
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stackly_testkit::{TestRoot, allocate_port, fake_server};

/// How long a test waits for something a process does. Generous on
/// purpose: on a loaded Mac the first run of a freshly linked binary waits
/// for its code signature check, and a passing run never comes close.
const PATIENCE: Duration = Duration::from_secs(30);
/// How long a test watches for something that must not happen.
const QUIET: Duration = Duration::from_millis(400);
/// An argument full of shell syntax: any shell that reads it creates a
/// `pwned` file, which the AC-1 test looks for.
const LABEL: &str = "$(touch pwned); touch pwned2 `touch pwned3` \"q\" 'q' | & > ü 空 * ~";
/// `SIGHUP` and `SIGPIPE` as bits of a `sigset_t`.
const HUP_BIT: u32 = 1;
const PIPE_BIT: u32 = 1 << (13 - 1);

/// Collects exit notices and lets a test wait for them.
#[derive(Default)]
struct Notices {
    seen: Mutex<Vec<ExitNotice>>,
    arrived: Condvar,
}

impl Notices {
    fn wait(&self) -> ExitNotice {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = self.seen.lock().expect("no listener panics");
        while seen.is_empty() {
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("an exit notice arrives in time");
            seen = self
                .arrived
                .wait_timeout(seen, left)
                .expect("no listener panics")
                .0;
        }
        seen.remove(0)
    }

    fn count(&self) -> usize {
        self.seen.lock().expect("no listener panics").len()
    }
}

impl ExitListener for Notices {
    fn exited(&self, notice: ExitNotice) {
        self.seen.lock().expect("no listener panics").push(notice);
        self.arrived.notify_all();
    }
}

/// A test root with a folder whose name has spaces and Unicode, the
/// adapter, and the children to clean up.
struct Setup {
    root: TestRoot,
    dir: PathBuf,
    adapter: MacosProcessSupervision,
    children: Mutex<Vec<ProcessFacts>>,
    spools: Mutex<usize>,
    /// The spools of each main process, for failure messages.
    spools_of: Mutex<Vec<(Pid, [PathBuf; 2])>>,
}

impl Setup {
    fn new() -> Self {
        let root = TestRoot::new().expect("a test root");
        let dir = root.path().join("Test Root Ünï cödé");
        fs::create_dir(&dir).expect("the test root takes a folder");
        Self {
            root,
            dir,
            adapter: MacosProcessSupervision::new(),
            children: Mutex::new(Vec::new()),
            spools: Mutex::new(0),
            spools_of: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> InstanceLock {
        match self
            .adapter
            .lock_instance(&self.dir.join("instance.lock"))
            .expect("the lock file can be created")
        {
            LockOutcome::Acquired(lock) => lock,
            LockOutcome::Held => panic!("a new instance lock is free"),
        }
    }

    /// A data folder with the config `serve` reads, as the working
    /// directory of a fake server.
    fn data_dir(&self) -> PathBuf {
        let data = self.dir.join("data");
        fs::create_dir_all(&data).expect("the data folder can be created");
        let token = FixtureCredential::new();
        let config = serde_json::json!({
            "greeting": "hello from the fake",
            "instanceName": "fake one",
            "authToken": token.value(),
        });
        fs::write(data.join("config.json"), config.to_string()).expect("the config is written");
        data
    }

    /// A new pair of spools in the run folder.
    fn spool_paths(&self) -> [PathBuf; 2] {
        let mut count = self.spools.lock().expect("no test thread panics");
        *count += 1;
        ["stdout", "stderr"].map(|stream| {
            self.root
                .run_dir()
                .join(format!("{stream}-{}.spool", *count))
        })
    }

    fn spool_output(&self, paths: &[PathBuf; 2]) -> OutputTarget {
        let [stdout, stderr] = paths.clone().map(|path| {
            self.adapter
                .create_spool(&path)
                .expect("a new spool can be created")
        });
        OutputTarget::Spool { stdout, stderr }
    }

    fn spawn_main(
        &self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        cwd: &Path,
        lock: Option<&InstanceLock>,
    ) -> SuspendedChild {
        let paths = self.spool_paths();
        let child = self
            .adapter
            .spawn_main(SpawnRequest {
                program,
                args,
                env,
                cwd,
                stdin: StdinSource::Null,
                output: self.spool_output(&paths),
                lock,
                role: ChildRole::Main,
            })
            .expect("the main process spawns");
        self.track(child.facts().clone());
        self.spools_of
            .lock()
            .expect("no test thread panics")
            .push((child.pid(), paths));
        child
    }

    /// Waits until a resumed fake server wrote its argv record, and says
    /// what the process did when it never does.
    fn wait_for_argv(&self, pid: Pid, data: &Path) -> Vec<String> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(argv) = argv_record(data) {
                return argv;
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let spools = self.spools_of.lock().expect("no test thread panics");
        let output: Vec<String> = spools
            .iter()
            .filter(|(owner, _)| *owner == pid)
            .flat_map(|(_, paths)| paths.iter())
            .map(|path| {
                format!(
                    "{}: {:?}",
                    path.display(),
                    fs::read_to_string(path).unwrap_or_else(|error| error.to_string())
                )
            })
            .collect();
        panic!(
            "fake server {pid} wrote no argv record within {PATIENCE:?}; facts now {:?}; spools {output:?}",
            self.adapter.inspect(pid),
        );
    }

    /// A suspended fake server in a fresh data folder, with the arguments
    /// it got.
    fn spawn_fake(&self, lock: Option<&InstanceLock>) -> (SuspendedChild, PathBuf, Vec<String>) {
        let data = self.data_dir();
        let args = serve_args(allocate_port().expect("a free port"));
        let child = self.spawn_main(&fake_program(), &args, &[], &data, lock);
        (child, data, args)
    }

    fn spawn_helper(
        &self,
        program: &str,
        args: &[&str],
        stdin: StdinSource,
    ) -> stackly_platform::HelperHandle {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let helper = self
            .adapter
            .spawn_helper(SpawnRequest {
                program: Path::new(program),
                args: &args,
                env: &[],
                cwd: &self.dir,
                stdin,
                output: OutputTarget::Capture,
                lock: None,
                role: ChildRole::Probe,
            })
            .expect("the helper spawns");
        if let Some(facts) = self.adapter.inspect(helper.pid()).expect("inspect works") {
            self.track(facts);
        }
        helper
    }

    fn track(&self, facts: ProcessFacts) {
        self.children
            .lock()
            .expect("no test thread panics")
            .push(facts);
    }

    fn watch(&self, pid: Pid) -> Arc<Notices> {
        let notices = Arc::new(Notices::default());
        self.adapter
            .watch_exit(pid, notices.clone())
            .expect("the watch registers");
        notices
    }

    fn target(&self, facts: &ProcessFacts) -> SignalTarget {
        SignalTarget::new(ProcessIdentity {
            pid: facts.pid,
            start: facts.start,
            executable: facts.executable.clone(),
            session: facts.session,
            boot: self.adapter.boot_id().expect("the boot ID can be read"),
        })
    }
}

impl Drop for Setup {
    /// Kills every child that is still alive with its original start time,
    /// and waits until the adapter's watcher reaped it.
    fn drop(&mut self) {
        let children = std::mem::take(self.children.get_mut().unwrap_or_else(|e| e.into_inner()));
        for facts in children {
            let alive = matches!(
                self.adapter.inspect(facts.pid),
                Ok(Some(now)) if now.start == facts.start
            );
            if !alive {
                continue;
            }
            let notices = Arc::new(Notices::default());
            let watched = self.adapter.watch_exit(facts.pid, notices.clone()).is_ok();
            let _ = Command::new("/bin/kill")
                .args(["-9", &facts.pid.to_string()])
                .status();
            if watched {
                let _ = notices.wait();
            }
        }
    }
}

fn fake_program() -> PathBuf {
    fs::canonicalize(fake_server::binary()).expect("the fake server binary exists")
}

fn serve_args(port: u16) -> Vec<String> {
    vec![
        "serve".to_owned(),
        "--config=config.json".to_owned(),
        "--data=.".to_owned(),
        format!("--port={port}"),
        format!("--label={LABEL}"),
    ]
}

fn uid() -> UserId {
    MacosIdentity
        .current_user()
        .expect("the current user is known")
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until the lock at `path` is free. A process another test spawns
/// at the same moment shares every open file of this test process until
/// its exec closes them, so a lock can read held for that instant after
/// its last holder closed it.
fn wait_for_free(setup: &Setup, path: &Path) {
    wait_for("the lock to be free", || {
        setup
            .adapter
            .probe_lock(path)
            .expect("the lock file can be probed")
            == LockState::Free
    });
}

fn argv_record(data: &Path) -> Option<Vec<String>> {
    let bytes = fs::read(data.join("argv-serve.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn names_under(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        found.push(entry.file_name().to_string_lossy().into_owned());
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            names_under(&path, found);
        }
    }
}

fn pid(number: u32) -> Pid {
    Pid::new(number).expect("a test PID is in range")
}

#[test]
fn a_main_process_starts_suspended_with_exactly_the_plan_and_only_fds_0_to_3() {
    let ev = evidence!(
        covers = ["AC-058", "AC-031"],
        kind = Kind::Fixture,
        expected = "spawn_main under a 'Test Root Ünï cödé' folder runs stackly-fake-server suspended in its own session with no blocked signals and SIGPIPE and SIGHUP back to default, holding only fds 0 to 3 with the instance lock at fd 3; after resume its argv record equals the plan byte for byte, shell syntax in an argument creates no file, the relative config proves the cwd, and the lock stays held by the child alone",
    );
    let setup = Setup::new();
    let lock = setup.lock();
    let (child, data, args) = setup.spawn_fake(Some(&lock));
    let facts = child.facts().clone();
    let me = Pid::new(std::process::id()).unwrap();

    assert!(facts.stopped, "a main process starts suspended");
    assert_eq!(facts.executable, fake_program());
    assert_eq!(facts.session, facts.pid);
    assert_eq!(facts.group, facts.pid);
    assert_eq!(facts.parent, Some(me));
    assert_eq!(facts.uid, uid());
    assert_eq!(setup.adapter.fd_numbers(facts.pid).unwrap(), [0, 1, 2, 3]);
    assert_eq!(
        setup
            .adapter
            .holds_file(facts.pid, lock.identity())
            .unwrap(),
        HoldsFile::Yes
    );
    let kinfo = setup.adapter.inspect_kinfo(facts.pid).unwrap().unwrap();
    assert_eq!(kinfo.blocked, 0, "no signal is blocked");
    assert_eq!(kinfo.pending, 0, "no signal is pending");
    assert_eq!(
        kinfo.ignored & (HUP_BIT | PIPE_BIT),
        0,
        "{:#x}",
        kinfo.ignored
    );
    thread::sleep(QUIET);
    assert_eq!(argv_record(&data), None, "a suspended child runs no code");

    drop(lock);
    let lock_path = setup.dir.join("instance.lock");
    assert_eq!(
        setup.adapter.probe_lock(&lock_path).unwrap(),
        LockState::Held,
        "the child holds the lock it inherited after the daemon closed its copy"
    );
    let notices = setup.watch(facts.pid);
    let handle = setup.adapter.resume(child).unwrap();
    assert_eq!(handle.pid(), facts.pid);
    let argv = setup.wait_for_argv(facts.pid, &data);

    assert_eq!(argv, args, "argv after argv[0] is the plan");
    let mut names = Vec::new();
    names_under(setup.root.path(), &mut names);
    names_under(setup.root.run_dir(), &mut names);
    names_under(&std::env::current_dir().unwrap(), &mut names);
    assert!(
        !names.iter().any(|name| name.starts_with("pwned")),
        "no shell read the label: {names:?}"
    );

    setup
        .adapter
        .signal(&setup.target(&facts), ProviderSignal::Term)
        .unwrap();
    let notice = notices.wait();
    assert_eq!(notice.pid, facts.pid);
    assert_eq!(notice.status, Some(ExitStatus::Exited(0)));
    assert_eq!(notice.captured, None);
    wait_for_free(&setup, &lock_path);
    ev.pass();
}

#[test]
fn a_main_process_sees_only_the_plan_environment_and_writes_to_its_spools() {
    let setup = Setup::new();
    let env = vec![
        ("FAKE_A".to_owned(), "a b".to_owned()),
        ("SECOND".to_owned(), "ü 空 $HOME".to_owned()),
    ];
    let paths = setup.spool_paths();
    let child = setup
        .adapter
        .spawn_main(SpawnRequest {
            program: Path::new("/usr/bin/env"),
            args: &[],
            env: &env,
            cwd: &setup.dir,
            stdin: StdinSource::Null,
            output: setup.spool_output(&paths),
            lock: None,
            role: ChildRole::Main,
        })
        .unwrap();
    setup.track(child.facts().clone());
    let notices = setup.watch(child.pid());

    setup.adapter.resume(child).unwrap();

    assert_eq!(notices.wait().status, Some(ExitStatus::Exited(0)));
    assert_eq!(
        fs::read_to_string(&paths[0]).unwrap(),
        "FAKE_A=a b\nSECOND=ü 空 $HOME\n",
        "nothing of the daemon's environment reaches the child"
    );
    assert_eq!(fs::read_to_string(&paths[1]).unwrap(), "");
}

#[test]
fn a_discarded_child_never_runs_and_its_lock_is_free_afterwards() {
    let setup = Setup::new();
    let lock = setup.lock();
    let (child, data, _) = setup.spawn_fake(Some(&lock));
    let pid = child.pid();
    drop(lock);
    let notices = setup.watch(pid);

    assert_eq!(setup.adapter.discard(child).unwrap(), None);

    let notice = notices.wait();
    assert_eq!(notice.status, Some(ExitStatus::Signaled(9)));
    assert_eq!(setup.adapter.inspect(pid).unwrap(), None);
    assert_eq!(argv_record(&data), None, "it never ran");
    wait_for_free(&setup, &setup.dir.join("instance.lock"));
}

#[test]
fn a_suspended_child_that_died_on_its_own_is_discarded_with_its_status_and_no_signal() {
    let setup = Setup::new();
    let (child, data, _) = setup.spawn_fake(None);
    let facts = child.facts().clone();
    // Someone other than the adapter ends it, as the OS does with a binary
    // it refuses.
    let killed = Command::new("/bin/kill")
        .args(["-9", &facts.pid.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    wait_for("the child to end", || {
        setup.adapter.inspect(facts.pid).unwrap().is_none()
    });

    assert_eq!(
        setup.adapter.discard(child),
        Ok(Some(ExitStatus::Signaled(9))),
        "discard answers the status the child already had (K7)"
    );
    assert_eq!(argv_record(&data), None, "it never ran");
    assert_eq!(
        setup.adapter.discard(SuspendedChild::new(facts.clone())),
        Err(ProcessError::NotFound { pid: facts.pid })
    );
    let gone = setup.watch(facts.pid).wait();
    assert_eq!(
        gone.status, None,
        "the discard reaped it, so a later watch knows nothing"
    );
}

#[test]
fn an_unsigned_binary_the_os_kills_is_still_spawned_and_discarded() {
    let setup = Setup::new();
    let unsigned = setup.root.path().join("unsigned-fake-server");
    fs::copy(fake_program(), &unsigned).unwrap();
    let removed = Command::new("/usr/bin/codesign")
        .arg("--remove-signature")
        .arg(&unsigned)
        .status()
        .unwrap();
    assert!(removed.success());

    // The kernel kills an unsigned arm64 binary during exec or at its
    // first instruction, so the child is a zombie or waits suspended:
    // either way the spawn works and the discard tells which.
    let child = setup.spawn_main(&unsigned, &["version".to_owned()], &[], &setup.dir, None);
    assert_eq!(child.facts().executable, unsigned);
    let pid = child.pid();
    let paths_written = fs::read_dir(setup.root.run_dir()).unwrap().count();

    let discarded = setup.adapter.discard(child).unwrap();

    assert!(
        matches!(discarded, None | Some(ExitStatus::Signaled(9))),
        "{discarded:?}"
    );
    assert_eq!(setup.adapter.inspect(pid).unwrap(), None);
    assert_eq!(
        fs::read_dir(setup.root.run_dir()).unwrap().count(),
        paths_written
    );
}

#[test]
fn the_kinfo_fallback_reads_what_proc_pidtbsdinfo_reads_for_a_child() {
    let setup = Setup::new();
    let (child, _, _) = setup.spawn_fake(None);
    let facts = child.facts().clone();
    let UserId::Unix(me) = uid() else {
        panic!("a macOS user has a Unix ID");
    };

    let kinfo = setup.adapter.inspect_kinfo(facts.pid).unwrap().unwrap();

    assert_eq!(kinfo.start, facts.start);
    assert!(kinfo.stopped && facts.stopped);
    assert!(!kinfo.zombie);
    assert_eq!(Some(kinfo.parent), facts.parent.map(Pid::as_i32));
    assert_eq!(kinfo.group, facts.group.as_i32());
    assert_eq!(UserId::Unix(kinfo.uid), facts.uid);
    assert_eq!((kinfo.real_uid, kinfo.saved_uid), (me, me));

    setup.adapter.discard(child).unwrap();
    assert_eq!(
        setup.adapter.inspect_kinfo(facts.pid).unwrap(),
        None,
        "a reaped PID answers a length of 0"
    );
}

#[test]
fn a_signal_reaches_only_the_identity_it_targets() {
    let setup = Setup::new();
    let (child, data, _) = setup.spawn_fake(None);
    let facts = child.facts().clone();
    let notices = setup.watch(facts.pid);
    setup.adapter.resume(child).unwrap();
    setup.wait_for_argv(facts.pid, &data);
    let right = setup.target(&facts);
    let mut later = right.expected().clone();
    later.start = ProcessStart::new(facts.start.seconds() + 1, facts.start.micros()).unwrap();
    let mut other_session = right.expected().clone();
    other_session.session = pid(1);

    for wrong in [later, other_session] {
        assert_eq!(
            setup
                .adapter
                .signal(&SignalTarget::new(wrong), ProviderSignal::Term),
            Err(ProcessError::IdentityMismatch { pid: facts.pid })
        );
    }
    setup.adapter.signal(&right, ProviderSignal::Hup).unwrap();
    setup.adapter.continue_stopped(&right).unwrap();
    thread::sleep(QUIET);
    assert_eq!(
        notices.count(),
        0,
        "neither a refused signal nor SIGHUP ended it"
    );
    assert!(setup.adapter.inspect(facts.pid).unwrap().is_some());

    setup.adapter.signal(&right, ProviderSignal::Term).unwrap();
    assert_eq!(notices.wait().status, Some(ExitStatus::Exited(0)));
    assert_eq!(
        setup.adapter.signal(&right, ProviderSignal::Term),
        Err(ProcessError::NotFound { pid: facts.pid })
    );
}

#[test]
fn another_users_process_has_facts_from_kinfo_and_is_never_signalled() {
    let ev = evidence!(
        covers = ["AC-067"],
        kind = Kind::Fixture,
        expected = "inspect of PID 1 (root's launchd, which PROC_PIDTBSDINFO refuses with EPERM) answers facts from kinfo_proc with uid 0, so a reused PID is proven foreign; holds_file answers Unreadable, and a signal to it is refused as IdentityMismatch, never sent and never PermissionDenied",
    );
    let setup = Setup::new();
    let launchd = pid(1);

    let facts = setup.adapter.inspect(launchd).unwrap().unwrap();

    assert_eq!(facts.uid, UserId::Unix(0));
    assert_eq!(facts.parent, None, "the kernel is launchd's parent");
    assert_eq!((facts.group, facts.session), (launchd, launchd));
    assert_eq!(facts.executable, Path::new("/sbin/launchd"));
    assert!(!facts.stopped);
    assert_eq!(
        setup.adapter.holds_file(
            launchd,
            stackly_platform::FileIdentity {
                device: 1,
                inode: 2,
            }
        ),
        Ok(HoldsFile::Unreadable)
    );
    assert_eq!(
        setup.adapter.fd_numbers(launchd),
        Err(ProcessError::PermissionDenied { pid: launchd })
    );
    let target = setup.target(&facts);
    assert_eq!(
        setup.adapter.signal(&target, ProviderSignal::Hup),
        Err(ProcessError::IdentityMismatch { pid: launchd })
    );
    assert_eq!(
        setup.adapter.continue_stopped(&target),
        Err(ProcessError::IdentityMismatch { pid: launchd })
    );
    ev.pass();
}

#[test]
fn a_helper_reads_its_stdin_secret_and_its_output_tails_are_captured() {
    let setup = Setup::new();
    let credential = FixtureCredential::new();
    let value = SecretValue::new(credential.value().to_owned()).unwrap();

    let cat = setup.spawn_helper("/bin/cat", &[], StdinSource::Bytes(value));
    let notice = setup.watch(cat.pid()).wait();

    assert_eq!(notice.status, Some(ExitStatus::Exited(0)));
    let captured = notice.captured.unwrap();
    assert!(
        captured.stdout == credential.value().as_bytes(),
        "cat echoed exactly the secret it read on stdin"
    );
    assert!(captured.stderr.is_empty());

    let flood = setup.spawn_helper(
        "/bin/sh",
        &[
            "-c",
            "head -c 100000 /dev/zero; printf done; printf oops >&2; exit 3",
        ],
        StdinSource::Null,
    );
    let notice = setup.watch(flood.pid()).wait();

    assert_eq!(notice.status, Some(ExitStatus::Exited(3)));
    let captured = notice.captured.unwrap();
    assert_eq!(captured.stdout.len(), HELPER_CAPTURE_MAX);
    assert!(captured.stdout.ends_with(b"done"));
    assert_eq!(captured.stderr, b"oops");
}

#[test]
fn a_helper_group_that_ignores_term_ends_only_on_force() {
    let setup = Setup::new();
    let helper = setup.spawn_helper(
        "/bin/sh",
        &["-c", "trap '' TERM; /bin/sleep 30 & wait"],
        StdinSource::Null,
    );
    let notices = setup.watch(helper.pid());
    wait_for("the helper's descendant", || {
        setup.adapter.group_members(helper.pid()).unwrap().len() == 2
    });
    let members = setup.adapter.group_members(helper.pid()).unwrap();
    assert!(members.contains(&helper.pid()));
    for member in &members {
        if let Some(facts) = setup.adapter.inspect(*member).unwrap() {
            setup.track(facts);
        }
    }

    setup
        .adapter
        .terminate_helper_group(&helper, false)
        .unwrap();
    thread::sleep(QUIET);
    assert_eq!(notices.count(), 0, "the group ignores SIGTERM");
    assert_eq!(setup.adapter.group_members(helper.pid()).unwrap(), members);

    setup.adapter.terminate_helper_group(&helper, true).unwrap();
    assert_eq!(notices.wait().status, Some(ExitStatus::Signaled(9)));
    wait_for("the group to empty", || {
        setup
            .adapter
            .group_members(helper.pid())
            .unwrap()
            .is_empty()
    });
    assert_eq!(
        setup.adapter.terminate_helper_group(&helper, true),
        Err(ProcessError::NotFound { pid: helper.pid() }),
        "a reaped helper's group is never signalled"
    );
}

#[test]
fn a_process_that_is_not_a_child_is_watched_but_never_reaped() {
    let setup = Setup::new();
    let mut sleeper = Command::new("/bin/sleep").arg("0.3").spawn().unwrap();
    let sleeper_pid = pid(sleeper.id());

    let notice = setup.watch(sleeper_pid).wait();

    assert_eq!(
        notice,
        ExitNotice {
            pid: sleeper_pid,
            status: None,
            captured: None,
        }
    );
    assert!(
        sleeper.wait().unwrap().success(),
        "its own parent still reaps it"
    );
    let gone = setup.watch(sleeper_pid).wait();
    assert_eq!(gone.status, None, "a gone PID is reported at once");
}

#[test]
fn the_instance_lock_is_exclusive_regular_and_free_when_missing() {
    let ev = evidence!(
        covers = ["REQ-013"],
        kind = Kind::Fixture,
        expected = "lock_instance takes an exclusive flock on a 0600 regular file of the user and refuses a second taker; probe_lock answers Held while any open file holds it, Free once it closes, and Free for a missing file without creating it; a symlink or a folder in its place is LockFileInvalid",
    );
    let setup = Setup::new();
    let path = setup.dir.join("instance.lock");
    let missing = setup.dir.join("missing.lock");

    assert_eq!(setup.adapter.probe_lock(&missing).unwrap(), LockState::Free);
    assert!(!missing.exists(), "a probe never creates the lock file");
    let lock = setup.lock();
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(lock.path(), path);
    assert_eq!(lock.identity().inode, metadata.ino());
    assert!(matches!(
        setup.adapter.lock_instance(&path).unwrap(),
        LockOutcome::Held
    ));
    assert_eq!(setup.adapter.probe_lock(&path).unwrap(), LockState::Held);
    drop(lock);
    wait_for_free(&setup, &path);

    let link = setup.dir.join("link.lock");
    symlink(&path, &link).unwrap();
    let folder = setup.dir.join("folder.lock");
    fs::create_dir(&folder).unwrap();
    for (bad, reason) in [(&link, "is a symlink"), (&folder, "is not a regular file")] {
        let expected = Err(ProcessError::LockFileInvalid {
            path: bad.clone(),
            reason,
        });
        assert!(
            matches!(setup.adapter.lock_instance(bad), Err(ProcessError::LockFileInvalid { reason: r, .. }) if r == reason)
        );
        assert_eq!(setup.adapter.probe_lock(bad), expected);
    }
    ev.pass();
}

#[test]
fn a_spool_reader_frees_only_whole_pages_below_what_the_sink_took() {
    let setup = Setup::new();
    let path = setup.root.run_dir().join("stdout.spool");
    let writer = setup.adapter.create_spool(&path).unwrap();
    assert!(
        setup.adapter.create_spool(&path).is_err(),
        "a spool is always new"
    );
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let data: Vec<u8> = (0..100_000_u32).map(|n| (n % 251) as u8 + 1).collect();
    std::io::Write::write_all(&mut writer.file(), &data).unwrap();
    writer.file().sync_all().unwrap();
    let identity = writer.identity();
    drop(writer);

    let mut reader = setup.adapter.open_spool(&path).unwrap().unwrap();
    let unit = reader.punch_unit().get();
    assert_eq!(unit, 16_384, "one VM page of Apple silicon");
    assert_eq!(reader.file_identity(), identity);
    assert_eq!(reader.len().unwrap(), 100_000);
    assert_eq!(reader.first_data_offset().unwrap(), Some(0));
    let blocks_before = fs::metadata(&path).unwrap().blocks();

    reader.release_below(40_000).unwrap();

    assert_eq!(reader.first_data_offset().unwrap(), Some(2 * unit));
    assert!(fs::metadata(&path).unwrap().blocks() < blocks_before);
    let mut buf = vec![0xaa_u8; 100];
    assert_eq!(reader.read_at(0, &mut buf).unwrap(), 100);
    assert!(
        buf.iter().all(|byte| *byte == 0),
        "freed bytes read as zeros"
    );
    let start = usize::try_from(2 * unit).unwrap();
    assert_eq!(reader.read_at(2 * unit, &mut buf).unwrap(), 100);
    assert_eq!(buf, data[start..start + 100]);

    reader.release_below(10).unwrap();
    assert_eq!(reader.first_data_offset().unwrap(), Some(2 * unit));
    reader.release_below(u64::MAX).unwrap();
    assert_eq!(
        reader.first_data_offset().unwrap(),
        Some(6 * unit),
        "a release past the end stops at the last whole unit"
    );
    drop(reader);
    let reopened = setup.adapter.open_spool(&path).unwrap().unwrap();
    assert_eq!(reopened.first_data_offset().unwrap(), Some(6 * unit));

    assert!(
        setup
            .adapter
            .open_spool(&setup.root.run_dir().join("none.spool"))
            .unwrap()
            .is_none()
    );
    let link = setup.root.run_dir().join("link.spool");
    symlink(&path, &link).unwrap();
    assert!(setup.adapter.open_spool(&link).is_err());
    assert!(setup.adapter.create_spool(&link).is_err());
}

#[test]
fn the_boot_id_is_the_kernel_boot_session_uuid() {
    let setup = Setup::new();
    let output = Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.bootsessionuuid"])
        .output()
        .unwrap();
    let expected = String::from_utf8(output.stdout).unwrap();

    let boot = setup.adapter.boot_id().unwrap();

    assert_eq!(boot.to_string(), expected.trim().to_lowercase());
    assert_eq!(setup.adapter.boot_id().unwrap(), boot);
}

#[test]
fn a_bad_request_is_refused_before_anything_runs() {
    let setup = Setup::new();
    let nul = ["ok".to_owned(), "nul\0inside".to_owned()];
    let helper = |program: &Path, args: &[String], cwd: &Path, role: ChildRole| {
        setup.adapter.spawn_helper(SpawnRequest {
            program,
            args,
            env: &[],
            cwd,
            stdin: StdinSource::Null,
            output: OutputTarget::Capture,
            lock: None,
            role,
        })
    };
    let echo = Path::new("/bin/echo");
    let missing = setup.dir.join("missing");

    assert_eq!(
        helper(echo, &nul, &setup.dir, ChildRole::Probe),
        Err(ProcessError::InvalidArgument {
            position: RequestPart::Arg(1)
        })
    );
    assert_eq!(
        helper(echo, &[], &setup.dir, ChildRole::Main),
        Err(ProcessError::InvalidArgument {
            position: RequestPart::Role
        })
    );
    for (program, cwd) in [(missing.as_path(), setup.dir.as_path()), (echo, &missing)] {
        assert!(
            matches!(
                helper(program, &[], cwd, ChildRole::Probe),
                Err(ProcessError::SpawnFailed { errno: 2, .. })
            ),
            "a missing program or working directory is ENOENT from posix_spawn"
        );
    }
    let stranger = pid(99_999);
    let forged = SuspendedChild::new(ProcessFacts {
        pid: stranger,
        parent: None,
        group: stranger,
        session: stranger,
        uid: uid(),
        start: ProcessStart::new(1, 0).unwrap(),
        executable: echo.to_path_buf(),
        stopped: true,
    });
    assert_eq!(
        setup.adapter.discard(forged),
        Err(ProcessError::NotFound { pid: stranger }),
        "only a child of this adapter can be discarded"
    );
}

#[test]
fn macos_platform_supports_and_installs_process_supervision() {
    let root = TestRoot::new().unwrap();
    let platform = MacosPlatform::with_roots(root.roots());

    assert_eq!(
        platform
            .info()
            .capabilities()
            .status(PlatformCapability::ProcessSupervision),
        CapabilityStatus::Supported
    );
    assert!(platform.processes().boot_id().is_ok());
    assert_eq!(
        platform
            .processes()
            .probe_lock(&root.path().join("instance.lock")),
        Ok(LockState::Free)
    );
}
