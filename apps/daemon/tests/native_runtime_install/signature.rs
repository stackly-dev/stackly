//! What macOS itself decides about a binary (spec 0018 AC-12, spec 0010 K7):
//! a file that carries `com.apple.quarantine` is refused before it runs, a
//! native binary whose signature was damaged is killed by the kernel when
//! `spawn_main` runs it and that death is classified
//! `binary_rejected_by_os`, and the daemon names no tool but `codesign`.
//! The binary is the `nats-server` of the cached NATS artifact, unpacked by
//! the test into its own root, so the evidence names a real runtime.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use stackly_core::runtime::launch::{LaunchRefusal, check_launchable, classify_early_exit};
use stackly_platform::{
    ChildRole, ExitListener, ExitNotice, ExitStatus, OutputTarget, ProcessSupervision,
    SpawnRequest, StdinSource,
};
use stackly_platform_macos::MacosProcessSupervision;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use stackly_testkit::RuntimeCache;
use stackly_testkit::evidence::Source;

use crate::harness::{NATS, OWNER, Site, artifact_facts};

/// How long a killed child may take to be noticed.
const PATIENCE: Duration = Duration::from_secs(30);
/// The quarantine value Safari sets on a download.
const QUARANTINE: &str = "0083;5f5e1000;Safari;";
/// `LC_MAIN`, the load command that names the entry point.
const LC_MAIN: u32 = 0x8000_0028;

/// Unpacks the cached NATS artifact into `dir` and returns its
/// `nats-server`, or `None` when the spike never downloaded the artifact.
fn extract_nats(dir: &Path) -> Option<PathBuf> {
    let (sha256, size) = artifact_facts(NATS);
    let archive = RuntimeCache::open()?.entry(&sha256, size)?;
    let unpacked = Command::new("/usr/bin/tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .status()
        .expect("tar runs");
    assert!(unpacked.success(), "the cached archive unpacks");
    let server = dir.join("nats-server-v2.15.1-darwin-arm64/nats-server");
    assert!(server.is_file(), "the archive holds {}", server.display());
    Some(server)
}

/// The file offset of the first instruction of a thin 64 bit Mach-O.
fn entry_offset(bytes: &[u8]) -> usize {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    assert_eq!(word(0), 0xfeed_facf, "a thin 64 bit Mach-O");
    let mut at = 32;
    for _ in 0..word(16) {
        if word(at) == LC_MAIN {
            let offset = u64::from_le_bytes(bytes[at + 8..at + 16].try_into().expect("8 bytes"));
            return usize::try_from(offset).expect("an offset fits");
        }
        at += usize::try_from(word(at + 4)).expect("a size fits");
    }
    panic!("the binary has no LC_MAIN");
}

/// Collects the one exit notice of a watched process.
#[derive(Default)]
struct Notice {
    seen: Mutex<Option<ExitNotice>>,
    arrived: Condvar,
}

impl Notice {
    fn wait(&self) -> ExitNotice {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = self.seen.lock().expect("no listener panics");
        while seen.is_none() {
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("an exit notice arrives in time");
            seen = self
                .arrived
                .wait_timeout(seen, left)
                .expect("no listener panics")
                .0;
        }
        seen.take().expect("checked above")
    }
}

impl ExitListener for Notice {
    fn exited(&self, notice: ExitNotice) {
        *self.seen.lock().expect("no listener panics") = Some(notice);
        self.arrived.notify_all();
    }
}

/// Runs `program` as a main process, resumed, and returns how it ended.
fn run_main(
    adapter: &MacosProcessSupervision,
    root: &TestRoot,
    program: &Path,
    args: &[String],
) -> ExitStatus {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, Ordering::Relaxed);
    let spool = |stream: &str| {
        adapter
            .create_spool(&root.run_dir().join(format!("{stream}-{run}.spool")))
            .expect("a new spool can be created")
    };
    let child = adapter
        .spawn_main(SpawnRequest {
            program,
            args,
            env: &[],
            cwd: root.path(),
            stdin: StdinSource::Null,
            output: OutputTarget::Spool {
                stdout: spool("stdout"),
                stderr: spool("stderr"),
            },
            lock: None,
            role: ChildRole::Main,
        })
        .expect("the main process spawns, even a binary the OS will kill");
    let pid = child.pid();
    let notice = Arc::new(Notice::default());
    adapter
        .watch_exit(pid, notice.clone())
        .expect("the watch registers");
    // A kill during exec leaves nothing to resume; the notice tells either way.
    let _ = adapter.resume(child);
    notice
        .wait()
        .status
        .expect("the child was reaped by its watcher")
}

#[test]
#[ignore = "native: runs a binary the kernel kills; needs the NATS artifact in the runtime cache"]
fn a_binary_with_a_damaged_signature_is_killed_by_the_os_and_classified_as_rejected() {
    let ev = evidence!(
        covers = ["AC-012"],
        kind = Kind::Native,
        expected = "nats-server from the cached NATS artifact prints its version and exits 0 through spawn_main, and a copy with one byte of its first instruction changed is killed with SIGKILL that the daemon did not send, which classify_early_exit reports as binary_rejected_by_os",
    );
    let root = TestRoot::new().expect("a test root can be created");
    let Some(good) = extract_nats(root.path()) else {
        ev.blocked(
            "artifact nats-2.15.1-macos-aarch64 is not in the runtime cache",
            OWNER,
        );
        return;
    };
    let adapter = MacosProcessSupervision::new();
    let version = vec!["--version".to_owned()];
    let control = run_main(&adapter, &root, &good, &version);
    assert_eq!(control, ExitStatus::Exited(0));

    let mut bytes = fs::read(&good).expect("the binary can be read");
    let at = entry_offset(&bytes);
    bytes[at] ^= 0xff;
    let bad = root.path().join("nats-server-damaged");
    fs::write(&bad, &bytes).expect("the copy can be written");
    Command::new("/bin/chmod")
        .args(["755"])
        .arg(&bad)
        .status()
        .expect("chmod runs");
    // The signature still claims the old bytes: macOS must see it as damaged.
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(&bad)
        .output()
        .expect("codesign runs");
    assert!(!verified.status.success(), "codesign sees the damage");

    let status = run_main(&adapter, &root, &bad, &version);

    assert_eq!(status, ExitStatus::Signaled(9));
    assert_eq!(
        classify_early_exit(Some(status), false),
        Some(LaunchRefusal::BinaryRejectedByOs { signal: 9 })
    );
    let (sha256, _) = artifact_facts(NATS);
    ev.subject("nats", NATS.version, Some(&sha256), Source::Managed);
    ev.pass_with("the control nats-server --version exited 0; the damaged copy ended Signaled(9), classified binary_rejected_by_os");
}

#[test]
#[ignore = "native: needs the NATS artifact in the runtime cache and /usr/bin/xattr"]
fn an_executable_with_the_quarantine_attribute_is_refused_before_it_runs() {
    let ev = evidence!(
        covers = ["AC-012"],
        kind = Kind::Native,
        expected = "check_launchable on the real macOS adapter names runtime.quarantined for nats-server with com.apple.quarantine, nothing is spawned, and the same file without the attribute passes",
    );
    let site = Site::offline();
    let Some(program) = extract_nats(site.root.path()) else {
        ev.blocked(
            "artifact nats-2.15.1-macos-aarch64 is not in the runtime cache",
            OWNER,
        );
        return;
    };
    assert_eq!(check_launchable(&site.platform, &program), Ok(()));

    let set = Command::new("/usr/bin/xattr")
        .args(["-w", "com.apple.quarantine", QUARANTINE])
        .arg(&program)
        .status()
        .expect("xattr runs");
    assert!(set.success());
    let refusal = check_launchable(&site.platform, &program).expect_err("the file is quarantined");

    match refusal {
        LaunchRefusal::Quarantined { path, agent } => {
            assert_eq!(path, program);
            assert_eq!(agent.as_deref(), Some("Safari"));
        }
        other => panic!("expected a quarantine refusal, got {other:?}"),
    }
    assert!(
        site.spawned
            .lock()
            .expect("no test thread panics")
            .is_empty(),
        "nothing was spawned"
    );
    let (sha256, _) = artifact_facts(NATS);
    ev.subject("nats", NATS.version, Some(&sha256), Source::Managed);
    ev.pass_with("quarantined nats-server refused with agent Safari; nothing spawned; the control without the attribute passed");
}

/// Source files of the crates that start processes for the install, with
/// the lines that are not comments.
fn code_lines(dir: &Path, found: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir)
        .expect("the source folder can be read")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            code_lines(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = fs::read_to_string(&path).expect("a source file can be read");
            for line in text.lines() {
                if !line.trim_start().starts_with("//") {
                    found.push((path.clone(), line.to_owned()));
                }
            }
        }
    }
}

#[test]
fn no_daemon_code_names_a_developer_tool_to_run() {
    let ev = evidence!(
        covers = ["AC-049"],
        kind = Kind::Fixture,
        expected = "no non comment line of stackly-core, stackly-platform-macos, or stacklyd names otool, lipo, xcrun, xcodebuild, or clang as a program",
    );
    let daemon = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut lines = Vec::new();
    for dir in [
        "../../crates/core/src",
        "../../crates/platform-macos/src",
        "src",
    ] {
        code_lines(&daemon.join(dir), &mut lines);
    }
    assert!(!lines.is_empty());
    let programs = ["otool", "lipo", "xcrun", "xcodebuild", "clang"];
    let hits: Vec<_> = lines
        .iter()
        .filter(|(_, line)| {
            programs.iter().any(|tool| {
                line.contains(&format!("\"{tool}\"")) || line.contains(&format!("/{tool}\""))
            })
        })
        .collect();
    assert!(hits.is_empty(), "{hits:?}");
    ev.pass();
}
