//! The metrics sampler over `FakeProcessSupervision` (spec 0028 AC-1, AC-3,
//! AC-5): trees, other users, refused and vanished processes, reused PIDs,
//! the facts cache, histories that follow the main process, and the row
//! limit. Time is paused, so every CPU rate is exact.
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stackly_core::metrics::{MainProcesses, MetricsStore, RunningMain, Sampler, SamplerConfig};
use stackly_domain::{
    InstanceId, Metric, MetricsSample, Os, Pid, ProcessCounters, ProcessStart, UnavailableReason,
    UsageScope,
};
use stackly_platform::{PlatformCapability, ProcessError, ProcessFacts, UserId};
use stackly_testkit::{FakePlatform, ProcessOp, TestRoot};

const INTERVAL: Duration = Duration::from_secs(5);

fn pid(n: u32) -> Pid {
    Pid::new(n).expect("a valid pid")
}

fn start(n: u64) -> ProcessStart {
    ProcessStart::new(1_700_000_000 + n, 0).expect("a valid start")
}

/// A source whose list a test changes between ticks.
#[derive(Clone, Default)]
struct Mains(Arc<Mutex<Vec<RunningMain>>>);

impl Mains {
    fn set(&self, mains: Vec<RunningMain>) {
        *self.0.lock().expect("the mains lock") = mains;
    }
}

impl MainProcesses for Mains {
    fn running(&self) -> Vec<RunningMain> {
        self.0.lock().expect("the mains lock").clone()
    }
}

struct Rig {
    _root: TestRoot,
    fake: FakePlatform,
    mains: Mains,
    store: MetricsStore,
    sampler: Sampler,
    instance: InstanceId,
}

impl Rig {
    fn new() -> Self {
        Self::with(SamplerConfig::default())
    }

    fn with(config: SamplerConfig) -> Self {
        let root = TestRoot::new().expect("a test root");
        let fake = FakePlatform::new(&root);
        let mains = Mains::default();
        let store = MetricsStore::new();
        let sampler = Sampler::new(
            Arc::new(fake.platform()),
            Arc::new(mains.clone()),
            store.clone(),
            config,
        );
        Self {
            _root: root,
            fake,
            mains,
            store,
            sampler,
            instance: InstanceId::generate(),
        }
    }

    fn user(&self) -> UserId {
        self.fake.processes().user()
    }

    /// Adds a process of the daemon's user and gives it `footprint` bytes.
    fn add(&self, n: u32, parent: Option<u32>, footprint: u64) {
        self.add_for(n, parent, footprint, self.user());
    }

    fn add_for(&self, n: u32, parent: Option<u32>, footprint: u64, uid: UserId) {
        self.fake.processes().insert(ProcessFacts {
            pid: pid(n),
            parent: parent.map(pid),
            group: pid(n),
            session: pid(n),
            uid,
            start: start(u64::from(n)),
            executable: PathBuf::from(format!("/opt/test/bin/proc{n}")),
            stopped: false,
        });
        self.fake.processes().set_counters(
            pid(n),
            ProcessCounters {
                footprint,
                ..ProcessCounters::default()
            },
        );
    }

    fn run_main(&self, n: u32) {
        self.mains.set(vec![RunningMain::new(
            self.instance,
            pid(n),
            start(u64::from(n)),
        )]);
    }

    fn busy(&self, n: u32, cpu: Duration) {
        self.fake
            .processes()
            .update_counters(pid(n), |counters| counters.cpu_user += cpu);
    }

    async fn tick(&mut self) {
        self.sampler.sample_once().await;
    }

    /// Samples, waits one interval, and samples again, so rates exist.
    async fn two_ticks(&mut self) -> MetricsSample {
        self.tick().await;
        tokio::time::advance(INTERVAL).await;
        self.tick().await;
        self.newest()
    }

    fn newest(&self) -> MetricsSample {
        self.store
            .with_history(self.instance, |history| {
                history
                    .expect("the instance has a history")
                    .samples()
                    .last()
                    .cloned()
            })
            .expect("the history has a sample")
    }

    fn rows(&self) -> Vec<(u32, Option<u32>, u32, String, bool)> {
        self.store.with_history(self.instance, |history| {
            history
                .expect("the instance has a history")
                .latest_processes()
                .iter()
                .map(|row| {
                    (
                        row.pid.get(),
                        row.parent.map(Pid::get),
                        row.depth,
                        row.name.clone(),
                        row.is_main,
                    )
                })
                .collect()
        })
    }
}

fn unavailable<T>(reason: UnavailableReason) -> Metric<T> {
    Metric::Unavailable(reason)
}

#[tokio::test(start_paused = true)]
async fn the_first_sample_has_memory_and_warming_up_rates_then_the_second_has_cpu() {
    let mut rig = Rig::new();
    rig.add(10, None, 4_096);
    rig.run_main(10);

    rig.tick().await;
    let first = rig.newest();
    assert_eq!(first.seq, 1);
    assert_eq!(first.main.memory, Metric::Value(4_096));
    assert_eq!(first.main.cpu, unavailable(UnavailableReason::WarmingUp));
    assert_eq!(
        first.main.disk_read,
        unavailable(UnavailableReason::WarmingUp)
    );
    assert_eq!(first.main.measured_count, 0);

    rig.busy(10, Duration::from_millis(2_500));
    rig.fake
        .processes()
        .update_counters(pid(10), |counters| counters.disk_write += 10_000);
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    let second = rig.newest();
    assert_eq!(second.seq, 2);
    assert_eq!(second.main.cpu, Metric::Value(50.0));
    assert_eq!(second.main.disk_write, Metric::Value(2_000));
    assert_eq!(second.main.disk_read, Metric::Value(0));
    assert_eq!(second.main.measured_count, 1);
}

#[tokio::test(start_paused = true)]
async fn the_tree_sums_the_main_process_and_every_descendant_of_the_same_user() {
    let mut rig = Rig::new();
    rig.add(10, None, 1_000);
    rig.add(11, Some(10), 200);
    rig.add(12, Some(10), 300);
    rig.add(13, Some(11), 40);
    // Not in the tree: another branch, and a child of another user with a
    // child of the daemon's user below it.
    rig.add(99, Some(1), 7_777);
    rig.add_for(14, Some(10), 5_000, UserId::Unix(0));
    rig.add(15, Some(14), 6_000);
    rig.run_main(10);
    for n in [10, 11, 12, 13] {
        rig.busy(n, Duration::ZERO);
    }

    rig.tick().await;
    for n in [10, 11, 12, 13] {
        rig.busy(n, Duration::from_millis(500));
    }
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    let sample = rig.newest();

    assert_eq!(sample.main.scope, UsageScope::Main);
    assert_eq!(sample.main.memory, Metric::Value(1_000));
    assert_eq!(sample.main.cpu, Metric::Value(10.0));
    assert_eq!(sample.tree.scope, UsageScope::Tree);
    assert_eq!(sample.tree.process_count, 4);
    assert_eq!(sample.tree.measured_count, 4);
    assert_eq!(sample.tree.memory, Metric::Value(1_540));
    assert_eq!(sample.tree.cpu, Metric::Value(40.0));
    let name = |n: u32| format!("proc{n}");
    assert_eq!(
        rig.rows(),
        [
            (10, None, 0, name(10), true),
            (11, Some(10), 1, name(11), false),
            (13, Some(11), 2, name(13), false),
            (12, Some(10), 1, name(12), false),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_tree_cpu_can_pass_one_hundred_percent() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.add(11, Some(10), 1);
    rig.run_main(10);
    rig.tick().await;
    rig.busy(10, Duration::from_secs(4));
    rig.busy(11, Duration::from_secs(4));
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;

    assert_eq!(rig.newest().tree.cpu, Metric::Value(160.0));
    assert_eq!(rig.newest().main.cpu, Metric::Value(80.0));
}

#[tokio::test(start_paused = true)]
async fn a_parent_cycle_ends_the_walk_without_looping() {
    let mut rig = Rig::new();
    rig.add(10, Some(11), 1);
    rig.add(11, Some(10), 1);
    rig.run_main(10);
    rig.tick().await;

    assert_eq!(rig.newest().tree.process_count, 2);
}

#[tokio::test(start_paused = true)]
async fn a_refused_main_process_is_not_permitted_and_never_zero() {
    let mut rig = Rig::new();
    rig.add(10, None, 1_000);
    rig.fake.processes().deny_counters(pid(10));
    rig.run_main(10);

    let sample = rig.two_ticks().await;
    let denied = UnavailableReason::NotPermitted;
    assert_eq!(sample.main.cpu, unavailable(denied));
    assert_eq!(sample.main.memory, unavailable(denied));
    assert_eq!(sample.main.disk_read, unavailable(denied));
    assert_eq!(sample.main.disk_write, unavailable(denied));
    assert_eq!(sample.main.measured_count, 0);
    // Nothing in the tree was measured, so the tree takes the main reason.
    assert_eq!(sample.tree.process_count, 1);
    assert_eq!(sample.tree.memory, unavailable(denied));
    assert_eq!(sample.tree.cpu, unavailable(denied));
}

#[tokio::test(start_paused = true)]
async fn a_refused_child_leaves_the_tree_sum_of_the_rest_and_a_lower_measured_count() {
    let mut rig = Rig::new();
    rig.add(10, None, 100);
    rig.add(11, Some(10), 50);
    rig.fake.processes().deny_counters(pid(11));
    rig.run_main(10);

    let sample = rig.two_ticks().await;
    assert_eq!(sample.tree.process_count, 2);
    assert_eq!(sample.tree.measured_count, 1);
    assert_eq!(sample.tree.memory, Metric::Value(100));
    let child = rig.store.with_history(rig.instance, |h| {
        h.expect("a history").latest_processes()[1].memory
    });
    assert_eq!(child, unavailable(UnavailableReason::NotPermitted));
}

#[tokio::test(start_paused = true)]
async fn a_main_process_that_vanishes_mid_sample_is_process_exited() {
    let mut rig = Rig::new();
    rig.add(10, None, 100);
    rig.add(11, Some(10), 50);
    rig.run_main(10);
    rig.tick().await;

    rig.fake
        .processes()
        .fail_next(ProcessOp::Counters, ProcessError::NotFound { pid: pid(10) });
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;

    let sample = rig.newest();
    assert_eq!(
        sample.main.memory,
        unavailable(UnavailableReason::ProcessExited)
    );
    assert_eq!(sample.tree.memory, Metric::Value(50));
    // The row of a process that exited is gone; the sample did not fail.
    assert_eq!(rig.rows().iter().map(|row| row.0).collect::<Vec<_>>(), [11]);
}

#[tokio::test(start_paused = true)]
async fn a_main_process_missing_from_the_process_list_is_process_exited() {
    let mut rig = Rig::new();
    rig.add(10, None, 100);
    rig.run_main(10);
    rig.tick().await;
    rig.fake.processes().exit(pid(10), None);
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;

    let sample = rig.newest();
    assert_eq!(
        sample.main.memory,
        unavailable(UnavailableReason::ProcessExited)
    );
    assert_eq!(
        sample.tree.cpu,
        unavailable(UnavailableReason::ProcessExited)
    );
}

#[tokio::test(start_paused = true)]
async fn a_child_that_exits_between_samples_leaves_the_rows_and_the_tree() {
    let mut rig = Rig::new();
    rig.add(10, None, 100);
    rig.add(11, Some(10), 50);
    rig.run_main(10);
    rig.tick().await;
    assert_eq!(rig.newest().tree.process_count, 2);

    rig.fake.processes().exit(pid(11), None);
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    assert_eq!(rig.newest().tree.process_count, 1);
    assert_eq!(rig.newest().tree.memory, Metric::Value(100));
}

#[tokio::test(start_paused = true)]
async fn a_reused_child_pid_is_a_new_process_that_starts_warming_up() {
    let mut rig = Rig::new();
    rig.add(10, None, 100);
    rig.add(11, Some(10), 50);
    rig.run_main(10);
    rig.tick().await;
    rig.busy(10, Duration::from_millis(500));
    rig.busy(11, Duration::from_millis(500));
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    assert_eq!(rig.newest().tree.measured_count, 2);
    let inspects = rig.fake.processes().count(ProcessOp::Inspect);

    // Pid 11 now belongs to another process of the same parent.
    rig.fake.processes().exit(pid(11), None);
    rig.fake.processes().insert(ProcessFacts {
        pid: pid(11),
        parent: Some(pid(10)),
        group: pid(11),
        session: pid(11),
        uid: rig.user(),
        start: start(777),
        executable: PathBuf::from("/opt/test/bin/other"),
        stopped: false,
    });
    rig.busy(10, Duration::from_millis(500));
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;

    let sample = rig.newest();
    assert_eq!(sample.tree.process_count, 2);
    assert_eq!(
        sample.tree.measured_count, 1,
        "the new process has no baseline"
    );
    let rows = rig.rows();
    assert_eq!(rows[1].3, "other");
    assert_eq!(
        rig.store.with_history(rig.instance, |h| {
            h.expect("a history").latest_processes()[1].cpu
        }),
        unavailable(UnavailableReason::WarmingUp)
    );
    assert_eq!(
        rig.fake.processes().count(ProcessOp::Inspect),
        inspects + 1,
        "only the reused pid is inspected again"
    );
}

#[tokio::test(start_paused = true)]
async fn inspect_runs_once_per_process_and_not_once_per_tick() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.add(11, Some(10), 1);
    rig.add(50, Some(1), 1);
    rig.run_main(10);

    rig.tick().await;
    let after_first = rig.fake.processes().count(ProcessOp::Inspect);
    assert_eq!(after_first, 3);
    for _ in 0..3 {
        tokio::time::advance(INTERVAL).await;
        rig.tick().await;
    }
    assert_eq!(rig.fake.processes().count(ProcessOp::Inspect), after_first);
    assert_eq!(rig.fake.processes().count(ProcessOp::ListPids), 4);

    rig.add(12, Some(10), 1);
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    assert_eq!(
        rig.fake.processes().count(ProcessOp::Inspect),
        after_first + 1
    );
    assert_eq!(rig.newest().tree.process_count, 3);
}

#[tokio::test(start_paused = true)]
async fn a_process_whose_inspect_is_refused_is_not_inspected_again_while_it_lives() {
    let mut rig = Rig::new();
    rig.add(5, Some(1), 1);
    rig.add(10, None, 1);
    rig.add(11, Some(10), 1);
    rig.run_main(10);
    rig.fake.processes().fail_next(
        ProcessOp::Inspect,
        ProcessError::PermissionDenied { pid: pid(5) },
    );

    rig.tick().await;
    let after_first = rig.fake.processes().count(ProcessOp::Inspect);
    assert_eq!(after_first, 3);
    for _ in 0..3 {
        tokio::time::advance(INTERVAL).await;
        rig.tick().await;
    }

    assert_eq!(
        rig.fake.processes().count(ProcessOp::Inspect),
        after_first,
        "the refused pid is remembered, not asked every tick"
    );
    assert_eq!(rig.newest().tree.process_count, 2);
}

#[tokio::test(start_paused = true)]
async fn a_new_main_process_starts_a_new_history_numbered_from_one() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.add(20, None, 2);
    rig.run_main(10);
    rig.tick().await;
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    let old_id = rig
        .store
        .with_history(rig.instance, |h| h.expect("a history").id().to_string());
    assert_eq!(rig.newest().seq, 2);

    rig.run_main(20);
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    let new_id = rig
        .store
        .with_history(rig.instance, |h| h.expect("a history").id().to_string());
    assert_ne!(old_id, new_id);
    let sample = rig.newest();
    assert_eq!(sample.seq, 1);
    assert_eq!(sample.main.memory, Metric::Value(2));
    assert_eq!(sample.main.cpu, unavailable(UnavailableReason::WarmingUp));
}

#[tokio::test(start_paused = true)]
async fn the_history_goes_when_the_instance_stops_running() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.run_main(10);
    rig.tick().await;
    assert_eq!(rig.store.instances(), [rig.instance]);

    rig.mains.set(Vec::new());
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    assert!(rig.store.instances().is_empty());
    assert!(rig.store.with_history(rig.instance, |h| h.is_none()));
    // With nothing running the sampler does not even list the processes.
    assert_eq!(rig.fake.processes().count(ProcessOp::ListPids), 1);
}

#[tokio::test(start_paused = true)]
async fn the_sixty_first_sample_drops_the_oldest() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.run_main(10);
    for _ in 0..61 {
        rig.tick().await;
        tokio::time::advance(INTERVAL).await;
    }
    let seqs: Vec<u64> = rig.store.with_history(rig.instance, |h| {
        h.expect("a history").samples().map(|s| s.seq).collect()
    });
    assert_eq!(seqs.len(), 60);
    assert_eq!(seqs.first(), Some(&2));
    assert_eq!(seqs.last(), Some(&61));
}

#[tokio::test(start_paused = true)]
async fn rows_stop_at_the_limit_while_the_tree_sum_counts_every_process() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    for n in 0..205 {
        rig.add(1_000 + n, Some(10), 2);
    }
    rig.run_main(10);
    rig.tick().await;

    let sample = rig.newest();
    assert_eq!(sample.tree.process_count, 206);
    assert_eq!(sample.tree.memory, Metric::Value(1 + 205 * 2));
    assert_eq!(rig.rows().len(), 200);
    assert!(rig.store.with_history(rig.instance, |h| {
        h.expect("a history").processes_truncated()
    }));
    assert!(rig.rows()[0].4, "the main process leads the rows");
}

#[tokio::test(start_paused = true)]
async fn two_running_instances_get_their_own_histories() {
    let mut rig = Rig::new();
    let other = InstanceId::generate();
    rig.add(10, None, 1);
    rig.add(20, None, 2);
    rig.mains.set(vec![
        RunningMain::new(rig.instance, pid(10), start(10)),
        RunningMain::new(other, pid(20), start(20)),
    ]);
    rig.tick().await;

    assert_eq!(rig.store.instances().len(), 2);
    assert_eq!(rig.newest().main.memory, Metric::Value(1));
    let memory = rig.store.with_history(other, |h| {
        h.expect("a history")
            .samples()
            .last()
            .expect("a sample")
            .main
            .memory
    });
    assert_eq!(memory, Metric::Value(2));
}

#[tokio::test(start_paused = true)]
async fn a_platform_without_process_metrics_gives_not_supported_never_zero() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.run_main(10);
    rig.fake.processes().fail_always(
        ProcessOp::ListPids,
        ProcessError::NotSupported {
            capability: PlatformCapability::ProcessSupervision,
            os: Os::Windows,
        },
    );
    rig.tick().await;

    let sample = rig.newest();
    let reason = UnavailableReason::NotSupported;
    assert_eq!(sample.main.memory, unavailable(reason));
    assert_eq!(sample.main.cpu, unavailable(reason));
    assert_eq!(sample.tree.disk_read, unavailable(reason));
}

#[tokio::test(start_paused = true)]
async fn a_process_list_that_cannot_be_read_records_no_sample() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.run_main(10);
    rig.tick().await;
    rig.fake.processes().fail_next(
        ProcessOp::ListPids,
        ProcessError::Io {
            op: "proc_listallpids",
            path: None,
            kind: std::io::ErrorKind::Other,
            message: "boom".to_owned(),
        },
    );
    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    assert_eq!(rig.newest().seq, 1);

    tokio::time::advance(INTERVAL).await;
    rig.tick().await;
    assert_eq!(rig.newest().seq, 2);
}

#[tokio::test(start_paused = true)]
async fn the_task_samples_on_every_interval_without_any_client() {
    let mut rig = Rig::new();
    rig.add(10, None, 1);
    rig.run_main(10);
    let store = rig.store.clone();
    let instance = rig.instance;
    let task = std::mem::replace(
        &mut rig.sampler,
        Sampler::new(
            Arc::new(rig.fake.platform()),
            Arc::new(rig.mains.clone()),
            MetricsStore::new(),
            SamplerConfig::default(),
        ),
    )
    .start();

    // Ticks at 0 s, 5 s, and 10 s.
    tokio::time::sleep(Duration::from_secs(11)).await;
    let count = store.with_history(instance, |h| h.expect("a history").samples().len());
    assert_eq!(count, 3);

    task.stop();
    tokio::time::sleep(Duration::from_secs(20)).await;
    let count = store.with_history(instance, |h| h.expect("a history").samples().len());
    assert_eq!(count, 3, "a stopped sampler takes no more samples");
}

#[tokio::test(start_paused = true)]
async fn a_sampler_config_can_shorten_the_interval_capacity_and_rows() {
    let config = SamplerConfig::default()
        .with_interval(Duration::from_secs(1))
        .with_history_capacity(std::num::NonZeroUsize::new(2).expect("two"))
        .with_process_rows(1);
    let mut rig = Rig::with(config);
    rig.add(10, None, 1);
    rig.add(11, Some(10), 1);
    rig.run_main(10);
    for _ in 0..3 {
        rig.tick().await;
    }
    assert_eq!(
        rig.store
            .with_history(rig.instance, |h| h.expect("a history").samples().len()),
        2
    );
    assert_eq!(rig.rows().len(), 1);
}
