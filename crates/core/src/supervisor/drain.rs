//! The spool reading of one running process (spec 0010, *Spool log*): the
//! task that pumps both spools every `SPOOL_DRAIN_INTERVAL`, and the final
//! read when the process exits.
//!
//! A [`DrainSlot`] belongs to one [`ProcessSlot`](super::actor::ProcessSlot).
//! The pumps are plain file reads, so they run on the blocking pool and
//! never hold the actor.

use std::sync::{Arc, Mutex, PoisonError};

use stackly_domain::InstanceId;
use stackly_platform::{OutputStream, ProcessError};
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use super::persist::blocking;
use super::{DrainContext, DrainOutcome, SpoolDrain, Supervisor, SupervisorDeps};

/// How a [`DrainSlot`] starts reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opening {
    /// The process was spawned a moment ago and the spools are new.
    Fresh,
    /// The process was found running after a daemon restart: reading
    /// resumes at the first byte that is not freed.
    // provisional: F10 step 11 (adoption) opens its slots this way.
    #[allow(dead_code)]
    Resumed,
}

/// The readers of the two spools of one process, and the task that pumps
/// them.
pub(crate) struct DrainSlot {
    stop: watch::Sender<bool>,
    drains: Arc<Mutex<Vec<SpoolDrain>>>,
}

impl DrainSlot {
    /// Opens the spools of `id` and starts the pump task. The flag is true
    /// when a spool could not be opened, so the instance reports
    /// `logsUnavailable`; the other stream is still read.
    pub(crate) async fn start(
        sup: &Supervisor,
        id: InstanceId,
        opening: Opening,
    ) -> (Arc<Self>, bool) {
        let deps = sup.deps().clone();
        let opened = blocking(move || open_all(&deps, id, opening)).await;
        let (drains, unavailable) = opened.unwrap_or_else(|error| {
            tracing::warn!(instance = %id, %error, "the spools could not be opened");
            (Vec::new(), true)
        });
        let (stop, stop_rx) = watch::channel(false);
        let slot = Arc::new(Self {
            stop,
            drains: Arc::new(Mutex::new(drains)),
        });
        if !slot.is_empty() {
            tokio::spawn(pump_loop(
                sup.deps().clone(),
                id,
                Arc::clone(&slot.drains),
                stop_rx,
            ));
        }
        (slot, unavailable)
    }

    fn is_empty(&self) -> bool {
        self.drains
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    /// Stops the pump task. Nothing more is read.
    pub(crate) fn stop(&self) {
        self.stop.send_replace(true);
    }

    /// Stops the pump task, then reads both spools to their end and hands
    /// over what the process left, as `partial` lines if they have no
    /// newline. A sink that stays busy is logged; the bytes stay in the
    /// spool.
    pub(crate) async fn finish(&self, sup: &Supervisor, id: InstanceId) {
        self.stop();
        let deps = sup.deps().clone();
        let drains = Arc::clone(&self.drains);
        let done = blocking(move || {
            let mut drains = drains.lock().unwrap_or_else(PoisonError::into_inner);
            let ctx = context(&deps, id);
            for drain in drains.iter_mut() {
                match drain.finish(&ctx) {
                    Ok(DrainOutcome::Busy) => {
                        tracing::warn!(
                            instance = %id,
                            stream = drain.stream().as_code(),
                            "the log sink was busy when the process exited; the rest stays in the spool"
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(instance = %id, %error, "the last read of a spool failed");
                    }
                }
            }
        })
        .await;
        if let Err(error) = done {
            tracing::warn!(instance = %id, %error, "the last read of the spools did not run");
        }
    }
}

fn context(deps: &SupervisorDeps, id: InstanceId) -> DrainContext<'_> {
    DrainContext::new(id, &deps.redactor, &*deps.sinks, deps.platform.clock())
}

/// Opens the stdout and stderr spool readers of `id`.
fn open_all(deps: &SupervisorDeps, id: InstanceId, opening: Opening) -> (Vec<SpoolDrain>, bool) {
    let key = id.short_key();
    let mut drains = Vec::new();
    let mut unavailable = false;
    for stream in OutputStream::ALL {
        let path = deps.layout.layout().instance_spool(key, stream);
        let reader = match deps.platform.processes().open_spool(&path) {
            Ok(Some(reader)) => reader,
            Ok(None) => {
                tracing::warn!(instance = %id, stream = stream.as_code(), "the spool does not exist");
                unavailable = true;
                continue;
            }
            Err(error) => {
                tracing::warn!(instance = %id, stream = stream.as_code(), %error, "the spool could not be opened");
                unavailable = true;
                continue;
            }
        };
        let drain = match opening {
            Opening::Fresh => Ok(SpoolDrain::start(stream, reader, &deps.config)),
            Opening::Resumed => SpoolDrain::resume(stream, reader, &deps.config),
        };
        match drain {
            Ok(drain) => drains.push(drain),
            Err(error) => {
                tracing::warn!(instance = %id, stream = stream.as_code(), %error, "the spool could not be read");
                unavailable = true;
            }
        }
    }
    (drains, unavailable)
}

/// Pumps both spools once. `More` wins over `Busy`, so a busy stream does
/// not slow the other one.
fn pump_once(
    deps: &SupervisorDeps,
    id: InstanceId,
    drains: &Mutex<Vec<SpoolDrain>>,
) -> Result<DrainOutcome, ProcessError> {
    let mut drains = drains.lock().unwrap_or_else(PoisonError::into_inner);
    let ctx = context(deps, id);
    let mut outcome = DrainOutcome::CaughtUp;
    let mut failure = None;
    for drain in drains.iter_mut() {
        match drain.pump(&ctx) {
            Ok(DrainOutcome::More) => outcome = DrainOutcome::More,
            Ok(DrainOutcome::Busy) if outcome == DrainOutcome::CaughtUp => {
                outcome = DrainOutcome::Busy;
            }
            Ok(_) => {}
            Err(error) => failure = failure.or(Some(error)),
        }
    }
    failure.map_or(Ok(outcome), Err)
}

async fn pump_loop(
    deps: SupervisorDeps,
    id: InstanceId,
    drains: Arc<Mutex<Vec<SpoolDrain>>>,
    mut stop: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(deps.config.spool_drain_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut warned = false;
    loop {
        tokio::select! {
            biased;
            _ = stop.wait_for(|stopped| *stopped) => return,
            _ = tick.tick() => {}
        }
        loop {
            let (deps, drains) = (deps.clone(), Arc::clone(&drains));
            match blocking(move || pump_once(&deps, id, &drains)).await {
                Ok(Ok(DrainOutcome::More)) => {}
                Ok(Ok(_)) => break,
                Ok(Err(error)) => {
                    if !warned {
                        warned = true;
                        tracing::warn!(instance = %id, %error, "reading a spool failed");
                    }
                    break;
                }
                Err(error) => {
                    tracing::warn!(instance = %id, %error, "reading the spools did not run");
                    return;
                }
            }
        }
    }
}
