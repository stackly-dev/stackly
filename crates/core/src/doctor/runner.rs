//! The runner (spec 0034, *Runner doctor*): every check in its own blocking
//! task with a stop flag it reads between items, a limit per check and per
//! run, two runs at once, and at most one task left behind per check.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_domain::doctor::{
    CHECK_TIME_LIMIT, CheckUnavailableReason, DAEMON_CHECKS, DoctorCheckId, FindingOrder,
    RUN_SLOT_WAIT, RUN_TIME_LIMIT, RUNS_AT_ONCE, SeverityTally, cap_findings,
};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};
use uuid::Uuid;

use super::checks::{self, CheckContext, CheckFailure, CheckFindings};
use super::types::{
    CapacityOutcome, CheckReport, DoctorError, DoctorMode, DoctorRequest, DoctorSources, Finding,
    InstanceRef, Report, SummaryFacts,
};

/// The limits of the runner; the defaults are those of spec 0034.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DoctorLimits {
    /// How long one check may run.
    pub check: Duration,
    /// How long one run may take in all.
    pub run: Duration,
    /// How long a run waits for a free slot before it starts anyway.
    pub slot_wait: Duration,
    /// How many runs go on at once.
    pub runs_at_once: usize,
}

impl Default for DoctorLimits {
    fn default() -> Self {
        Self {
            check: CHECK_TIME_LIMIT,
            run: RUN_TIME_LIMIT,
            slot_wait: RUN_SLOT_WAIT,
            runs_at_once: RUNS_AT_ONCE,
        }
    }
}

impl DoctorLimits {
    /// The same limits with `check` as the limit of one check.
    #[must_use]
    pub const fn with_check(mut self, check: Duration) -> Self {
        self.check = check;
        self
    }

    /// The same limits with `run` as the limit of one run.
    #[must_use]
    pub const fn with_run(mut self, run: Duration) -> Self {
        self.run = run;
        self
    }
}

type CheckOutcome = Result<CheckFindings, CheckFailure>;

/// The doctor of one daemon: its run slots and the check tasks that ran
/// past their limit. Clones share both.
#[derive(Clone)]
pub struct Doctor {
    inner: Arc<Inner>,
}

struct Inner {
    limits: DoctorLimits,
    slots: Arc<Semaphore>,
    stranded: Mutex<HashMap<DoctorCheckId, JoinHandle<CheckOutcome>>>,
}

impl std::fmt::Debug for Doctor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Doctor")
            .field("limits", &self.inner.limits)
            .finish_non_exhaustive()
    }
}

impl Default for Doctor {
    fn default() -> Self {
        Self::new(DoctorLimits::default())
    }
}

impl Doctor {
    /// A doctor with `limits`.
    #[must_use]
    pub fn new(limits: DoctorLimits) -> Self {
        Self {
            inner: Arc::new(Inner {
                limits,
                slots: Arc::new(Semaphore::new(limits.runs_at_once.max(1))),
                stranded: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The limits of this doctor.
    #[must_use]
    pub fn limits(&self) -> DoctorLimits {
        self.inner.limits
    }

    /// Runs the checks `request` names, in the order of the check table,
    /// and reports what they found. `capacity` reads the free space for
    /// `disk_space`; it is polled only when that check runs.
    ///
    /// The run waits for a free slot at most
    /// [`slot_wait`](DoctorLimits::slot_wait), then starts anyway. Each
    /// check gets [`check`](DoctorLimits::check) and the run
    /// [`run`](DoctorLimits::run): a check past its limit reports
    /// `check_unavailable` with reason `timeout`, its stop flag is set, and
    /// it is not waited for. While that task still runs, a later run
    /// reports `timeout` for the same check at once.
    ///
    /// # Errors
    ///
    /// Returns [`DoctorError::InstanceNotFound`] when the request names an
    /// instance the registry does not hold, and [`DoctorError::Registry`]
    /// when reading it failed.
    pub async fn run(
        &self,
        sources: DoctorSources,
        request: DoctorRequest,
        capacity: impl Future<Output = CapacityOutcome> + Send,
    ) -> Result<Report, DoctorError> {
        let limits = self.inner.limits;
        let _slot = timeout(
            limits.slot_wait,
            Arc::clone(&self.inner.slots).acquire_owned(),
        )
        .await
        .ok()
        .and_then(Result::ok);
        let started_at = sources.platform.clock().now();
        let begun = Instant::now();
        let scope = resolve_scope(&sources, request.instance).await?;
        let wanted: Vec<DoctorCheckId> = DAEMON_CHECKS
            .iter()
            .copied()
            .filter(|check| request.checks.is_empty() || request.checks.contains(check))
            .collect();
        let context = Arc::new(CheckContext::new(
            sources.clone(),
            request.instance,
            tokio::runtime::Handle::current(),
        ));
        let deadline = begun + limits.check.min(limits.run);

        let mut slots: Vec<(DoctorCheckId, Slot)> = Vec::with_capacity(wanted.len());
        for check in &wanted {
            slots.push((*check, self.start(*check, &context)));
        }
        let wants_disk = wanted.contains(&DoctorCheckId::DiskSpace);
        let disk = async {
            if wants_disk {
                Some(timeout_at(deadline, capacity).await)
            } else {
                None
            }
        };
        let others = async {
            let mut done = Vec::with_capacity(slots.len());
            for (check, slot) in slots {
                done.push((check, self.finish(check, slot, deadline).await));
            }
            done
        };
        let (disk, done) = tokio::join!(disk, others);

        let mut checks = Vec::with_capacity(done.len());
        let mut tally = SeverityTally::default();
        for (check, outcome) in done {
            let findings = if check == DoctorCheckId::DiskSpace {
                match &disk {
                    Some(Ok(Ok(reading))) => Ok(checks::disk::findings(*reading)),
                    Some(Ok(Err(failure))) => {
                        unavailable(failure.reason, Some(failure.message.clone()))
                    }
                    Some(Err(_)) => unavailable(CheckUnavailableReason::Timeout, None),
                    None => unavailable(CheckUnavailableReason::NotSupported, None),
                }
            } else {
                outcome
            };
            let report = report(check, findings);
            tally = tally.plus(report.tally);
            checks.push(report);
        }
        Ok(Report {
            run_id: Uuid::now_v7(),
            started_at,
            finished_at: sources.platform.clock().now(),
            recovery: matches!(sources.mode, DoctorMode::Recovery { .. }),
            scope,
            checks,
            tally,
        })
    }

    /// Starts `check`, or says at once why it does not run.
    fn start(&self, check: DoctorCheckId, context: &Arc<CheckContext>) -> Slot {
        if check == DoctorCheckId::DiskSpace {
            return Slot::Disk;
        }
        if let Some(reason) = context.refusal(check) {
            return Slot::Done(Box::new(reason));
        }
        let mut stranded = self
            .inner
            .stranded
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(task) = stranded.get(&check) {
            if !task.is_finished() {
                return Slot::Done(Box::new(unavailable(CheckUnavailableReason::Timeout, None)));
            }
            stranded.remove(&check);
        }
        drop(stranded);
        let stop = Arc::new(AtomicBool::new(false));
        let task = {
            let context = Arc::clone(context);
            let stop = Arc::clone(&stop);
            tokio::task::spawn_blocking(move || checks::run(check, &context, &stop))
        };
        Slot::Running { task, stop }
    }

    /// Waits for `slot` until `deadline`; a check still running then is
    /// stopped and kept as left behind.
    async fn finish(&self, check: DoctorCheckId, slot: Slot, deadline: Instant) -> CheckOutcome {
        match slot {
            Slot::Disk => unavailable(CheckUnavailableReason::NotSupported, None),
            Slot::Done(outcome) => *outcome,
            Slot::Running { mut task, stop } => match timeout_at(deadline, &mut task).await {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) => Err(CheckFailure::io("the check stopped unexpectedly")),
                Err(_) => {
                    stop.store(true, Ordering::SeqCst);
                    self.inner
                        .stranded
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(check, task);
                    unavailable(CheckUnavailableReason::Timeout, None)
                }
            },
        }
    }
}

enum Slot {
    Disk,
    Done(Box<CheckOutcome>),
    Running {
        task: JoinHandle<CheckOutcome>,
        stop: Arc<AtomicBool>,
    },
}

/// The outcome of a check that did not run, for `reason`.
pub(super) const fn unavailable(
    reason: CheckUnavailableReason,
    message: Option<String>,
) -> CheckOutcome {
    Err(CheckFailure { reason, message })
}

/// The instance the run is limited to, read once before the checks. In
/// recovery mode, or while the registry is not there, nothing is read.
async fn resolve_scope(
    sources: &DoctorSources,
    instance: Option<stackly_domain::InstanceId>,
) -> Result<Option<InstanceRef>, DoctorError> {
    let (Some(id), DoctorMode::Ready { supervisor, .. }) = (instance, &sources.mode) else {
        return Ok(None);
    };
    let registry = Arc::clone(&supervisor.deps().registry);
    let read = tokio::task::spawn_blocking(move || registry.instance(id))
        .await
        .map_err(|_| DoctorError::Registry(stackly_domain::RegistryError::Unavailable))?;
    match read {
        Ok(versioned) => versioned
            .value
            .map(|instance| InstanceRef::of(&instance))
            .map(Some)
            .ok_or(DoctorError::InstanceNotFound(id)),
        Err(stackly_domain::RegistryError::NotFound { .. }) => {
            Err(DoctorError::InstanceNotFound(id))
        }
        Err(error) => Err(DoctorError::Registry(error)),
    }
}

/// The report of `check` from what it found.
fn report(check: DoctorCheckId, outcome: CheckOutcome) -> CheckReport {
    let found = match outcome {
        Ok(found) => found,
        Err(failure) => CheckFindings {
            findings: vec![Finding::unavailable(failure.reason, failure.message)],
            omitted: Vec::new(),
            summary: SummaryFacts::Unavailable(failure.reason),
            capacity: None,
        },
    };
    let capped = cap_findings(
        found.findings,
        &found.omitted,
        |finding: &Finding| FindingOrder {
            severity: finding.severity,
            instance: finding
                .instance
                .as_ref()
                .map(|instance| instance.name.as_str()),
            code: finding.code,
            subject: finding.subject.as_deref(),
        },
        |finding| finding.severity,
    );
    CheckReport {
        id: check,
        severity: capped.severity,
        findings: capped.kept,
        omitted: capped.omitted,
        tally: capped.tally,
        summary: found.summary,
        capacity: found.capacity,
    }
}
