//! Conformance checks for `ProcessSupervision::list_pids` and `counters`
//! (spec 0028, *Port platform*): the same three level tree, a process that
//! is gone, and a process of another user answer alike from the fake and
//! from each OS adapter.

use std::fmt::Debug;

use stackly_domain::{Pid, ProcessCounters, ProcessStart, process_tree};
use stackly_platform::{ProcessError, ProcessSupervision};

/// The processes [`process_metrics_conformance`] reads. The caller makes
/// them and keeps them alive for the whole check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MetricsTree {
    /// The top of the tree, a process of the current user.
    pub main: Pid,
    /// A child of `main`.
    pub child: Pid,
    /// A child of `child`.
    pub grandchild: Pid,
    /// A PID no live process has, with the start time it last had.
    pub gone: (Pid, ProcessStart),
    /// A live process of another user, such as PID 1.
    pub foreign: Pid,
}

impl MetricsTree {
    /// Names the processes of one check.
    #[must_use]
    pub const fn new(
        main: Pid,
        child: Pid,
        grandchild: Pid,
        gone: (Pid, ProcessStart),
        foreign: Pid,
    ) -> Self {
        Self {
            main,
            child,
            grandchild,
            gone,
            foreign,
        }
    }
}

/// Runs the same behavior checks of the metrics reads against any
/// [`ProcessSupervision`], so the fake in this crate and each OS adapter
/// cannot drift apart unnoticed.
///
/// It checks that `list_pids` lists every process of the tree and the
/// foreign one; that the parents `inspect` reports rebuild the tree with
/// `process_tree` at depths 0, 1, and 2; that `counters` reads a footprint
/// above 0 for each tree member and CPU time and disk bytes that never go
/// backwards between two reads; and that it answers `IdentityMismatch`
/// for a wrong start time, `NotFound` for the gone PID, and
/// `PermissionDenied` for the foreign process.
///
/// # Panics
///
/// Panics with the name of the first check that fails.
pub fn process_metrics_conformance(processes: &dyn ProcessSupervision, tree: MetricsTree) {
    let listed = processes
        .list_pids()
        .unwrap_or_else(|err| fail("list_pids", &err.to_string()));
    for (name, pid) in [
        ("main", tree.main),
        ("child", tree.child),
        ("grandchild", tree.grandchild),
        ("foreign", tree.foreign),
    ] {
        if !listed.contains(&pid) {
            fail(
                "list_pids lists every live process",
                &format!("{name} {pid} is missing"),
            );
        }
    }

    let mut parents = Vec::new();
    let mut starts = Vec::new();
    for pid in [tree.main, tree.child, tree.grandchild] {
        let facts = processes
            .inspect(pid)
            .unwrap_or_else(|err| fail("inspect a tree member", &err.to_string()))
            .unwrap_or_else(|| fail("inspect a tree member", &format!("{pid} has no facts")));
        parents.push((pid, facts.parent));
        starts.push((pid, facts.start));
    }
    let shape: Vec<(Pid, u32)> = process_tree(tree.main, &parents)
        .iter()
        .map(|node| (node.pid, node.depth))
        .collect();
    expect_eq(
        "the parents rebuild the tree",
        shape,
        vec![(tree.main, 0), (tree.child, 1), (tree.grandchild, 2)],
    );

    for (pid, start) in &starts {
        let first = read("counters of a tree member", processes, *pid, *start);
        let second = read("counters read again", processes, *pid, *start);
        if first.footprint == 0 {
            fail("a live process has a footprint", &format!("{pid} reads 0"));
        }
        never_backwards(*pid, &first, &second);
    }

    let (main, main_start) = starts[0];
    let wrong = ProcessStart::new(main_start.seconds().saturating_add(1), main_start.micros())
        .unwrap_or_else(|err| fail("a wrong start time", &err.to_string()));
    expect_error(
        "a wrong start time is IdentityMismatch",
        processes.counters(main, wrong),
        |err| matches!(err, ProcessError::IdentityMismatch { pid } if *pid == main),
    );
    let (gone, gone_start) = tree.gone;
    expect_error(
        "a gone process is NotFound",
        processes.counters(gone, gone_start),
        |err| matches!(err, ProcessError::NotFound { pid } if *pid == gone),
    );
    let foreign_start = processes
        .inspect(tree.foreign)
        .ok()
        .flatten()
        .map_or(main_start, |facts| facts.start);
    expect_error(
        "another user's process is PermissionDenied",
        processes.counters(tree.foreign, foreign_start),
        |err| matches!(err, ProcessError::PermissionDenied { pid } if *pid == tree.foreign),
    );
}

fn read(
    check: &str,
    processes: &dyn ProcessSupervision,
    pid: Pid,
    start: ProcessStart,
) -> ProcessCounters {
    processes
        .counters(pid, start)
        .unwrap_or_else(|err| fail(check, &format!("{pid}: {err}")))
}

fn never_backwards(pid: Pid, first: &ProcessCounters, second: &ProcessCounters) {
    if second.cpu_total() < first.cpu_total()
        || second.disk_read < first.disk_read
        || second.disk_write < first.disk_write
    {
        fail(
            "counters never go backwards",
            &format!("{pid}: {first:?} then {second:?}"),
        );
    }
}

fn expect_eq<T: PartialEq + Debug>(check: &str, actual: T, expected: T) {
    if actual != expected {
        fail(check, &format!("expected {expected:?}, got {actual:?}"));
    }
}

fn expect_error<T: Debug>(
    check: &str,
    result: Result<T, ProcessError>,
    matches: impl FnOnce(&ProcessError) -> bool,
) {
    match result {
        Err(err) if matches(&err) => {}
        other => fail(check, &format!("{other:?}")),
    }
}

fn fail(check: &str, detail: &str) -> ! {
    panic!("process_metrics_conformance check failed: {check}: {detail}")
}
