//! One blocking pass over the OS for a tick (spec 0028, *Sampler*): the
//! list of PIDs, the facts of the processes not seen before, the tree of
//! every running main process, and the counters of every member.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use stackly_domain::{
    CounterReading, MemberRead, Pid, ProcessRead, ProcessStart, TreeNode, UnavailableReason,
    process_name, process_tree,
};
use stackly_platform::{ProcessError, ProcessFacts, ProcessSupervision, UserId};
use tokio::time::Instant;

use super::RunningMain;

/// The facts of every process seen so far, so `inspect` runs once per
/// process and not once per tick. An entry goes when its PID leaves the
/// process list, and is replaced when `counters` finds another process
/// behind the PID.
#[derive(Debug, Default)]
pub(super) struct FactsCache {
    facts: HashMap<Pid, ProcessFacts>,
}

impl FactsCache {
    /// Reads and keeps the facts of `pid`; `None` when it has none now.
    fn refresh(&mut self, processes: &dyn ProcessSupervision, pid: Pid) -> Option<&ProcessFacts> {
        match processes.inspect(pid) {
            Ok(Some(facts)) => {
                self.facts.insert(pid, facts);
            }
            Ok(None) | Err(_) => {
                self.facts.remove(&pid);
            }
        }
        self.facts.get(&pid)
    }
}

/// What the OS gave for one tick.
pub(super) struct TickReads {
    /// The members read for each running main process, the main process
    /// first.
    pub(super) instances: Vec<(RunningMain, Vec<MemberRead>)>,
}

/// Why a counters call gave nothing (spec 0028 AC-3): `ESRCH` and a reused
/// PID mean the process exited, `EPERM` means macOS refused. Any other
/// failure leaves no usable reading of that process either, and is
/// reported as `process_exited` because no other reason names it.
pub(super) fn reason_of(error: &ProcessError) -> UnavailableReason {
    match error {
        ProcessError::PermissionDenied { .. } => UnavailableReason::NotPermitted,
        ProcessError::NotSupported { .. } => UnavailableReason::NotSupported,
        _ => UnavailableReason::ProcessExited,
    }
}

/// Reads the OS once for every main process in `mains`.
///
/// `uid` is the daemon's user: only descendants of that user join a tree.
/// With no `uid` a tree is its main process alone. `origin` is the moment
/// the offsets of the readings count from.
///
/// # Errors
///
/// Returns the error of `list_pids` when the process list cannot be read,
/// except [`ProcessError::NotSupported`], which makes every main process
/// unavailable with reason `not_supported`: the history still records it.
pub(super) fn read_tick(
    processes: &dyn ProcessSupervision,
    uid: Option<&UserId>,
    mains: &[RunningMain],
    cache: &mut FactsCache,
    origin: Instant,
) -> Result<TickReads, ProcessError> {
    let pids = match processes.list_pids() {
        Ok(pids) => pids,
        Err(error @ ProcessError::NotSupported { .. }) => {
            return Ok(TickReads {
                instances: mains
                    .iter()
                    .map(|main| (*main, vec![gone(*main, reason_of(&error))]))
                    .collect(),
            });
        }
        Err(error) => return Err(error),
    };
    let alive: HashSet<Pid> = pids.iter().copied().collect();
    cache.facts.retain(|pid, _| alive.contains(pid));
    for pid in &pids {
        if !cache.facts.contains_key(pid) {
            cache.refresh(processes, *pid);
        }
    }

    // One read per process even when two trees share it.
    let mut memo: HashMap<Pid, Reading> = HashMap::new();
    let mut instances = Vec::with_capacity(mains.len());
    for main in mains {
        let members = match cache.facts.get(&main.pid) {
            Some(facts) if facts.start == main.start => {
                let parents: Vec<(Pid, Option<Pid>)> = cache
                    .facts
                    .values()
                    .filter(|facts| uid.is_some_and(|uid| facts.uid == *uid))
                    .map(|facts| (facts.pid, facts.parent))
                    .collect();
                process_tree(main.pid, &parents)
                    .into_iter()
                    .filter_map(|node| member(processes, cache, &mut memo, node, origin))
                    .collect()
            }
            _ => vec![gone(*main, UnavailableReason::ProcessExited)],
        };
        // A PID reused between the check and the read is not the main
        // process any more.
        let members = if members
            .first()
            .is_some_and(|first| first.node.pid == main.pid && first.start != main.start)
        {
            vec![gone(*main, UnavailableReason::ProcessExited)]
        } else {
            members
        };
        instances.push((*main, members));
    }
    Ok(TickReads { instances })
}

/// A main process with no reading.
fn gone(main: RunningMain, reason: UnavailableReason) -> MemberRead {
    MemberRead {
        node: TreeNode {
            pid: main.pid,
            parent: None,
            depth: 0,
        },
        start: main.start,
        name: String::new(),
        read: ProcessRead::Unavailable(reason),
    }
}

/// The reading of one tree member, from the memo when another tree read it
/// already in this tick. A process whose facts vanished while it was read
/// leaves the tree.
fn member(
    processes: &dyn ProcessSupervision,
    cache: &mut FactsCache,
    memo: &mut HashMap<Pid, Reading>,
    node: TreeNode,
    origin: Instant,
) -> Option<MemberRead> {
    if let Entry::Vacant(slot) = memo.entry(node.pid) {
        slot.insert(read_process(processes, cache, node.pid, origin)?);
    }
    let (start, name, read) = memo.get(&node.pid)?.clone();
    Some(MemberRead {
        node,
        start,
        name,
        read,
    })
}

/// The start time, executable name, and reading of one process.
type Reading = (ProcessStart, String, ProcessRead);

/// Reads the counters of `pid`. A PID reused since the facts were cached
/// fails the identity check; the facts are read again once, and the new
/// process is read as it is, with no baseline.
fn read_process(
    processes: &dyn ProcessSupervision,
    cache: &mut FactsCache,
    pid: Pid,
    origin: Instant,
) -> Option<Reading> {
    let mut facts = cache.facts.get(&pid)?.clone();
    for attempt in 0..2 {
        let before = Instant::now();
        let result = processes.counters(pid, facts.start);
        let after = Instant::now();
        match result {
            Ok(counters) => {
                let at = midpoint(before, after, origin);
                return Some((
                    facts.start,
                    process_name(&facts.executable),
                    ProcessRead::Counters(CounterReading { counters, at }),
                ));
            }
            Err(ProcessError::IdentityMismatch { .. }) if attempt == 0 => {
                match cache.refresh(processes, pid) {
                    Some(fresh) => facts = fresh.clone(),
                    None => break,
                }
            }
            Err(error) => {
                return Some((
                    facts.start,
                    process_name(&facts.executable),
                    ProcessRead::Unavailable(reason_of(&error)),
                ));
            }
        }
    }
    Some((
        facts.start,
        process_name(&facts.executable),
        ProcessRead::Unavailable(UnavailableReason::ProcessExited),
    ))
}

/// The moment halfway between two instants, as an offset from `origin`:
/// the OS read happened somewhere between them.
fn midpoint(before: Instant, after: Instant, origin: Instant) -> Duration {
    let start = before.saturating_duration_since(origin);
    start + after.saturating_duration_since(before) / 2
}
