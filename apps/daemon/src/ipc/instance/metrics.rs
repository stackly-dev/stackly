//! `instance.get_metrics` (spec 0028 AC-11), and what feeds it: the source
//! of running main processes for the sampler and the scheduled scan of the
//! data folders (AC-10).
//!
//! The method only reads: the histories the sampler wrote, the registry
//! row, the supervisor status, and the `last_scan` slot. It never samples,
//! scans, probes, or writes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use stackly_core::metrics::{MainProcesses, RunningMain};
use stackly_domain::{
    HistoryId, InstanceFilter, InstanceId, InstanceLifecycle as DomainLifecycle, ProcessStart,
    Registry,
};
use stackly_protocol::Count;
use stackly_protocol::instance::{
    GetMetricsParams, MetricsProcess, MetricsReply, MetricsSample, MetricsState, ProcessRow,
};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use super::{instance_id, read_instance, summary_of};
use crate::detail::UsageRequest;
use crate::dispatch::HandlerFuture;
use crate::map;
use crate::state::DaemonCore;

/// What one history gave a request.
struct Live {
    history_id: String,
    samples: Vec<MetricsSample>,
    rows: Vec<ProcessRow>,
    truncated: bool,
    reset: bool,
}

/// `instance.get_metrics`.
pub fn get_metrics(
    core: &DaemonCore,
    params: GetMetricsParams,
) -> HandlerFuture<'_, Box<MetricsReply>> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let id = instance_id(params.instance_id)?;
        let instance = read_instance(core, id).await?;
        let process = supervisor.status(id).process;
        let summary = summary_of(core, &supervisor, &instance);
        let config = core.sampler_config();
        let live = (instance.lifecycle() == DomainLifecycle::Running)
            .then_some(process)
            .flatten()
            .map(|process| {
                let history = HistoryId::new(process.pid, process.started_at);
                (process, history)
            });
        let view = live.map(|(_, expected)| {
            let held = expected.to_string();
            core.metrics()
                .with_history(id, |history| {
                    let history = history.filter(|history| history.id() == expected)?;
                    let page = history.page(
                        params.history_id.as_deref(),
                        params.after_seq.map(|seq| seq.get()),
                    );
                    let newest = !page.samples.is_empty();
                    Some(Live {
                        history_id: held.clone(),
                        samples: page
                            .samples
                            .iter()
                            .map(|s| map::metrics::sample(s))
                            .collect(),
                        // The process list belongs to the newest sample,
                        // so a reply with no new sample carries none.
                        rows: if newest {
                            history
                                .latest_processes()
                                .iter()
                                .map(map::metrics::process_row)
                                .collect()
                        } else {
                            Vec::new()
                        },
                        truncated: newest && history.processes_truncated(),
                        reset: page.reset,
                    })
                })
                .unwrap_or_else(|| Live {
                    // The sampler has not taken its first sample of this
                    // process yet: the client holds nothing of it.
                    reset: params.history_id.as_deref() != Some(held.as_str()),
                    history_id: held,
                    samples: Vec::new(),
                    rows: Vec::new(),
                    truncated: false,
                })
        });
        let last_scan = core.data_usage().last_scan(id);
        let (state, history_id, process, samples, rows, truncated, reset) = match view {
            Some(live_view) => (
                MetricsState::Live,
                Some(live_view.history_id),
                summary.process.as_ref().map(|process| MetricsProcess {
                    pid: process.pid,
                    started_at: process.started_at,
                }),
                live_view.samples,
                live_view.rows,
                live_view.truncated,
                live_view.reset,
            ),
            None => (
                MetricsState::NotRunning,
                None,
                None,
                Vec::new(),
                Vec::new(),
                false,
                true,
            ),
        };
        Ok(Box::new(MetricsReply {
            instance_id: params.instance_id,
            state,
            lifecycle: summary.lifecycle,
            interval_seconds: u32::try_from(config.interval.as_secs().max(1)).unwrap_or(u32::MAX),
            capacity: Count::saturating(config.history_capacity.get() as u64),
            history_id,
            process,
            samples,
            latest_processes: rows,
            processes_truncated: truncated,
            history_reset: reset,
            restart_total: summary.restart_total,
            last_failure: summary.last_failure,
            readiness: summary.readiness,
            health: summary.health,
            data_usage: map::metrics::data_usage(last_scan.as_ref()),
        }))
    })
}

/// The running main processes, read from the registry (lifecycle
/// `running`) and the supervisor status (the verified process).
///
/// It holds the daemon core weakly, so the sampler task never keeps the
/// registry open past the daemon's stop.
pub struct RegistryMains {
    core: Weak<DaemonCore>,
}

impl RegistryMains {
    /// A source that reads through `core`.
    #[must_use]
    pub fn new(core: &Arc<DaemonCore>) -> Self {
        Self {
            core: Arc::downgrade(core),
        }
    }
}

impl MainProcesses for RegistryMains {
    fn running(&self) -> Vec<RunningMain> {
        let Some(core) = self.core.upgrade() else {
            return Vec::new();
        };
        let (Some(registry), Ok(supervisor)) = (core.registry(), core.supervision().supervisor())
        else {
            return Vec::new();
        };
        let instances = match registry.instances(InstanceFilter::default()) {
            Ok(listed) => listed.value,
            Err(error) => {
                tracing::debug!(%error, "the running instances could not be listed for metrics");
                return Vec::new();
            }
        };
        instances
            .iter()
            .filter(|instance| instance.lifecycle() == DomainLifecycle::Running)
            .filter_map(|instance| {
                let process = supervisor.status(instance.id()).process?;
                Some(RunningMain::new(
                    instance.id(),
                    process.pid,
                    process.started_at,
                ))
            })
            .collect()
    }
}

/// Starts the scheduled scan of data folders (spec 0028 AC-10): an
/// instance that became running is scanned once, then again every
/// [`schedule_interval`](crate::detail::DataUsageService::schedule_interval)
/// while it runs, one scan at a time. Each scan fills the `last_scan` slot.
/// The task ends when it is aborted or the core is gone.
#[must_use]
pub fn start_data_scans(core: &Arc<DaemonCore>, mains: Arc<dyn MainProcesses>) -> JoinHandle<()> {
    let core = Arc::downgrade(core);
    let poll = core
        .upgrade()
        .map_or_else(Default::default, |core| core.sampler_config().interval);
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(poll);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut due: HashMap<InstanceId, (ProcessStart, Instant)> = HashMap::new();
        loop {
            ticks.tick().await;
            let source = Arc::clone(&mains);
            let Ok(running) = tokio::task::spawn_blocking(move || source.running()).await else {
                continue;
            };
            due.retain(|id, _| running.iter().any(|main| main.instance == *id));
            let mut scanned = false;
            for main in running {
                let Some(core) = core.upgrade() else {
                    return;
                };
                let now = Instant::now();
                let current = due
                    .get(&main.instance)
                    .is_some_and(|(start, next)| *start == main.start && now < *next);
                if current {
                    continue;
                }
                due.insert(
                    main.instance,
                    (main.start, now + core.data_usage().schedule_interval()),
                );
                scan(&core, main.instance).await;
                scanned = true;
            }
            if scanned && let Some(core) = core.upgrade() {
                forget_removed(&core).await;
            }
        }
    })
}

/// Scans the data folder of `id` now and lets the service keep the result.
async fn scan(core: &DaemonCore, id: InstanceId) {
    let Some(registry) = core.registry() else {
        return;
    };
    let layout = core.prepared().layout().clone();
    let path = tokio::task::spawn_blocking(move || {
        let paths = registry.instance_paths(id).ok()?.value?;
        super::detail::data_folder(&layout, &paths)
    })
    .await
    .ok()
    .flatten();
    let Some(path) = path else {
        return;
    };
    let request = UsageRequest {
        refresh: true,
        cached_only: false,
    };
    if let Err(failure) = core
        .data_usage()
        .usage(core.platform(), id, path, request)
        .await
    {
        tracing::debug!(
            reason = failure.reason(),
            "a scheduled data folder scan failed"
        );
    }
}

/// Drops the `last_scan` slots of instances the registry no longer holds.
async fn forget_removed(core: &DaemonCore) {
    let Some(registry) = core.registry() else {
        return;
    };
    let known = tokio::task::spawn_blocking(move || registry.known_instance_ids())
        .await
        .ok()
        .and_then(Result::ok);
    if let Some(known) = known {
        let known: HashSet<InstanceId> = known.value.into_iter().collect();
        core.data_usage().forget_unknown(&known);
    }
}
