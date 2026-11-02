//! Resource metrics of a running instance (spec 0028): the counters one
//! process read gives, the rates two reads give, the process tree of a main
//! process, the sums for the main process and the whole tree, and the
//! bounded history of samples with its cursor.
//!
//! Everything here is pure. The sampler in `stackly-core` reads the OS and
//! the monotonic clock, then hands the readings in. A value that was not
//! measured is always a [`Metric::Unavailable`] with a reason, never 0.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::process::{Pid, ProcessStart};

/// Nanoseconds in one second, for the integer rate of Disk I/O.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

code_enum! {
    /// Why a [`Metric`] has no value (spec 0028 AC-3). The wire set is
    /// open, so a newer reason reaches an older client as unrecognized.
    pub enum UnavailableReason: UnavailableReason {
        /// The first sample of a process: there is no earlier read to take
        /// a difference from yet.
        WarmingUp => "warming_up",
        /// The OS refused to read the process (`EPERM`).
        NotPermitted => "not_permitted",
        /// The process exited while it was being sampled.
        ProcessExited => "process_exited",
        /// The platform has no adapter for the reading.
        NotSupported => "not_supported",
        /// The data folder was never scanned.
        NotScanned => "not_scanned",
    }
}

code_enum! {
    /// Which processes a [`ResourceUsage`] covers (spec 0028 AC-4).
    pub enum UsageScope: UsageScope {
        /// The main process alone.
        Main => "main",
        /// The main process and every descendant of the same user.
        Tree => "tree",
    }
}

/// One measured number, or the reason it could not be measured.
///
/// There is no default and no zero constructor, so code that did not
/// measure a value cannot write 0 for it.
///
/// ```
/// use stackly_domain::{Metric, UnavailableReason};
///
/// let cpu: Metric<f64> = Metric::Unavailable(UnavailableReason::WarmingUp);
/// assert_eq!(cpu.value(), None);
/// assert_eq!(Metric::Value(12.5).value(), Some(&12.5));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Metric<T> {
    /// The measured number.
    Value(T),
    /// No number, and why.
    Unavailable(UnavailableReason),
}

impl<T> Metric<T> {
    /// Returns the number, or `None` when it is unavailable.
    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Unavailable(_) => None,
        }
    }

    /// Returns the reason, or `None` when there is a number.
    #[must_use]
    pub const fn reason(&self) -> Option<UnavailableReason> {
        match self {
            Self::Value(_) => None,
            Self::Unavailable(reason) => Some(*reason),
        }
    }
}

/// What one OS read says about one process (spec 0028, *Port platform*).
///
/// The CPU times are real durations: the macOS adapter converts the mach
/// time units of `proc_pid_rusage` before it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessCounters {
    /// CPU time spent in user mode since the process started.
    pub cpu_user: Duration,
    /// CPU time spent in the kernel for the process since it started.
    pub cpu_system: Duration,
    /// The physical footprint in bytes, the number Activity Monitor calls
    /// Memory.
    pub footprint: u64,
    /// Bytes read from disk since the process started.
    pub disk_read: u64,
    /// Bytes written to disk since the process started.
    pub disk_write: u64,
}

impl ProcessCounters {
    /// Returns the user and system CPU time together.
    #[must_use]
    pub fn cpu_total(&self) -> Duration {
        self.cpu_user.saturating_add(self.cpu_system)
    }
}

/// The counters of one process with the monotonic moment of the read, as
/// an offset from any fixed origin the sampler keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterReading {
    /// The counters.
    pub counters: ProcessCounters,
    /// When they were read, on the monotonic clock.
    pub at: Duration,
}

/// The rates between two reads of the same process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    /// Percent of one logical core, to one decimal; 100 is one full core.
    pub cpu: f64,
    /// Bytes read from disk per second, rounded down.
    pub disk_read: u64,
    /// Bytes written to disk per second, rounded down.
    pub disk_write: u64,
}

/// The rates from `previous` to `current`, or `None` when no time passed
/// between them or a counter went backwards, which only a different
/// process can cause.
///
/// ```
/// use std::time::Duration;
/// use stackly_domain::{CounterReading, ProcessCounters, rates};
///
/// let read = |cpu_ms, at_s| CounterReading {
///     counters: ProcessCounters {
///         cpu_user: Duration::from_millis(cpu_ms),
///         ..ProcessCounters::default()
///     },
///     at: Duration::from_secs(at_s),
/// };
/// let rates = rates(&read(0, 0), &read(2_500, 5)).expect("time passed");
/// assert_eq!(rates.cpu, 50.0);
/// ```
#[must_use]
pub fn rates(previous: &CounterReading, current: &CounterReading) -> Option<Rates> {
    let elapsed = current.at.checked_sub(previous.at)?;
    if elapsed.is_zero() {
        return None;
    }
    let cpu = current
        .counters
        .cpu_total()
        .checked_sub(previous.counters.cpu_total())?;
    let read = current
        .counters
        .disk_read
        .checked_sub(previous.counters.disk_read)?;
    let written = current
        .counters
        .disk_write
        .checked_sub(previous.counters.disk_write)?;
    Some(Rates {
        cpu: round_tenth(cpu.as_secs_f64() / elapsed.as_secs_f64() * 100.0),
        disk_read: per_second(read, elapsed),
        disk_write: per_second(written, elapsed),
    })
}

/// Rounds to one decimal.
fn round_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Bytes per second over `elapsed`, rounded down, in integers.
fn per_second(bytes: u64, elapsed: Duration) -> u64 {
    let nanos = elapsed.as_nanos().max(1);
    u64::try_from(u128::from(bytes) * NANOS_PER_SECOND / nanos).unwrap_or(u64::MAX)
}

/// The result of reading one process in one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRead {
    /// The counters were read.
    Counters(CounterReading),
    /// They could not be: the process exited, or the OS refused.
    Unavailable(UnavailableReason),
}

/// The numbers of one process in one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessMeasure {
    /// CPU percent of one core.
    pub cpu: Metric<f64>,
    /// Physical footprint in bytes.
    pub memory: Metric<u64>,
    /// Bytes read per second.
    pub disk_read: Metric<u64>,
    /// Bytes written per second.
    pub disk_write: Metric<u64>,
    /// True when the counters were read and an earlier read of the same
    /// process gave a difference, so the process counts in CPU and Disk
    /// I/O and in `measured_count`.
    pub measured: bool,
}

impl ProcessMeasure {
    /// A process with no reading at all.
    #[must_use]
    pub const fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            cpu: Metric::Unavailable(reason),
            memory: Metric::Unavailable(reason),
            disk_read: Metric::Unavailable(reason),
            disk_write: Metric::Unavailable(reason),
            measured: false,
        }
    }
}

/// The numbers of one process from this read and the earlier read of the
/// same process, if any (spec 0028 AC-2): memory needs only this read, CPU
/// and Disk I/O need both.
#[must_use]
pub fn measure(previous: Option<&CounterReading>, read: &ProcessRead) -> ProcessMeasure {
    let current = match read {
        ProcessRead::Unavailable(reason) => return ProcessMeasure::unavailable(*reason),
        ProcessRead::Counters(current) => current,
    };
    let memory = Metric::Value(current.counters.footprint);
    match previous.and_then(|previous| rates(previous, current)) {
        Some(rates) => ProcessMeasure {
            cpu: Metric::Value(rates.cpu),
            memory,
            disk_read: Metric::Value(rates.disk_read),
            disk_write: Metric::Value(rates.disk_write),
            measured: true,
        },
        None => ProcessMeasure {
            memory,
            ..ProcessMeasure::unavailable(UnavailableReason::WarmingUp)
        },
    }
}

/// The usage of the main process or of the whole tree in one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ResourceUsage {
    /// Which processes it covers.
    pub scope: UsageScope,
    /// The processes found: 1 for the main process.
    pub process_count: u64,
    /// The processes counted in CPU and Disk I/O.
    pub measured_count: u64,
    /// CPU percent of one core; over 100 for a busy tree.
    pub cpu: Metric<f64>,
    /// Physical footprint in bytes.
    pub memory: Metric<u64>,
    /// Bytes read per second.
    pub disk_read: Metric<u64>,
    /// Bytes written per second.
    pub disk_write: Metric<u64>,
}

impl ResourceUsage {
    /// The usage of the main process alone.
    #[must_use]
    pub fn main(main: &ProcessMeasure) -> Self {
        Self {
            scope: UsageScope::Main,
            process_count: 1,
            measured_count: u64::from(main.measured),
            cpu: main.cpu,
            memory: main.memory,
            disk_read: main.disk_read,
            disk_write: main.disk_write,
        }
    }

    /// The usage of the whole tree: each value is the sum over the
    /// processes that have it, and unavailable with the main process's
    /// reason only when none has it (spec 0028 AC-3).
    #[must_use]
    pub fn tree(main: &ProcessMeasure, descendants: &[ProcessMeasure]) -> Self {
        let all = || std::iter::once(main).chain(descendants);
        let cpu = sum(all().map(|m| m.cpu), main.cpu).map_value(round_tenth);
        Self {
            scope: UsageScope::Tree,
            process_count: count(all().count()),
            measured_count: count(all().filter(|m| m.measured).count()),
            cpu,
            memory: sum(all().map(|m| m.memory), main.memory),
            disk_read: sum(all().map(|m| m.disk_read), main.disk_read),
            disk_write: sum(all().map(|m| m.disk_write), main.disk_write),
        }
    }
}

impl Metric<f64> {
    fn map_value(self, f: impl FnOnce(f64) -> f64) -> Self {
        match self {
            Self::Value(value) => Self::Value(f(value)),
            Self::Unavailable(reason) => Self::Unavailable(reason),
        }
    }
}

/// Adds the values that exist, or answers `main` when none does.
fn sum<T: Summable>(values: impl Iterator<Item = Metric<T>>, main: Metric<T>) -> Metric<T> {
    let mut total: Option<T> = None;
    for value in values {
        if let Metric::Value(value) = value {
            total = Some(total.map_or(value, |total| total.add(value)));
        }
    }
    total.map_or(main, Metric::Value)
}

/// A number a tree sum adds.
trait Summable: Copy {
    fn add(self, other: Self) -> Self;
}

impl Summable for u64 {
    fn add(self, other: Self) -> Self {
        self.saturating_add(other)
    }
}

impl Summable for f64 {
    fn add(self, other: Self) -> Self {
        self + other
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// One process of a tree, in tree order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeNode {
    /// The process.
    pub pid: Pid,
    /// Its parent, `None` for the main process.
    pub parent: Option<Pid>,
    /// Its distance from the main process, 0 for the main process.
    pub depth: u32,
}

/// The main process and every descendant reachable through `parents`
/// (each process with its parent), in tree order: a child right below its
/// parent, siblings by PID (spec 0028 AC-5). A PID already visited is
/// skipped, so a parent cycle ends the walk instead of looping. The caller
/// leaves out processes of another user, so their descendants are not
/// reached either.
///
/// ```
/// use stackly_domain::{Pid, process_tree};
///
/// let pid = |n| Pid::new(n).expect("a valid pid");
/// let tree = process_tree(pid(10), &[(pid(12), Some(pid(10))), (pid(11), Some(pid(10)))]);
/// let pids: Vec<u32> = tree.iter().map(|node| node.pid.get()).collect();
/// assert_eq!(pids, [10, 11, 12]);
/// ```
#[must_use]
pub fn process_tree(main: Pid, parents: &[(Pid, Option<Pid>)]) -> Vec<TreeNode> {
    let mut children: BTreeMap<Pid, Vec<Pid>> = BTreeMap::new();
    for (pid, parent) in parents {
        if let Some(parent) = parent {
            children.entry(*parent).or_default().push(*pid);
        }
    }
    for list in children.values_mut() {
        list.sort_unstable();
        list.dedup();
    }
    let mut tree = Vec::new();
    let mut visited = BTreeSet::new();
    let mut stack = vec![TreeNode {
        pid: main,
        parent: None,
        depth: 0,
    }];
    while let Some(node) = stack.pop() {
        if !visited.insert(node.pid) {
            continue;
        }
        tree.push(node);
        if let Some(kids) = children.get(&node.pid) {
            for kid in kids.iter().rev() {
                if !visited.contains(kid) {
                    stack.push(TreeNode {
                        pid: *kid,
                        parent: Some(node.pid),
                        depth: node.depth.saturating_add(1),
                    });
                }
            }
        }
    }
    tree
}

/// The name a process row shows: the file name of the verified executable
/// path, never argv or a process title (spec 0028, *Security model*).
#[must_use]
pub fn process_name(executable: &Path) -> String {
    executable
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One row of the process list of the newest sample.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProcessUsage {
    /// The process.
    pub pid: Pid,
    /// Its parent, `None` for the main process.
    pub parent: Option<Pid>,
    /// Its distance from the main process.
    pub depth: u32,
    /// The executable's file name.
    pub name: String,
    /// True for the main process.
    pub is_main: bool,
    /// CPU percent of one core.
    pub cpu: Metric<f64>,
    /// Physical footprint in bytes.
    pub memory: Metric<u64>,
}

/// One sample of an instance.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MetricsSample {
    /// 1 for the first sample of a history, then one more per sample.
    pub seq: u64,
    /// When it was taken, from the platform clock.
    pub sampled_at: Timestamp,
    /// The main process alone.
    pub main: ResourceUsage,
    /// The whole tree.
    pub tree: ResourceUsage,
}

/// Which main process a history belongs to: its PID and start time. Its
/// text form, `{pid}-{start}`, is the `historyId` clients hold as opaque
/// text.
///
/// ```
/// use stackly_domain::{HistoryId, Pid, ProcessStart};
///
/// let id = HistoryId::new(Pid::new(42)?, ProcessStart::new(1_700_000_000, 7)?);
/// assert_eq!(id.to_string(), "42-1700000000.000007");
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HistoryId {
    pid: Pid,
    start: ProcessStart,
}

impl HistoryId {
    /// The history of the main process with this PID and start time.
    #[must_use]
    pub const fn new(pid: Pid, start: ProcessStart) -> Self {
        Self { pid, start }
    }

    /// The main process's PID.
    #[must_use]
    pub const fn pid(self) -> Pid {
        self.pid
    }

    /// The main process's start time.
    #[must_use]
    pub const fn start(self) -> ProcessStart {
        self.start
    }
}

impl fmt::Display for HistoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.pid, self.start)
    }
}

/// One process of a tree as the sampler read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRead {
    /// Its place in the tree.
    pub node: TreeNode,
    /// Its start time, which with the PID tells one process from a later
    /// one with the same PID.
    pub start: ProcessStart,
    /// The executable's file name.
    pub name: String,
    /// The read.
    pub read: ProcessRead,
}

/// The samples a cursor asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplesPage<'a> {
    /// The samples after the cursor, oldest first.
    pub samples: Vec<&'a MetricsSample>,
    /// True when the client must drop what it holds: another history, no
    /// cursor, or samples between the cursor and the oldest one are gone.
    pub reset: bool,
}

/// The samples of one main process, newest last, at most `capacity` of
/// them, with the counters of the last read per process for the next
/// difference (spec 0028 AC-1).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsHistory {
    id: HistoryId,
    capacity: NonZeroUsize,
    samples: VecDeque<MetricsSample>,
    next_seq: u64,
    baselines: HashMap<(Pid, ProcessStart), CounterReading>,
    latest_processes: Vec<ProcessUsage>,
    processes_truncated: bool,
}

impl MetricsHistory {
    /// An empty history for the main process `id`.
    #[must_use]
    pub fn new(id: HistoryId, capacity: NonZeroUsize) -> Self {
        Self {
            id,
            capacity,
            samples: VecDeque::with_capacity(capacity.get()),
            next_seq: 1,
            baselines: HashMap::new(),
            latest_processes: Vec::new(),
            processes_truncated: false,
        }
    }

    /// Which main process it belongs to.
    #[must_use]
    pub const fn id(&self) -> HistoryId {
        self.id
    }

    /// The samples, oldest first.
    pub fn samples(&self) -> impl ExactSizeIterator<Item = &MetricsSample> {
        self.samples.iter()
    }

    /// The process rows of the newest sample, in tree order.
    #[must_use]
    pub fn latest_processes(&self) -> &[ProcessUsage] {
        &self.latest_processes
    }

    /// True when the newest sample had more processes than rows.
    #[must_use]
    pub const fn processes_truncated(&self) -> bool {
        self.processes_truncated
    }

    /// Records one sample from the reads of every tree member, the main
    /// process first, and returns it. The oldest sample goes when the
    /// history is full. Only processes read now keep a baseline, so a
    /// process that disappears and a reused PID both start warming up.
    /// Rows of processes that exited are left out, and at most `rows` rows
    /// are kept.
    ///
    /// # Panics
    ///
    /// Never: the history holds at least one sample right after the push.
    pub fn record(
        &mut self,
        sampled_at: Timestamp,
        members: &[MemberRead],
        rows: usize,
    ) -> &MetricsSample {
        let mut baselines = HashMap::with_capacity(members.len());
        let mut measures = Vec::with_capacity(members.len());
        for member in members {
            let key = (member.node.pid, member.start);
            let measure = measure(self.baselines.get(&key), &member.read);
            if let ProcessRead::Counters(reading) = member.read {
                baselines.insert(key, reading);
            }
            measures.push(measure);
        }
        self.baselines = baselines;

        let main_measure = members
            .iter()
            .zip(&measures)
            .find(|(member, _)| member.node.pid == self.id.pid)
            .map_or(
                ProcessMeasure::unavailable(UnavailableReason::ProcessExited),
                |(_, measure)| *measure,
            );
        let descendants: Vec<ProcessMeasure> = members
            .iter()
            .zip(&measures)
            .filter(|(member, _)| member.node.pid != self.id.pid)
            .map(|(_, measure)| *measure)
            .collect();

        let live: Vec<ProcessUsage> = members
            .iter()
            .zip(&measures)
            .filter(|(member, _)| {
                member.read != ProcessRead::Unavailable(UnavailableReason::ProcessExited)
            })
            .map(|(member, measure)| ProcessUsage {
                pid: member.node.pid,
                parent: member.node.parent,
                depth: member.node.depth,
                name: member.name.clone(),
                is_main: member.node.pid == self.id.pid,
                cpu: measure.cpu,
                memory: measure.memory,
            })
            .collect();
        self.processes_truncated = live.len() > rows;
        self.latest_processes = live;
        self.latest_processes.truncate(rows);

        if self.samples.len() >= self.capacity.get() {
            self.samples.pop_front();
        }
        self.samples.push_back(MetricsSample {
            seq: self.next_seq,
            sampled_at,
            main: ResourceUsage::main(&main_measure),
            tree: ResourceUsage::tree(&main_measure, &descendants),
        });
        self.next_seq = self.next_seq.saturating_add(1);
        self.samples
            .back()
            .expect("the history holds the sample it just pushed")
    }

    /// The samples a client cursor asks for (spec 0028 AC-11): every sample
    /// with a reset for no history ID or another one; for this one, those
    /// after `after_seq` (0 when absent), with a reset when samples between
    /// the cursor and the oldest one are gone. A cursor past the newest
    /// sample gets nothing and no reset.
    #[must_use]
    pub fn page(&self, history_id: Option<&str>, after_seq: Option<u64>) -> SamplesPage<'_> {
        let same = history_id.is_some_and(|id| id == self.id.to_string());
        if !same {
            return SamplesPage {
                samples: self.samples.iter().collect(),
                reset: true,
            };
        }
        let after = after_seq.unwrap_or(0);
        let reset = self
            .samples
            .front()
            .is_some_and(|oldest| after.saturating_add(1) < oldest.seq);
        SamplesPage {
            samples: self.samples.iter().filter(|s| s.seq > after).collect(),
            reset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(n: u32) -> Pid {
        Pid::new(n).unwrap()
    }

    fn start(n: u64) -> ProcessStart {
        ProcessStart::new(n, 0).unwrap()
    }

    fn reading(cpu_ms: u64, footprint: u64, read: u64, written: u64, at_ms: u64) -> CounterReading {
        CounterReading {
            counters: ProcessCounters {
                cpu_user: Duration::from_millis(cpu_ms / 2),
                cpu_system: Duration::from_millis(cpu_ms - cpu_ms / 2),
                footprint,
                disk_read: read,
                disk_write: written,
            },
            at: Duration::from_millis(at_ms),
        }
    }

    fn member(n: u32, parent: Option<u32>, depth: u32, read: ProcessRead) -> MemberRead {
        MemberRead {
            node: TreeNode {
                pid: pid(n),
                parent: parent.map(pid),
                depth,
            },
            start: start(u64::from(n)),
            name: format!("proc{n}"),
            read,
        }
    }

    fn history() -> MetricsHistory {
        MetricsHistory::new(
            HistoryId::new(pid(10), start(10)),
            NonZeroUsize::new(3).unwrap(),
        )
    }

    #[test]
    fn cpu_is_percent_of_one_core_rounded_to_one_decimal() {
        let rates = rates(&reading(0, 0, 0, 0, 0), &reading(1_234, 0, 0, 0, 5_000)).unwrap();
        assert_eq!(rates.cpu, 24.7);
        let busy = super::rates(&reading(0, 0, 0, 0, 0), &reading(15_000, 0, 0, 0, 5_000));
        assert_eq!(busy.unwrap().cpu, 300.0);
    }

    #[test]
    fn disk_io_is_bytes_per_second_rounded_down() {
        let rates = rates(&reading(0, 0, 0, 0, 0), &reading(0, 0, 10_001, 999, 2_000)).unwrap();
        assert_eq!(rates.disk_read, 5_000);
        assert_eq!(rates.disk_write, 499);
    }

    #[test]
    fn no_rate_comes_from_no_elapsed_time_or_a_counter_that_went_back() {
        assert_eq!(
            rates(&reading(0, 0, 0, 0, 5), &reading(10, 0, 0, 0, 5)),
            None
        );
        assert_eq!(
            rates(&reading(10, 0, 0, 0, 0), &reading(5, 0, 0, 0, 5)),
            None
        );
        assert_eq!(
            rates(&reading(0, 0, 9, 0, 0), &reading(0, 0, 1, 0, 5)),
            None
        );
    }

    #[test]
    fn a_first_read_has_memory_but_cpu_and_disk_are_warming_up() {
        let m = measure(None, &ProcessRead::Counters(reading(5, 4096, 0, 0, 0)));
        assert_eq!(m.memory, Metric::Value(4096));
        assert_eq!(m.cpu, Metric::Unavailable(UnavailableReason::WarmingUp));
        assert_eq!(
            m.disk_read,
            Metric::Unavailable(UnavailableReason::WarmingUp)
        );
        assert!(!m.measured);
    }

    #[test]
    fn a_refused_or_vanished_read_is_unavailable_with_its_reason_never_zero() {
        for reason in [
            UnavailableReason::NotPermitted,
            UnavailableReason::ProcessExited,
        ] {
            let m = measure(
                Some(&reading(0, 1, 0, 0, 0)),
                &ProcessRead::Unavailable(reason),
            );
            assert_eq!(m, ProcessMeasure::unavailable(reason));
            assert_eq!(m.memory.value(), None);
        }
    }

    #[test]
    fn a_tree_sums_only_the_processes_that_have_a_value() {
        let main = measure(
            Some(&reading(0, 0, 0, 0, 0)),
            &ProcessRead::Counters(reading(500, 100, 0, 0, 5_000)),
        );
        let warming = measure(None, &ProcessRead::Counters(reading(0, 50, 0, 0, 5_000)));
        let denied = ProcessMeasure::unavailable(UnavailableReason::NotPermitted);
        let tree = ResourceUsage::tree(&main, &[warming, denied]);
        assert_eq!(tree.process_count, 3);
        assert_eq!(tree.measured_count, 1);
        assert_eq!(tree.cpu, Metric::Value(10.0));
        assert_eq!(tree.memory, Metric::Value(150));
        let main_only = ResourceUsage::main(&main);
        assert_eq!((main_only.process_count, main_only.measured_count), (1, 1));
        assert_eq!(main_only.scope, UsageScope::Main);
    }

    #[test]
    fn a_tree_with_nothing_measured_takes_the_reason_of_the_main_process() {
        let main = ProcessMeasure::unavailable(UnavailableReason::ProcessExited);
        let child = ProcessMeasure::unavailable(UnavailableReason::NotPermitted);
        let tree = ResourceUsage::tree(&main, &[child]);
        assert_eq!(
            tree.cpu,
            Metric::Unavailable(UnavailableReason::ProcessExited)
        );
        assert_eq!(
            tree.memory,
            Metric::Unavailable(UnavailableReason::ProcessExited)
        );
        assert_eq!(tree.measured_count, 0);
    }

    #[test]
    fn a_tree_orders_children_below_their_parent_with_siblings_by_pid() {
        let tree = process_tree(
            pid(10),
            &[
                (pid(30), Some(pid(10))),
                (pid(20), Some(pid(10))),
                (pid(21), Some(pid(20))),
                (pid(99), Some(pid(1))),
                (pid(10), Some(pid(1))),
            ],
        );
        let shape: Vec<(u32, Option<u32>, u32)> = tree
            .iter()
            .map(|n| (n.pid.get(), n.parent.map(Pid::get), n.depth))
            .collect();
        assert_eq!(
            shape,
            [
                (10, None, 0),
                (20, Some(10), 1),
                (21, Some(20), 2),
                (30, Some(10), 1)
            ]
        );
    }

    #[test]
    fn a_parent_cycle_ends_the_walk() {
        let tree = process_tree(
            pid(10),
            &[(pid(11), Some(pid(10))), (pid(10), Some(pid(11)))],
        );
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn the_process_name_is_the_executable_file_name() {
        assert_eq!(process_name(Path::new("/opt/pg/bin/postgres")), "postgres");
    }

    #[test]
    fn the_history_numbers_samples_from_one_and_drops_the_oldest_past_capacity() {
        let mut history = history();
        for (i, at) in [0_u64, 5_000, 10_000, 15_000].iter().enumerate() {
            let read = ProcessRead::Counters(reading(100 * i as u64, 10, 0, 0, *at));
            history.record(Timestamp::UNIX_EPOCH, &[member(10, None, 0, read)], 200);
        }
        let seqs: Vec<u64> = history.samples().map(|s| s.seq).collect();
        assert_eq!(seqs, [2, 3, 4]);
        let first = history.samples().next().unwrap();
        assert_eq!(first.main.cpu, Metric::Value(2.0));
    }

    #[test]
    fn a_reused_pid_starts_warming_up_again() {
        let mut history = history();
        let read = ProcessRead::Counters(reading(0, 10, 0, 0, 0));
        history.record(
            Timestamp::UNIX_EPOCH,
            &[member(10, None, 0, read), member(11, Some(10), 1, read)],
            200,
        );
        let mut reused = member(
            11,
            Some(10),
            1,
            ProcessRead::Counters(reading(50, 10, 0, 0, 5_000)),
        );
        reused.start = start(999);
        let main = member(
            10,
            None,
            0,
            ProcessRead::Counters(reading(50, 10, 0, 0, 5_000)),
        );
        let sample = history
            .record(Timestamp::UNIX_EPOCH, &[main, reused], 200)
            .clone();
        assert_eq!(sample.tree.measured_count, 1);
        assert_eq!(
            history.latest_processes()[1].cpu,
            Metric::Unavailable(UnavailableReason::WarmingUp)
        );
    }

    #[test]
    fn exited_processes_leave_the_rows_and_a_gone_main_is_process_exited() {
        let mut history = history();
        let exited = ProcessRead::Unavailable(UnavailableReason::ProcessExited);
        let child = ProcessRead::Counters(reading(0, 10, 0, 0, 0));
        let sample = history
            .record(
                Timestamp::UNIX_EPOCH,
                &[member(10, None, 0, exited), member(11, Some(10), 1, child)],
                200,
            )
            .clone();
        assert_eq!(
            sample.main.memory,
            Metric::Unavailable(UnavailableReason::ProcessExited)
        );
        assert_eq!(sample.tree.memory, Metric::Value(10));
        let rows: Vec<u32> = history
            .latest_processes()
            .iter()
            .map(|r| r.pid.get())
            .collect();
        assert_eq!(rows, [11]);
    }

    #[test]
    fn rows_stop_at_the_limit_and_say_so() {
        let mut history = history();
        let read = ProcessRead::Counters(reading(0, 10, 0, 0, 0));
        let members = [
            member(10, None, 0, read),
            member(11, Some(10), 1, read),
            member(12, Some(10), 1, read),
        ];
        let sample = history.record(Timestamp::UNIX_EPOCH, &members, 2).clone();
        assert_eq!(history.latest_processes().len(), 2);
        assert!(history.processes_truncated());
        assert_eq!(sample.tree.process_count, 3);
        assert!(history.latest_processes()[0].is_main);
    }

    #[test]
    fn the_cursor_follows_the_history_rules() {
        let mut history = history();
        for at in [0_u64, 5_000, 10_000, 15_000, 20_000] {
            let read = ProcessRead::Counters(reading(0, 10, 0, 0, at));
            history.record(Timestamp::UNIX_EPOCH, &[member(10, None, 0, read)], 200);
        }
        let id = history.id().to_string();
        let seqs = |page: SamplesPage<'_>| -> (Vec<u64>, bool) {
            (page.samples.iter().map(|s| s.seq).collect(), page.reset)
        };
        assert_eq!(seqs(history.page(None, None)), (vec![3, 4, 5], true));
        assert_eq!(
            seqs(history.page(Some("other"), Some(4))),
            (vec![3, 4, 5], true)
        );
        assert_eq!(seqs(history.page(Some(&id), Some(4))), (vec![5], false));
        assert_eq!(
            seqs(history.page(Some(&id), Some(2))),
            (vec![3, 4, 5], false)
        );
        assert_eq!(
            seqs(history.page(Some(&id), Some(1))),
            (vec![3, 4, 5], true)
        );
        assert_eq!(seqs(history.page(Some(&id), Some(9))), (vec![], false));
    }
}
