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

use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{Health, HealthState, Readiness, ReadinessState};
use stackly_platform::HoldsFile;
use tokio::sync::watch;
use tokio::time::Instant;

use super::Supervisor;
use super::actor::{InstanceActor, ProcessSlot, restart_status};
use super::persist::blocking;
use super::probe::{self, PROBE_OK, ProbeContext, ProbeOutcome};
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
        loop {
            if self.stopped() {
                return;
            }
            let outcome = self.attempt(&readiness).await;
            if self.stopped() {
                return;
            }
            match outcome {
                ProbeOutcome::Ok => break,
                ProbeOutcome::Failed { reason, detail } => {
                    let now = deps.platform.clock().now();
                    self.update(|state| {
                        state.readiness =
                            Readiness::new(ReadinessState::NotReady, now, reason, detail);
                    });
                }
            }
            if !self.pause(readiness.interval).await {
                return;
            }
        }
        if !self.first_ready().await {
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
    async fn first_ready(&self) -> bool {
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
            state.readiness = Readiness::new(ReadinessState::Ready, now, PROBE_OK, None);
            state.restart.note_ready(Instant::now(), &config);
        });
        if self.stopped() {
            return false;
        }
        self.ready.send_replace(true);
        true
    }

    /// The probes after the first success.
    async fn routine(mut self, share: bool) {
        let deps = self.sup.deps().clone();
        let readiness = self.readiness.clone();
        let health_plan = self.health.clone();
        let mut failures: u32 = 0;
        let mut health_failures: u32 = 0;
        loop {
            if !self.pause(deps.config.readiness_refresh_interval).await {
                return;
            }
            let outcome = self.attempt(&readiness).await;
            if self.stopped() {
                return;
            }
            let now = deps.platform.clock().now();
            match &outcome {
                ProbeOutcome::Ok => {
                    failures = 0;
                    let config = deps.config.clone();
                    self.update(|state| {
                        state.readiness =
                            Readiness::new(ReadinessState::Ready, now, PROBE_OK, None);
                        if state.restart.note_ready(Instant::now(), &config) {
                            state.restart_status = restart_status(&config, 0, None, false);
                        }
                    });
                }
                ProbeOutcome::Failed { reason, detail } => {
                    failures += 1;
                    if failures >= deps.config.readiness_failure_threshold.get() {
                        let (reason, detail) = (reason.clone(), detail.clone());
                        self.update(|state| {
                            state.readiness =
                                Readiness::new(ReadinessState::NotReady, now, reason, detail);
                            state.restart.note_not_ready();
                        });
                    }
                }
            }
            if share {
                health_failures = observe_health(
                    &self.sup,
                    &self.actor,
                    self.generation,
                    &health_plan,
                    &outcome,
                    health_failures,
                );
            }
            // Time alone may have turned an observation stale.
            self.actor.refresh(&deps);
        }
    }
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
        let mut failures: u32 = 0;
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
            failures = observe_health(
                &self.sup,
                &self.actor,
                self.generation,
                &self.plan,
                &outcome,
                failures,
            );
            let mut stop = self.stop.clone();
            tokio::select! {
                () = tokio::time::sleep(self.plan.interval) => {}
                _ = stop.wait_for(|stopped| *stopped) => return,
            }
        }
    }
}

/// Applies one health attempt: `healthy` on success, `degraded` below the
/// failure threshold, `unhealthy` at it. Returns the new count of
/// consecutive failures.
fn observe_health(
    sup: &Supervisor,
    actor: &InstanceActor,
    generation: u64,
    plan: &HealthPlan,
    outcome: &ProbeOutcome,
    failures: u32,
) -> u32 {
    let deps = sup.deps();
    let now = deps.platform.clock().now();
    let (failures, health) = match outcome {
        ProbeOutcome::Ok => (0, Health::new(HealthState::Healthy, now, PROBE_OK, None)),
        ProbeOutcome::Failed { reason, detail } => {
            let failures = failures.saturating_add(1);
            let state = if failures >= u32::from(plan.failure_threshold.get()) {
                HealthState::Unhealthy
            } else {
                HealthState::Degraded
            };
            (
                failures,
                Health::new(state, now, reason.clone(), detail.clone()),
            )
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
    failures
}
