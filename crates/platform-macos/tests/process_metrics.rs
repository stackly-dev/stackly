//! `MacosProcessSupervision::list_pids` and `counters` against real
//! processes (spec 0028 build step 3): a three level `sh` tree this test
//! starts in its own process group, a reaped child as the gone PID, and
//! PID 1 as a process of another user. The group is killed and reaped when
//! the test ends, also when it panics.
#![cfg(target_os = "macos")]

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use stackly_domain::{Pid, ProcessStart};
use stackly_platform::{ProcessError, ProcessSupervision};
use stackly_platform_macos::MacosProcessSupervision;
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{MetricsTree, process_metrics_conformance};

/// How long the test waits for the tree to form.
const PATIENCE: Duration = Duration::from_secs(10);

/// A process group of `sh`, `sh`, and `sleep`, killed and reaped on drop.
struct Tree {
    child: Child,
}

impl Tree {
    /// `sh` runs a second `sh` in the background and waits; the second
    /// runs `sleep` and then `:`, so it stays the parent of `sleep`.
    fn start() -> Self {
        let child = Command::new("/bin/sh")
            .args(["-c", "/bin/sh -c '/bin/sleep 60; :' & wait"])
            .process_group(0)
            .spawn()
            .expect("the shell starts");
        Self { child }
    }

    fn main(&self) -> Pid {
        Pid::new(self.child.id()).expect("a child has a valid pid")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        kill_group(self.main());
        let _ = self.child.wait();
    }
}

/// Kills the process group this test created and led with `leader`.
fn kill_group(leader: Pid) {
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{leader}")])
        .status();
}

/// The only child of `parent`, waiting until there is exactly one.
fn only_child(adapter: &MacosProcessSupervision, parent: Pid) -> Pid {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let children: Vec<Pid> = adapter
            .list_pids()
            .expect("the pid list reads")
            .into_iter()
            .filter(|pid| {
                adapter
                    .inspect(*pid)
                    .ok()
                    .flatten()
                    .is_some_and(|facts| facts.parent == Some(parent))
            })
            .collect();
        if let [child] = children[..] {
            return child;
        }
        assert!(Instant::now() < deadline, "the tree below {parent} formed");
        thread::sleep(Duration::from_millis(20));
    }
}

/// A PID that just exited and was reaped, with the start time it had.
fn gone_process(adapter: &MacosProcessSupervision) -> (Pid, ProcessStart) {
    let mut child = Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("sleep starts");
    let pid = Pid::new(child.id()).expect("a child has a valid pid");
    let start = adapter
        .inspect(pid)
        .expect("inspect reads a live child")
        .expect("the child is alive")
        .start;
    child.kill().expect("the child can be killed");
    child.wait().expect("the child is reaped");
    (pid, start)
}

#[test]
fn the_macos_adapter_passes_the_process_metrics_conformance() {
    let ev = evidence!(
        covers = ["AC-018"],
        kind = Kind::Fixture,
        expected = "list_pids lists a real sh, sh, sleep tree and PID 1; inspect parents rebuild it at depths 0 to 2; counters read a footprint above 0 and CPU and disk counters that never go back; a wrong start is IdentityMismatch, a reaped PID NotFound, and PID 1 PermissionDenied",
    );
    let adapter = MacosProcessSupervision::new();
    let tree = Tree::start();
    let main = tree.main();
    let child = only_child(&adapter, main);
    let grandchild = only_child(&adapter, child);
    let gone = gone_process(&adapter);
    let launchd = Pid::new(1).expect("1 is a valid pid");

    process_metrics_conformance(
        &adapter,
        MetricsTree::new(main, child, grandchild, gone, launchd),
    );
    ev.pass();
}

#[test]
fn cpu_time_is_converted_from_mach_units_to_real_time() {
    let ev = evidence!(
        covers = ["AC-018"],
        kind = Kind::Fixture,
        expected = "a shell loop that burns about 300 ms of CPU reads between 100 ms and 10 s of user plus system time, so the mach time units are converted (unconverted they read about 41 times too little)",
    );
    let adapter = MacosProcessSupervision::new();
    let mut busy = Command::new("/bin/sh")
        .args([
            "-c",
            "i=0; while [ $i -lt 400000 ]; do i=$((i+1)); done; sleep 30",
        ])
        .process_group(0)
        .spawn()
        .expect("the shell starts");
    let pid = Pid::new(busy.id()).expect("a child has a valid pid");
    let start = adapter
        .inspect(pid)
        .expect("inspect reads a live child")
        .expect("the child is alive")
        .start;
    let deadline = Instant::now() + PATIENCE;
    let cpu = loop {
        let counters = adapter.counters(pid, start).expect("counters read");
        let cpu = counters.cpu_total();
        if cpu >= Duration::from_millis(100) || Instant::now() >= deadline {
            break cpu;
        }
        thread::sleep(Duration::from_millis(50));
    };
    kill_group(pid);
    let _ = busy.wait();

    assert!(
        cpu >= Duration::from_millis(100) && cpu <= Duration::from_secs(10),
        "{cpu:?}"
    );
    assert_eq!(
        adapter.counters(pid, start),
        Err(ProcessError::NotFound { pid })
    );
    ev.pass();
}
