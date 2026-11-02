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
    let parents = tree_parents(cache, uid, mains);
    let mut instances = Vec::with_capacity(mains.len());
    for main in mains {
        let members = match cache.facts.get(&main.pid) {
            Some(facts) if facts.start == main.start => process_tree(main.pid, &parents)
                .into_iter()
                .filter_map(|node| member(processes, cache, &mut memo, node, origin))
                .collect(),
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

/// The `(pid, parent)` pairs the trees of `mains` are built from, once per
/// tick: the processes of the daemon user `uid` whose line of parents
/// reaches a main process. No other process can join a tree, so the rest of
/// the user's processes are left out here rather than walked again for
/// every main. With no `uid` there are none, and a tree is its main process
/// alone.
fn tree_parents(
    cache: &FactsCache,
    uid: Option<&UserId>,
    mains: &[RunningMain],
) -> Vec<(Pid, Option<Pid>)> {
    let Some(uid) = uid else {
        return Vec::new();
    };
    let parent_of: HashMap<Pid, Option<Pid>> = cache
        .facts
        .values()
        .filter(|facts| facts.uid == *uid)
        .map(|facts| (facts.pid, facts.parent))
        .collect();
    let roots: HashSet<Pid> = mains.iter().map(|main| main.pid).collect();
    let mut joins: HashMap<Pid, bool> = HashMap::with_capacity(parent_of.len());
    for pid in parent_of.keys() {
        let mut line = Vec::new();
        let mut at = *pid;
        let found = loop {
            if roots.contains(&at) {
                break true;
            }
            if let Some(known) = joins.get(&at) {
                break *known;
            }
            // A line longer than the process list is a loop of parents.
            if line.len() > parent_of.len() {
                break false;
            }
            line.push(at);
            match parent_of.get(&at) {
                Some(Some(parent)) => at = *parent,
                _ => break false,
            }
        };
        for seen in line {
            joins.insert(seen, found);
        }
    }
    parent_of
        .into_iter()
        .filter(|(pid, _)| roots.contains(pid) || joins.get(pid) == Some(&true))
        .collect()
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use stackly_domain::InstanceId;

    use super::*;

    const ME: UserId = UserId::Unix(501);

    fn pid(n: u32) -> Pid {
        Pid::new(n).unwrap()
    }

    fn start(n: u32) -> ProcessStart {
        ProcessStart::new(u64::from(n), 0).unwrap()
    }

    fn cache(processes: &[(u32, Option<u32>, UserId)]) -> FactsCache {
        let facts = processes
            .iter()
            .map(|(n, parent, uid)| {
                let facts = ProcessFacts {
                    pid: pid(*n),
                    parent: parent.map(pid),
                    group: pid(*n),
                    session: pid(*n),
                    uid: uid.clone(),
                    start: start(*n),
                    executable: PathBuf::from(format!("/bin/proc{n}")),
                    stopped: false,
                };
                (pid(*n), facts)
            })
            .collect();
        FactsCache { facts }
    }

    fn main_process(n: u32) -> RunningMain {
        RunningMain::new(InstanceId::generate(), pid(n), start(n))
    }

    /// Every process of the user, as each tree was built from before.
    fn every_parent(cache: &FactsCache) -> Vec<(Pid, Option<Pid>)> {
        cache
            .facts
            .values()
            .filter(|facts| facts.uid == ME)
            .map(|facts| (facts.pid, facts.parent))
            .collect()
    }

    #[test]
    fn the_trees_from_the_pruned_parents_match_the_trees_from_every_process_of_the_user() {
        let other = UserId::Unix(0);
        let cache = cache(&[
            (1, None, other.clone()),
            (10, Some(1), ME),
            (11, Some(10), ME),
            (12, Some(11), ME),
            (13, Some(10), other.clone()),
            (14, Some(13), ME),
            (20, Some(1), ME),
            (21, Some(20), ME),
            (30, Some(1), ME),
            (31, Some(30), ME),
            (40, Some(41), ME),
            (41, Some(40), ME),
        ]);
        let mains = [main_process(10), main_process(20)];
        let pruned = tree_parents(&cache, Some(&ME), &mains);
        for main in &mains {
            assert_eq!(
                process_tree(main.pid, &pruned),
                process_tree(main.pid, &every_parent(&cache))
            );
        }
        let mut kept: Vec<u32> = pruned.iter().map(|(pid, _)| pid.get()).collect();
        kept.sort_unstable();
        assert_eq!(kept, vec![10, 11, 12, 20, 21]);
    }

    #[test]
    fn a_main_process_under_another_main_stays_in_the_outer_tree() {
        let cache = cache(&[(10, None, ME), (11, Some(10), ME), (12, Some(11), ME)]);
        let mains = [main_process(10), main_process(11)];
        let pruned = tree_parents(&cache, Some(&ME), &mains);
        for main in &mains {
            assert_eq!(
                process_tree(main.pid, &pruned),
                process_tree(main.pid, &every_parent(&cache))
            );
        }
    }

    #[test]
    fn a_loop_of_parents_that_holds_a_main_process_ends_and_matches() {
        let cache = cache(&[(10, Some(11), ME), (11, Some(10), ME), (12, Some(11), ME)]);
        let mains = [main_process(10)];
        let pruned = tree_parents(&cache, Some(&ME), &mains);
        assert_eq!(
            process_tree(pid(10), &pruned),
            process_tree(pid(10), &every_parent(&cache))
        );
    }

    #[test]
    fn with_no_user_there_are_no_parents_and_a_tree_is_its_main_process() {
        let cache = cache(&[(10, None, ME), (11, Some(10), ME)]);
        let mains = [main_process(10)];
        let pruned = tree_parents(&cache, None, &mains);
        assert!(pruned.is_empty());
        assert_eq!(process_tree(pid(10), &pruned).len(), 1);
    }
}
