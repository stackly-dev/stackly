//! Readiness and health probing of a running process (spec 0010, *Probe,
//! readiness, dan health*).
//!
//! One task per process probes readiness every `interval` until the first
//! success, with no deadline of its own (the start operation holds the
//! deadline and the probes go on after it, so an instance that was slow
//! still becomes `ready`). After that it probes every
//! `readiness_refresh_interval`; a success renews `ready`, a failure turns
//! it `not_ready` only after `readiness_failure_threshold` in a row, and
//! each tick reads the observations again with `effective`, so a value that
//! went stale is reported.
//!
//! Health runs on its own interval, or on the readiness attempts when the
//! probe and the interval are the same. Neither ever starts, stops, or
//! restarts anything (REQ-012).
//!
//! Every observation a probe makes carries the latency of that attempt
//! (spec 0028 AC-7). A wake from sleep (spec 0028 AC-9) makes the routine
//! loops read readiness and health `waking`, probe at once, and give the
//! failures of the next [`WAKE_GRACE`](super::wake::WAKE_GRACE) no weight.

use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{
    Health, HealthState, Readiness, ReadinessState, RegistryError, WriteBatch, WriteOp,
};
use stackly_platform::HoldsFile;
use tokio::sync::watch;
use tokio::time::Instant;

use super::Supervisor;
use super::actor::{InstanceActor, ProcessSlot, restart_status};
use super::persist::blocking;
use super::probe::{self, PROBE_OK, ProbeContext, ProbeOutcome};
use super::wake::{Track, Wake, waking};
use crate::provider::{HealthPlan, ProbePlan};

/// Starts the probe task of one process and returns the receiver that
/// becomes `true` at its first successful readiness probe.
pub(crate) fn spawn_probes(
    sup: Supervisor,
    actor: Arc<InstanceActor>,
    generation: u64,
    readiness: ProbePlan,
    health: HealthPlan,
    stop: watch::Receiver<bool>,
) -> watch::Receiver<bool> {
    sup.start_wake_watch();
    let (ready, ready_rx) = watch::channel(false);
    let probes = Probes {
        sup,
        actor,
        generation,
        readiness,
        health,
        stop,
        ready,
    };
    tokio::spawn(probes.run());
    ready_rx
}

struct Probes {
    sup: Supervisor,
    actor: Arc<InstanceActor>,
    generation: u64,
    readiness: ProbePlan,
    health: HealthPlan,
    stop: watch::Receiver<bool>,
    ready: watch::Sender<bool>,
}

impl Probes {
    fn stopped(&self) -> bool {
        *self.stop.borrow()
    }

    /// Sleeps `pause`, or until the probes are told to stop. Returns false
    /// when they were.
    async fn pause(&mut self, pause: Duration) -> bool {
        let mut stop = self.stop.clone();
        tokio::select! {
            () = tokio::time::sleep(pause) => !self.stopped(),
            _ = stop.wait_for(|stopped| *stopped) => false,
        }
    }

    async fn attempt(&self, plan: &ProbePlan) -> ProbeOutcome {
        let deps = self.sup.deps();
        let context = ProbeContext {
            platform: &deps.platform,
            redactor: &deps.redactor,
            config: &deps.config,
            native: deps.native.as_ref(),
        };
        probe::run(&plan.probe, plan.attempt_timeout, context).await
    }

    async fn run(mut self) {
        let deps = self.sup.deps().clone();
        let readiness = self.readiness.clone();
        let first_latency;
        loop {
            if self.stopped() {
                return;
            }
            let outcome = self.attempt(&readiness).await;
            if self.stopped() {
                return;
            }
            match outcome {
                ProbeOutcome::Ok { latency } => {
                    first_latency = latency;
                    break;
                }
                ProbeOutcome::Failed {
                    reason,
                    detail,
                    latency,
                } => {
                    let now = deps.platform.clock().now();
                    self.update(|state| {
                        state.readiness =
                            Readiness::new(ReadinessState::NotReady, now, reason, detail)
                                .with_latency(latency);
                    });
                }
            }
            if !self.pause(readiness.interval).await {
                return;
            }
        }
        if !self.first_ready(first_latency).await {
            return;
        }
        let share = self.health.probe == self.readiness.probe
            && self.health.interval == deps.config.readiness_refresh_interval;
        if !share {
            let health = HealthLoop {
                sup: self.sup.clone(),
                actor: Arc::clone(&self.actor),
                generation: self.generation,
                plan: self.health.clone(),
                stop: self.stop.clone(),
            };
            tokio::spawn(health.run());
        }
        self.routine(share).await;
    }

    /// Applies `change` to the actor's state when this probe task still
    /// belongs to the current process.
    fn update(&self, change: impl FnOnce(&mut super::actor::ActorState)) {
        let generation = self.generation;
        self.actor.update(self.sup.deps(), |state| {
            let ours = state
                .process
                .as_ref()
                .is_some_and(|slot| slot.generation == generation);
            if ours {
                change(state);
            }
        });
    }

    /// The first successful readiness probe: `holds_file` once, the ports
    /// become final, readiness is `ready`.
    async fn first_ready(&self, latency: Duration) -> bool {
        let deps = self.sup.deps();
        let target = self.actor.read(|state| {
            state
                .process
                .as_ref()
                .filter(|slot| slot.generation == self.generation)
                .map(|slot| (slot.identity.pid, slot.lock))
        });
        let Some((pid, lock)) = target else {
            return false;
        };
        let platform = deps.platform.clone();
        let holds = blocking(move || platform.processes().holds_file(pid, lock)).await;
        let adoptable = matches!(holds, Ok(Ok(HoldsFile::Yes)));
        if !adoptable {
            tracing::warn!(
                %pid,
                "the process does not hold the instance lock, so it cannot be adopted after a daemon restart"
            );
        }
        let now = deps.platform.clock().now();
        let config = deps.config.clone();
        self.update(|state| {
            if let Some(slot) = state.process.as_mut() {
                slot.adoptable = Some(adoptable);
            }
            state.ports_final = true;
            state.readiness =
                Readiness::new(ReadinessState::Ready, now, PROBE_OK, None).with_latency(latency);
            state.restart.note_ready(Instant::now(), &config);
        });
        mark_ready(&self.sup, &self.actor, self.generation);
        if self.stopped() {
            return false;
        }
        self.ready.send_replace(true);
        true
    }

    /// The probes after the first success.
    async fn routine(self, share: bool) {
        let deps = self.sup.deps().clone();
        let readiness = self.readiness.clone();
        let health_plan = self.health.clone();
        let mut wakes = self.sup.subscribe_wake();
        let mut ready_track = Track::default();
        let mut health_track = Track::default();
        loop {
            let mut stop = self.stop.clone();
            let woke = tokio::select! {
                () = tokio::time::sleep(deps.config.readiness_refresh_interval) => None,
                wake = wakes.next() => Some(wake),
                _ = stop.wait_for(|stopped| *stopped) => return,
            };
            if self.stopped() {
                return;
            }
            if let Some(wake) = woke {
                let grace_until = Instant::now() + deps.config.wake_grace;
                ready_track.wake(grace_until);
                if share {
                    health_track.wake(grace_until);
                }
                self.note_wake(&wake, share);
            }
            let outcome = self.attempt(&readiness).await;
            if self.stopped() {
                return;
            }
            let now = deps.platform.clock().now();
            match &outcome {
                ProbeOutcome::Ok { latency } => {
                    ready_track.succeeded();
                    let config = deps.config.clone();
                    let latency = *latency;
                    let mut back = false;
                    self.update(|state| {
                        back = state.readiness.state() != ReadinessState::Ready;
                        state.readiness =
                            Readiness::new(ReadinessState::Ready, now, PROBE_OK, None)
                                .with_latency(latency);
                        if state.restart.note_ready(Instant::now(), &config) {
                            state.restart_status = restart_status(&config, 0, None, false);
                        }
                    });
                    if back {
                        mark_ready(&self.sup, &self.actor, self.generation);
                    }
                }
                ProbeOutcome::Failed {
                    reason,
                    detail,
                    latency,
                } => {
                    let counted = ready_track.failed(Instant::now());
                    if counted
                        && ready_track.failures >= deps.config.readiness_failure_threshold.get()
                    {
                        let (reason, detail, latency) = (reason.clone(), detail.clone(), *latency);
                        self.update(|state| {
                            state.readiness =
                                Readiness::new(ReadinessState::NotReady, now, reason, detail)
                                    .with_latency(latency);
                            state.restart.note_not_ready();
                        });
                    }
                }
            }
            if share {
                observe_health(
                    &self.sup,
                    &self.actor,
                    self.generation,
                    &health_plan,
                    &outcome,
                    &mut health_track,
                );
            }
            // Time alone may have turned an observation stale.
            self.actor.refresh(&deps);
        }
    }

    /// A wake: readiness, and health when this loop also runs it, read
    /// `waking` until a probe answers. Nothing changes while the process
    /// is being stopped.
    fn note_wake(&self, wake: &Wake, share: bool) {
        let (readiness, health) = waking(wake);
        self.update(|state| {
            if state.stopping {
                return;
            }
            state.readiness = readiness;
            if share {
                state.health = health;
            }
        });
    }
}

/// Marks the revision the process of `generation` was rendered for as
/// ready, once, when it is still the active revision (spec 0029, *Last
/// known good*). The write runs on a blocking thread outside the probe
/// task, and readiness in memory never waits for it; a write that fails is
/// logged and tried again at the next ready transition.
pub(crate) fn mark_ready(sup: &Supervisor, actor: &InstanceActor, generation: u64) {
    let applied = actor.read(|state| {
        state
            .process
            .as_ref()
            .filter(|slot| slot.generation == generation)
            .and_then(|slot| slot.applied_revision)
    });
    let Some(revision) = applied else {
        return;
    };
    let deps = sup.deps().clone();
    let id = actor.id;
    tokio::spawn(async move {
        let at = deps.platform.clock().now();
        let registry = deps.registry.clone();
        let written = blocking(move || -> Result<bool, RegistryError> {
            let active = registry.active_config_revision(id)?.value;
            if active.is_none_or(|active| active.id != revision || active.ready_at.is_some()) {
                return Ok(false);
            }
            registry
                .commit(WriteBatch::new(vec![WriteOp::MarkConfigRevisionReady {
                    revision,
                    at,
                }]))
                .map_err(|error| error.error)?;
            Ok(true)
        })
        .await;
        match written {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                tracing::warn!(instance = %id, %revision, %error, "the ready config revision could not be recorded");
            }
            Err(error) => {
                tracing::warn!(instance = %id, %revision, %error, "the ready config revision could not be recorded");
            }
        }
    });
}

struct HealthLoop {
    sup: Supervisor,
    actor: Arc<InstanceActor>,
    generation: u64,
    plan: HealthPlan,
    stop: watch::Receiver<bool>,
}

impl HealthLoop {
    async fn run(self) {
        let deps = self.sup.deps().clone();
        let mut wakes = self.sup.subscribe_wake();
        let mut track = Track::default();
        loop {
            if *self.stop.borrow() {
                return;
            }
            let context = ProbeContext {
                platform: &deps.platform,
                redactor: &deps.redactor,
                config: &deps.config,
                native: deps.native.as_ref(),
            };
            let outcome = probe::run(&self.plan.probe, self.plan.attempt_timeout, context).await;
            if *self.stop.borrow() {
                return;
            }
            observe_health(
                &self.sup,
                &self.actor,
                self.generation,
                &self.plan,
                &outcome,
                &mut track,
            );
            let mut stop = self.stop.clone();
            tokio::select! {
                () = tokio::time::sleep(self.plan.interval) => {}
                wake = wakes.next() => {
                    track.wake(Instant::now() + deps.config.wake_grace);
                    self.note_wake(&wake);
                }
                _ = stop.wait_for(|stopped| *stopped) => return,
            }
        }
    }

    /// A wake: health reads `waking` until the probe that follows answers.
    fn note_wake(&self, wake: &Wake) {
        let (_, health) = waking(wake);
        let generation = self.generation;
        self.actor.update(self.sup.deps(), |state| {
            let ours = state
                .process
                .as_ref()
                .is_some_and(|slot: &ProcessSlot| slot.generation == generation);
            if ours && !state.stopping {
                state.health = health;
            }
        });
    }
}

/// Applies one health attempt: `healthy` on success, `degraded` below the
/// failure threshold, `unhealthy` at it. `track` holds the count of
/// consecutive failures; a failure inside the grace of a wake changes
/// nothing and is not counted.
fn observe_health(
    sup: &Supervisor,
    actor: &InstanceActor,
    generation: u64,
    plan: &HealthPlan,
    outcome: &ProbeOutcome,
    track: &mut Track,
) {
    let deps = sup.deps();
    let now = deps.platform.clock().now();
    let health = match outcome {
        ProbeOutcome::Ok { latency } => {
            track.succeeded();
            Health::new(HealthState::Healthy, now, PROBE_OK, None).with_latency(*latency)
        }
        ProbeOutcome::Failed {
            reason,
            detail,
            latency,
        } => {
            if !track.failed(Instant::now()) {
                return;
            }
            let state = if track.failures >= u32::from(plan.failure_threshold.get()) {
                HealthState::Unhealthy
            } else {
                HealthState::Degraded
            };
            Health::new(state, now, reason.clone(), detail.clone()).with_latency(*latency)
        }
    };
    actor.update(deps, |state| {
        let ours = state
            .process
            .as_ref()
            .is_some_and(|slot: &ProcessSlot| slot.generation == generation);
        if ours {
            state.health = health;
        }
    });
}
