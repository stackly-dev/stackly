//! The bounded write queue and the one writer thread of the durable log
//! store (spec 0027, *Antrian tulis*, *Thread writer*).
//!
//! [`DurableLog::accept`] never blocks: it queues a batch or answers
//! `Busy`, so the bytes stay in the spool. The thread `stackly-logs-writer`
//! serves the instances in turn, one queued item at a time. An instance
//! whose store cannot be opened or written is parked until its retry time
//! and the thread goes on with the others; its failed item stays at the
//! head of its queue, so the queue fills and the sink answers `Busy`.
//!
//! After each item it publishes the store's `head` through a
//! [`tokio::sync::watch`] per instance, which followers wait on.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use stackly_domain::{InstanceId, ProcessStart};
use stackly_platform::{OutputStream, Platform, RootKind};
use tokio::sync::watch;

use super::DurableLogConfig;
use super::level::level_of;
use super::record::{LineRecord, Record, RecordKind, RecordSource};
use super::store::{LogStoreId, OpenStore, StoreFailure};
use crate::PreparedLayout;
use crate::supervisor::{LogBatch, RunEvent, SinkOutcome};

/// The fixed cost of one queued line beyond its text, for the byte limits.
const LINE_OVERHEAD: usize = 64;

/// What followers of one instance see: the store and its newest `seq`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogHead {
    /// The store, once the writer opened it in this daemon run.
    pub store: Option<LogStoreId>,
    /// The newest `seq` written.
    pub head: Option<u64>,
    /// The instance was deleted; its follows end.
    pub removed: bool,
}

/// The `instance_logs` subsystem of `daemon.get_health`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogsHealth {
    /// A write is failing now, or failed within the recovery time.
    pub degraded: bool,
    /// The reason of a failing instance, else of the last failure.
    pub reason: Option<StoreFailure>,
    /// How many instances fail to write now.
    pub failing: u64,
    /// The `truncated` records written since the daemon started.
    pub truncations: u64,
    /// The bytes those records stand for.
    pub bytes_truncated: u64,
}

/// One queued piece of work.
enum Item {
    Batch {
        batch: LogBatch,
        cost: usize,
        /// The records once the batch was turned into them, with their
        /// `seq` taken, and how many are written.
        prepared: Option<(Vec<Record>, usize)>,
    },
    Run {
        event: RunEvent,
        /// The `seq` once taken, and the streams written.
        seq: Option<u64>,
        done: [bool; 2],
    },
}

#[derive(Default)]
struct Queue {
    items: VecDeque<Item>,
    bytes: usize,
    run_events: usize,
    retry_at: Option<Timestamp>,
}

#[derive(Default)]
struct State {
    queues: HashMap<InstanceId, Queue>,
    order: VecDeque<InstanceId>,
    total: usize,
    busy: Option<InstanceId>,
    removed: HashSet<InstanceId>,
    to_close: Vec<InstanceId>,
    shutdown: bool,
    failing: HashMap<InstanceId, StoreFailure>,
    last_failure: Option<(Timestamp, StoreFailure)>,
    truncations: u64,
    bytes_truncated: u64,
}

struct Shared {
    platform: Arc<Platform>,
    layout: Arc<PreparedLayout>,
    config: DurableLogConfig,
    state: Mutex<State>,
    wake: Condvar,
    heads: Mutex<HashMap<InstanceId, watch::Sender<LogHead>>>,
}

/// The durable log store of every instance: the queue, the writer thread,
/// and the heads. Clones share one store.
#[derive(Clone)]
pub struct DurableLog {
    shared: Arc<Shared>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl DurableLog {
    /// Starts the writer thread over the instance log folders of `layout`.
    ///
    /// # Errors
    ///
    /// Returns the IO error when the thread cannot be started.
    pub fn start(
        platform: Arc<Platform>,
        layout: Arc<PreparedLayout>,
        config: DurableLogConfig,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            platform,
            layout,
            config,
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            heads: Mutex::new(HashMap::new()),
        });
        let worker = Arc::clone(&shared);
        let handle = std::thread::Builder::new()
            .name("stackly-logs-writer".to_owned())
            .spawn(move || run(&worker))?;
        Ok(Self {
            shared,
            thread: Arc::new(Mutex::new(Some(handle))),
        })
    }

    /// Returns the platform the store reads and writes through.
    #[must_use]
    pub fn platform(&self) -> &Platform {
        &self.shared.platform
    }

    /// Returns the limits.
    #[must_use]
    pub fn config(&self) -> DurableLogConfig {
        self.shared.config
    }

    /// Returns the store folder of `id`, under the canonical logs root.
    #[must_use]
    pub fn dir(&self, id: InstanceId) -> PathBuf {
        store_dir(&self.shared.layout, id)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues a batch of `id`, or answers `Busy` when the instance's queue
    /// is not empty and the batch would take it over its limit, or all
    /// queues over theirs (AC-3). It never blocks.
    pub fn accept(&self, id: InstanceId, batch: LogBatch) -> SinkOutcome {
        let cost = batch.text_bytes() + LINE_OVERHEAD * (batch.lines.len() + 1);
        let mut state = self.lock();
        if state.removed.contains(&id) || state.shutdown {
            return SinkOutcome::Accepted;
        }
        let config = self.shared.config;
        let total = state.total;
        let queue = state.queues.entry(id).or_default();
        if queue.bytes > 0
            && (queue.bytes + cost > config.queue_max || total + cost > config.queue_total)
        {
            return SinkOutcome::Busy;
        }
        queue.bytes += cost;
        queue.items.push_back(Item::Batch {
            batch,
            cost,
            prepared: None,
        });
        state.total += cost;
        if !state.order.contains(&id) {
            state.order.push_back(id);
        }
        drop(state);
        self.shared.wake.notify_all();
        SinkOutcome::Accepted
    }

    /// Queues a run event of `id`. Past the limit, an `Exited` replaces the
    /// `Exited` queued last when no `Started` came after it, and anything
    /// else is dropped, so a crash loop on a failing disk stays bounded.
    pub fn run_event(&self, id: InstanceId, event: RunEvent) {
        let mut state = self.lock();
        if state.removed.contains(&id) || state.shutdown {
            return;
        }
        let max = self.shared.config.run_events_max;
        let queue = state.queues.entry(id).or_default();
        if queue.run_events >= max {
            if let RunEvent::Exited { .. } = event {
                let last = queue.items.iter_mut().rev().find_map(|item| match item {
                    Item::Run {
                        event: last,
                        seq: None,
                        ..
                    } => Some(last),
                    _ => None,
                });
                if let Some(last @ RunEvent::Exited { .. }) = last {
                    *last = event;
                    return;
                }
            }
            tracing::debug!(instance = %id, "a run event was dropped because the queue is full");
            return;
        }
        queue.run_events += 1;
        queue.items.push_back(Item::Run {
            event,
            seq: None,
            done: [false; 2],
        });
        if !state.order.contains(&id) {
            state.order.push_back(id);
        }
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Returns a receiver of the head of `id`.
    #[must_use]
    pub fn subscribe(&self, id: InstanceId) -> watch::Receiver<LogHead> {
        self.shared
            .heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(id)
            .or_insert_with(|| watch::channel(LogHead::default()).0)
            .subscribe()
    }

    /// Drops everything of `id` before its log folder is deleted: the
    /// queue goes, the store is closed, later batches are taken and
    /// ignored, and followers see `removed`. It waits while the writer is
    /// in the middle of an item of `id`.
    pub fn remove(&self, id: InstanceId) {
        let mut state = self.lock();
        state.removed.insert(id);
        if let Some(queue) = state.queues.remove(&id) {
            state.total -= queue.bytes;
        }
        state.order.retain(|other| *other != id);
        state.failing.remove(&id);
        state.to_close.push(id);
        while state.busy == Some(id) {
            state = self
                .shared
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        drop(state);
        self.shared.wake.notify_all();
        publish(&self.shared, id, |head| head.removed = true);
    }

    /// Returns the `instance_logs` health.
    #[must_use]
    pub fn health(&self) -> LogsHealth {
        let now = self.shared.platform.clock().now();
        let recovery = SignedDuration::try_from(self.shared.config.health_recovery)
            .unwrap_or(SignedDuration::MAX);
        let state = self.lock();
        let recent = state
            .last_failure
            .filter(|(at, _)| now.duration_since(*at) < recovery);
        LogsHealth {
            degraded: !state.failing.is_empty() || recent.is_some(),
            reason: state
                .failing
                .values()
                .next()
                .copied()
                .or(recent.map(|(_, reason)| reason)),
            failing: state.failing.len() as u64,
            truncations: state.truncations,
            bytes_truncated: state.bytes_truncated,
        }
    }

    /// Wakes the writer, for a test that moved its clock past a retry.
    pub fn nudge(&self) {
        self.shared.wake.notify_all();
    }

    /// Returns when every queue is empty, or after `timeout`; true when
    /// they are. For tests and the shutdown.
    #[must_use]
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if state.total == 0
                && state.busy.is_none()
                && state.queues.values().all(|queue| queue.items.is_empty())
            {
                return true;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .shared
                .wake
                .wait_timeout(state, left.min(Duration::from_millis(50)))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Writes what is queued (each failing item gets one more try), closes
    /// and syncs every segment, and stops the thread.
    pub fn shutdown(&self) {
        self.lock().shutdown = true;
        self.shared.wake.notify_all();
        let handle = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = handle
            && handle.join().is_err()
        {
            tracing::warn!("the log writer thread panicked");
        }
    }
}

impl std::fmt::Debug for DurableLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DurableLog").finish_non_exhaustive()
    }
}

/// Returns the store folder of `id` under the canonical logs root.
pub(crate) fn store_dir(layout: &PreparedLayout, id: InstanceId) -> PathBuf {
    let dir = layout.layout().instance_captured_logs_dir(id);
    match dir.strip_prefix(layout.layout().roots().logs()) {
        Ok(relative) => layout.canonical_root(RootKind::Logs).join(relative),
        Err(_) => dir,
    }
}

fn publish(shared: &Shared, id: InstanceId, change: impl FnOnce(&mut LogHead)) {
    let mut heads = shared.heads.lock().unwrap_or_else(PoisonError::into_inner);
    let sender = heads
        .entry(id)
        .or_insert_with(|| watch::channel(LogHead::default()).0);
    sender.send_modify(change);
}

/// The writer thread.
fn run(shared: &Shared) {
    let mut stores: HashMap<InstanceId, OpenStore> = HashMap::new();
    let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        for id in std::mem::take(&mut state.to_close) {
            if let Some(mut store) = stores.remove(&id) {
                store.close();
            }
        }
        let now = shared.platform.clock().now();
        let shutdown = state.shutdown;
        let next = state.order.iter().position(|id| {
            state.queues.get(id).is_some_and(|queue| {
                !queue.items.is_empty() && (shutdown || queue.retry_at.is_none_or(|at| at <= now))
            })
        });
        let Some(index) = next else {
            if shutdown {
                break;
            }
            let wait = if state.queues.values().any(|queue| queue.retry_at.is_some()) {
                shared.config.write_retry.min(Duration::from_millis(100))
            } else {
                Duration::from_secs(1)
            };
            state = shared
                .wake
                .wait_timeout(state, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            continue;
        };
        let id = state
            .order
            .remove(index)
            .expect("the position came from the same list");
        let Some(mut item) = state
            .queues
            .get_mut(&id)
            .and_then(|queue| queue.items.pop_front())
        else {
            continue;
        };
        state.busy = Some(id);
        drop(state);

        let outcome = write_item(shared, &mut stores, id, &mut item);

        state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.busy = None;
        let removed = state.removed.contains(&id);
        match outcome {
            Ok(written) => {
                state.truncations += written.truncations;
                state.bytes_truncated += written.bytes_truncated;
                state.failing.remove(&id);
                if let Some(queue) = state.queues.get_mut(&id) {
                    queue.retry_at = None;
                    match &item {
                        Item::Batch { cost, .. } => {
                            queue.bytes -= cost;
                            state.total -= cost;
                        }
                        Item::Run { .. } => queue.run_events -= 1,
                    }
                }
                if let Some((store, head)) = written.head {
                    publish(shared, id, |seen| {
                        seen.store = Some(store);
                        seen.head = head;
                    });
                }
            }
            Err(reason) if shutdown => {
                tracing::warn!(instance = %id, reason = reason.as_code(), "queued log output was dropped at shutdown");
                if let Some(queue) = state.queues.get_mut(&id)
                    && let Item::Batch { cost, .. } = &item
                {
                    queue.bytes -= cost;
                    state.total -= cost;
                }
            }
            Err(reason) => {
                if !state.failing.contains_key(&id) {
                    tracing::warn!(instance = %id, reason = reason.as_code(), "writing the log store failed; retrying");
                }
                state.failing.insert(id, reason);
                state.last_failure = Some((now, reason));
                let retry = SignedDuration::try_from(shared.config.write_retry)
                    .unwrap_or(SignedDuration::from_millis(500));
                if let Some(queue) = state.queues.get_mut(&id) {
                    queue.retry_at = Some(now.saturating_add(retry).unwrap_or(now));
                    queue.items.push_front(item);
                }
            }
        }
        if !removed
            && state
                .queues
                .get(&id)
                .is_some_and(|queue| !queue.items.is_empty())
        {
            state.order.push_back(id);
        }
        shared.wake.notify_all();
    }
    drop(state);
    for store in stores.values_mut() {
        store.close();
    }
}

/// What one written item changed.
#[derive(Default)]
struct Written {
    head: Option<(LogStoreId, Option<u64>)>,
    truncations: u64,
    bytes_truncated: u64,
}

/// Writes one item of `id`, opening its store first when needed.
fn write_item(
    shared: &Shared,
    stores: &mut HashMap<InstanceId, OpenStore>,
    id: InstanceId,
    item: &mut Item,
) -> Result<Written, StoreFailure> {
    let fs = shared.platform.fs();
    let now = shared.platform.clock().now();
    let store = match stores.entry(id) {
        std::collections::hash_map::Entry::Occupied(open) => open.into_mut(),
        std::collections::hash_map::Entry::Vacant(slot) => {
            let dir = store_dir(&shared.layout, id);
            let store = OpenStore::open(fs, now, &dir, shared.config)
                .map_err(|error| StoreFailure::of(&error))?;
            slot.insert(store)
        }
    };
    let mut written = Written::default();
    match item {
        Item::Batch {
            batch, prepared, ..
        } => {
            let stream = batch.stream;
            let (records, done) = prepared.get_or_insert_with(|| (prepare(store, batch), 0));
            let (count, result) = store.append(fs, stream, &records[*done..]);
            for record in &records[*done..*done + count] {
                if let RecordKind::Truncated { bytes } = record.kind {
                    written.truncations += 1;
                    written.bytes_truncated += bytes;
                }
            }
            *done += count;
            result.map_err(|error| StoreFailure::of(&error))?;
        }
        Item::Run { event, seq, done } => {
            let seq = *seq.get_or_insert_with(|| store.take_seq());
            let record = Record::new(seq, event.at(), run_kind(*event));
            for stream in OutputStream::ALL {
                let index = super::scan::slot(stream);
                if done[index] {
                    continue;
                }
                let (_, result) = store.append(fs, stream, std::slice::from_ref(&record));
                result.map_err(|error| StoreFailure::of(&error))?;
                done[index] = true;
            }
        }
    }
    written.head = Some((store.id(), store.head()));
    Ok(written)
}

const fn run_kind(event: RunEvent) -> RecordKind {
    match event {
        RunEvent::Started { pid, .. } => RecordKind::Started { pid: pid.get() },
        RunEvent::Exited { code, signal, .. } => RecordKind::Exited { code, signal },
        RunEvent::Unavailable { .. } => RecordKind::Unavailable,
    }
}

/// Turns a batch into records with their `seq` taken: a `truncated` record
/// for bytes a crash lost (spec 0027, *Kehilangan saat crash*) and for the
/// spool's own drop, then one record per line that is not a duplicate
/// (*Buang duplikat*). Only the last piece of a logical line carries
/// `src`, so a stored `src` always means the whole line is stored.
fn prepare(store: &mut OpenStore, batch: &LogBatch) -> Vec<Record> {
    let stream = batch.stream;
    let at = batch.read_at;
    let mut kinds = Vec::new();
    let source = batch
        .source
        .map(|source| (source.pid.get(), start_time(source.started_at)));
    if let (Some((pid, started_at)), Some(from)) = (source, batch.resumed_from)
        && let Some(last) = store.last_src(stream)
        && last.pid == pid
        && last.started_at == started_at
        && last.end < from
    {
        kinds.push(RecordKind::Truncated {
            bytes: from - last.end,
        });
    }
    if let Some(bytes) = batch.truncated {
        kinds.push(RecordKind::Truncated { bytes });
    }
    for (index, line) in batch.lines.iter().enumerate() {
        let src = match (source, line.end) {
            (Some((pid, started_at)), Some(end)) => Some(RecordSource {
                pid,
                started_at,
                end,
            }),
            _ => None,
        };
        if let (Some(src), Some(last)) = (src, store.last_src(stream))
            && last.same_run(&src)
            && src.end <= last.end
        {
            continue;
        }
        let last_piece = batch
            .lines
            .get(index + 1)
            .is_none_or(|next| next.end != line.end);
        let src = src.filter(|_| last_piece);
        if let Some(src) = src {
            store.set_last_src(stream, src);
        }
        kinds.push(RecordKind::Line(LineRecord {
            level: level_of(&line.text),
            text: line.text.clone(),
            partial: line.partial,
            continued: line.continued,
            step: batch.step.clone(),
            src,
        }));
    }
    kinds
        .into_iter()
        .map(|kind| Record::new(store.take_seq(), at, kind))
        .collect()
}

/// The start time of a process as a timestamp.
fn start_time(start: ProcessStart) -> Timestamp {
    let seconds = i64::try_from(start.seconds()).unwrap_or(i64::MAX);
    let nanos = i32::try_from(start.micros() * 1000).unwrap_or(0);
    Timestamp::new(seconds, nanos).unwrap_or(Timestamp::UNIX_EPOCH)
}
