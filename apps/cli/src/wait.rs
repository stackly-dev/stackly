//! `--wait` (spec 0008, *`--wait`*): wait for an operation to reach a final
//! state or `needs_recovery` without ever cancelling it.
//!
//! The snapshot from `operation.get` is the truth; events only say that a
//! refresh is due. At most one get runs at a time, at least 250 ms apart,
//! with a refresh every 5 seconds anyway. `--timeout` and Ctrl+C bound the
//! whole wait, connecting, retries, and reconnects included, and neither
//! ever sends `operation.cancel`.

use std::time::Duration;

use clap::Args;
use stackly_protocol::codes::{daemon, ipc};
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::operation::{
    GetOperationParams, OperationDetail, OperationProgress, OperationState,
};
use stackly_protocol::{Call, ResourceKind, ResourceRef};
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::time::{Instant, interval_at, sleep, sleep_until};
use uuid::Uuid;

use crate::context::Context;
use crate::exit::EXIT_INTERRUPTED;
use crate::failure::CliFailure;
use crate::output::human::wire_name;
use crate::progress::{Progress, ProgressMode, Update};
use crate::session::CallSpec;

/// A refresh at least this often, whatever the events say.
pub const TICK: Duration = Duration::from_secs(5);
/// The least time between two `operation.get`.
pub const REFRESH_GAP: Duration = Duration::from_millis(250);
/// How long the connection may stay broken, or the gets may keep failing,
/// before the wait ends with `cli.wait_disconnected`.
pub const RECONNECT_BUDGET: Duration = Duration::from_secs(30);
/// The first wait before reconnecting or retrying a get; it doubles.
pub const RECONNECT_FIRST: Duration = Duration::from_millis(250);
/// The longest wait between two reconnect or get attempts.
pub const RECONNECT_MAX: Duration = Duration::from_secs(2);
/// How often the live progress line is drawn again for its wait time.
pub const REDRAW: Duration = Duration::from_secs(1);
/// The state written before the first snapshot arrived.
pub const UNKNOWN_STATE: &str = "unknown";

/// `--wait` and `--timeout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub struct WaitArgs {
    /// Wait until the operation is final or needs recovery; stopping the wait never cancels it
    #[arg(long)]
    pub wait: bool,
    /// With --wait, stop waiting after this long: 90, 30s, 5m, or 1h30m
    #[arg(long, requires = "wait", value_name = "DURATION", value_parser = crate::args::timeout)]
    pub timeout: Option<Duration>,
}

/// What the wait is for, which decides the exit at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitMode {
    /// `inspect` and commands that start an operation: `succeeded` exits 0.
    Observe,
    /// `cancel`: `cancelled` exits 0, `succeeded` and `failed` do not.
    Cancel,
}

/// How a wait ended: the last snapshot, if any came, and the failure the
/// command ends with, if any.
#[derive(Debug)]
pub struct WaitOutcome {
    /// The newest snapshot.
    pub last: Option<OperationDetail>,
    /// `None` when the end state means exit 0 for the mode.
    pub failure: Option<CliFailure>,
}

/// Why the wait stopped.
enum Stop {
    Interrupted,
    TimedOut,
    Followed(Result<(), CliFailure>),
}

/// Waits for operations.
#[derive(Debug)]
pub struct Waiter;

impl Waiter {
    /// Waits for operation `id` in `mode`. SIGINT is registered before the
    /// `waiting for operation` line, so a Ctrl+C after that line is always
    /// caught.
    pub async fn wait(cx: &mut Context, id: Uuid, args: &WaitArgs, mode: WaitMode) -> WaitOutcome {
        let mut interrupt = signal(SignalKind::interrupt()).ok();
        let started = Instant::now();
        let deadline = args.timeout.and_then(|limit| started.checked_add(limit));
        if !cx.output.quiet {
            cx.output.stderr.line(&format!(
                "waiting for operation {id} (Ctrl+C stops waiting; the operation keeps running)"
            ));
        }
        let mut progress = Progress::new(cx.progress);
        let mut last: Option<OperationDetail> = None;
        let stop = {
            let follow = follow(cx, id, &mut last, &mut progress, started);
            tokio::pin!(follow);
            tokio::select! {
                biased;
                () = interrupted(interrupt.as_mut()) => Stop::Interrupted,
                () = expired(deadline) => Stop::TimedOut,
                result = &mut follow => Stop::Followed(result),
            }
        };
        progress.finish(&mut cx.output.stderr);
        let state = state_of(last.as_ref());
        let failure = match stop {
            Stop::Interrupted => {
                exit_on_next_interrupt();
                Some(CliFailure::interrupted(id, &state))
            }
            Stop::TimedOut => Some(CliFailure::wait_timeout(
                id,
                &state,
                args.timeout.unwrap_or_default(),
            )),
            Stop::Followed(Ok(())) => last.as_ref().and_then(|detail| settle(mode, detail)),
            Stop::Followed(Err(failure)) => Some(failure),
        };
        WaitOutcome { last, failure }
    }
}

/// The failure an operation that stopped moving means in `mode`, or `None`
/// for exit 0 (spec 0008, *Pemetaan hasil*). The operation's own error
/// comes first; the `cli.operation_*` codes stand in when it has none.
#[must_use]
pub fn settle(mode: WaitMode, detail: &OperationDetail) -> Option<CliFailure> {
    let operation = &detail.operation;
    let own_error = operation
        .failure
        .as_ref()
        .map(|failure| failure.error.clone())
        .or_else(|| {
            operation
                .recovery
                .as_ref()
                .and_then(|recovery| recovery.error.clone())
        });
    let own = |fallback: fn(Uuid) -> CliFailure| {
        own_error.clone().map_or_else(
            || fallback(operation.id),
            |error| CliFailure::daemon(error, None),
        )
    };
    match (mode, operation.state) {
        (_, OperationState::NeedsRecovery) => Some(own(CliFailure::operation_needs_recovery)),
        (WaitMode::Observe, OperationState::Succeeded)
        | (WaitMode::Cancel, OperationState::Cancelled) => None,
        (WaitMode::Observe, OperationState::Failed) => Some(own(CliFailure::operation_failed)),
        (WaitMode::Observe, OperationState::Cancelled) => {
            Some(own(CliFailure::operation_cancelled))
        }
        (WaitMode::Cancel, state @ (OperationState::Succeeded | OperationState::Failed)) => Some(
            CliFailure::cancel_not_applied(operation.id, &wire_name(&state)),
        ),
        (_, OperationState::Queued | OperationState::Running | OperationState::CancelRequested) => {
            None
        }
    }
}

/// Whether the operation stopped moving: a final state or
/// `needs_recovery`.
#[must_use]
pub const fn settled(state: OperationState) -> bool {
    state.is_final() || matches!(state, OperationState::NeedsRecovery)
}

/// The state of the newest snapshot, or `unknown`.
fn state_of(last: Option<&OperationDetail>) -> String {
    last.map_or_else(
        || UNKNOWN_STATE.to_owned(),
        |detail| wire_name(&detail.operation.state),
    )
}

/// Subscribes, then gets, then refreshes on events and ticks, reconnecting
/// when the connection breaks. Returns once the operation settled.
async fn follow(
    cx: &mut Context,
    id: Uuid,
    last: &mut Option<OperationDetail>,
    progress: &mut Progress,
    started: Instant,
) -> Result<(), CliFailure> {
    let mut trouble = Trouble::default();
    let params = SubscribeParams {
        since: None,
        kinds: vec!["operation.".to_owned()],
        resource: Some(ResourceRef {
            kind: ResourceKind::Operation,
            id: id.to_string(),
        }),
    };
    loop {
        let mut stream = match cx.subscribe(params.clone()).await {
            Ok((_, stream)) => stream,
            Err(failure) if breaks_connection(&failure) || retried(&failure) => {
                cx.disconnect();
                trouble.pause(id, last.as_ref()).await?;
                continue;
            }
            Err(failure) => return Err(failure),
        };
        let mut refresh = true;
        let mut last_get: Option<Instant> = None;
        let mut tick = interval_at(Instant::now() + TICK, TICK);
        let mut redraw = interval_at(Instant::now() + REDRAW, REDRAW);
        loop {
            if refresh {
                if let Some(at) = last_get {
                    sleep_until(at + REFRESH_GAP).await;
                }
                refresh = false;
                last_get = Some(Instant::now());
                let call = Call::OperationGet(GetOperationParams { operation_id: id });
                match cx.call_once(call, CallSpec::control()).await {
                    Ok(stackly_protocol::Reply::OperationGet(detail)) => {
                        trouble = Trouble::default();
                        let newer = last
                            .as_ref()
                            .is_none_or(|held| detail.operation.revision > held.operation.revision);
                        if newer {
                            *last = Some(*detail);
                        }
                        show(cx, progress, last.as_ref(), started);
                        if last
                            .as_ref()
                            .is_some_and(|detail| settled(detail.operation.state))
                        {
                            return Ok(());
                        }
                    }
                    Ok(_) => return Err(CliFailure::unexpected_reply(None)),
                    Err(failure) if breaks_connection(&failure) => {
                        cx.disconnect();
                        trouble.pause(id, last.as_ref()).await?;
                        break;
                    }
                    Err(failure) if retried(&failure) => {
                        trouble.pause(id, last.as_ref()).await?;
                        refresh = true;
                        continue;
                    }
                    Err(failure) => return Err(failure),
                }
            }
            tokio::select! {
                event = stream.next() => match event {
                    Ok(Some(_)) => refresh = true,
                    Ok(None) | Err(_) => {
                        cx.disconnect();
                        trouble.pause(id, last.as_ref()).await?;
                        break;
                    }
                },
                _ = tick.tick() => refresh = true,
                _ = redraw.tick(), if progress.mode() == ProgressMode::Live => {
                    show(cx, progress, last.as_ref(), started);
                }
            }
        }
    }
}

/// The reconnect and retry budget: how long things have been failing and
/// the next wait.
#[derive(Debug)]
struct Trouble {
    since: Option<Instant>,
    next: Duration,
}

impl Default for Trouble {
    fn default() -> Self {
        Self {
            since: None,
            next: RECONNECT_FIRST,
        }
    }
}

impl Trouble {
    /// Waits before the next attempt, or fails `cli.wait_disconnected` once
    /// the budget is spent.
    async fn pause(&mut self, id: Uuid, last: Option<&OperationDetail>) -> Result<(), CliFailure> {
        let since = *self.since.get_or_insert_with(Instant::now);
        let spent = since.elapsed();
        if spent >= RECONNECT_BUDGET {
            return Err(CliFailure::wait_disconnected(id, &state_of(last)));
        }
        sleep(self.next.min(RECONNECT_BUDGET - spent)).await;
        self.next = (self.next * 2).min(RECONNECT_MAX);
        Ok(())
    }
}

/// The connection really broke: reconnect.
fn breaks_connection(failure: &CliFailure) -> bool {
    [
        &ipc::CONNECTION_CLOSED,
        &ipc::ENDPOINT_MISSING,
        &ipc::CONNECTION_REFUSED,
        &ipc::NOT_CONNECTED,
    ]
    .iter()
    .any(|spec| failure.body.is(spec))
}

/// The daemon is there but cannot answer yet: retry within the budget.
/// `daemon.shutting_down` counts here, so a get during a planned restart
/// does not end the wait.
fn retried(failure: &CliFailure) -> bool {
    [
        &ipc::CALL_TIMEOUT,
        &daemon::STARTING,
        &daemon::SHUTTING_DOWN,
    ]
    .iter()
    .any(|spec| failure.body.is(spec))
}

/// Draws the newest snapshot, or `unknown` before the first.
fn show(
    cx: &mut Context,
    progress: &mut Progress,
    last: Option<&OperationDetail>,
    started: Instant,
) {
    let state = state_of(last);
    let operation = last.map(|detail| &detail.operation);
    let counted = operation.and_then(|operation| match operation.progress {
        OperationProgress::Counted(counted) => Some((counted.done.get(), counted.total.get())),
        OperationProgress::Indeterminate => None,
    });
    progress.show(
        &mut cx.output.stderr,
        &Update {
            state: &state,
            stage: operation.and_then(|operation| operation.stage.as_deref()),
            counted,
            waited: started.elapsed(),
        },
    );
}

async fn interrupted(signal: Option<&mut Signal>) {
    if let Some(signal) = signal
        && signal.recv().await.is_some()
    {
        return;
    }
    std::future::pending::<()>().await;
}

async fn expired(deadline: Option<Instant>) {
    match deadline {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// After the first Ctrl+C the CLI writes its output; a second one exits
/// 130 at once, even when stdout blocks. A thread with its own runtime
/// listens, since the command's runtime is busy writing.
fn exit_on_next_interrupt() {
    let (ready, listening) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("stackly-interrupt".to_owned())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                let _ = ready.send(());
                return;
            };
            runtime.block_on(async move {
                let next = signal(SignalKind::interrupt());
                let _ = ready.send(());
                if let Ok(mut next) = next
                    && next.recv().await.is_some()
                {
                    std::process::exit(i32::from(EXIT_INTERRUPTED));
                }
            });
        });
    if spawned.is_ok() {
        let _ = listening.recv_timeout(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::codes::{cli, operation};
    use stackly_protocol::operation::{
        OperationFailureInfo, OperationKind, OperationSummary, OperationTriggerInfo, RecoveryCause,
        RecoveryHint, RecoveryInfo, TriggerOrigin,
    };
    use stackly_protocol::{ErrorBody, Revision, Timestamp};

    use super::*;

    const ID: Uuid = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d0e);

    fn detail(state: OperationState, with_error: bool) -> OperationDetail {
        let error = ErrorBody::new(&operation::INTERRUPTED, "the daemon stopped in the middle");
        OperationDetail {
            operation: OperationSummary {
                id: ID,
                kind: OperationKind::StartInstance,
                resource: None,
                state,
                stage: None,
                progress: OperationProgress::Indeterminate,
                waiting_on: None,
                cancel_requested_at: None,
                point_of_no_return_at: None,
                failure: (with_error && state == OperationState::Failed).then(|| {
                    OperationFailureInfo {
                        reason: "interrupted".to_owned(),
                        retryable: false,
                        error: error.clone(),
                    }
                }),
                recovery: (state == OperationState::NeedsRecovery).then(|| RecoveryInfo {
                    hint: RecoveryHint::Resume,
                    cause: RecoveryCause::Interrupted,
                    error: with_error.then(|| error.clone()),
                    actions: Vec::new(),
                }),
                result: None,
                trigger: OperationTriggerInfo {
                    origin: TriggerOrigin::User,
                    client: None,
                },
                created_at: Timestamp::UNIX_EPOCH,
                started_at: None,
                updated_at: Timestamp::UNIX_EPOCH,
                revision: Revision::new(1).unwrap(),
            },
            steps: Vec::new(),
            created: Vec::new(),
        }
    }

    fn code(failure: Option<CliFailure>) -> Option<(String, u8)> {
        failure.map(|failure| (failure.body.code().to_owned(), failure.exit_code()))
    }

    #[test]
    fn observe_maps_each_end_state_to_its_exit() {
        let cases = [
            (OperationState::Succeeded, true, None),
            (
                OperationState::Failed,
                true,
                Some(("operation.interrupted", 7)),
            ),
            (
                OperationState::Failed,
                false,
                Some(("cli.operation_failed", 7)),
            ),
            (
                OperationState::Cancelled,
                false,
                Some(("cli.operation_cancelled", 9)),
            ),
            (
                OperationState::NeedsRecovery,
                true,
                Some(("operation.interrupted", 7)),
            ),
            (
                OperationState::NeedsRecovery,
                false,
                Some(("cli.operation_needs_recovery", 7)),
            ),
        ];
        for (state, with_error, expected) in cases {
            assert_eq!(
                code(settle(WaitMode::Observe, &detail(state, with_error))),
                expected.map(|(code, exit)| (code.to_owned(), exit)),
                "{state:?} {with_error}"
            );
        }
    }

    #[test]
    fn cancel_exits_0_only_for_cancelled_and_reports_other_ends_as_not_applied() {
        assert_eq!(
            settle(WaitMode::Cancel, &detail(OperationState::Cancelled, false)),
            None
        );
        for state in [OperationState::Succeeded, OperationState::Failed] {
            let failure = settle(WaitMode::Cancel, &detail(state, true)).unwrap();
            assert!(failure.body.is(&cli::CANCEL_NOT_APPLIED));
            assert_eq!(
                failure.body.detail("state"),
                Some(wire_name(&state).as_str())
            );
            assert_eq!(failure.body.operation_id(), Some(ID));
            assert_eq!(failure.exit_code(), 3);
        }
        assert_eq!(
            code(settle(
                WaitMode::Cancel,
                &detail(OperationState::NeedsRecovery, false)
            )),
            Some(("cli.operation_needs_recovery".to_owned(), 7))
        );
    }

    #[test]
    fn only_final_states_and_needs_recovery_end_the_wait() {
        let ends: Vec<OperationState> = OperationState::ALL
            .into_iter()
            .filter(|state| settled(*state))
            .collect();
        assert_eq!(
            ends,
            [
                OperationState::Succeeded,
                OperationState::Failed,
                OperationState::Cancelled,
                OperationState::NeedsRecovery,
            ]
        );
    }

    #[test]
    fn the_wait_codes_split_into_reconnect_retry_and_end() {
        let body = |spec| CliFailure::local(ErrorBody::new(spec, "x"));
        for spec in [
            &ipc::CONNECTION_CLOSED,
            &ipc::ENDPOINT_MISSING,
            &ipc::CONNECTION_REFUSED,
        ] {
            assert!(breaks_connection(&body(spec)) && !retried(&body(spec)));
        }
        for spec in [
            &ipc::CALL_TIMEOUT,
            &daemon::STARTING,
            &daemon::SHUTTING_DOWN,
        ] {
            assert!(retried(&body(spec)) && !breaks_connection(&body(spec)));
        }
        for spec in [
            &stackly_protocol::codes::resource::NOT_FOUND,
            &ipc::ENDPOINT_UNTRUSTED,
        ] {
            assert!(!retried(&body(spec)) && !breaks_connection(&body(spec)));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_retry_budget_doubles_to_two_seconds_and_ends_after_thirty() {
        let mut trouble = Trouble::default();
        let started = Instant::now();
        let mut waits = Vec::new();
        loop {
            let before = Instant::now();
            match trouble.pause(ID, None).await {
                Ok(()) => waits.push(before.elapsed().as_millis()),
                Err(failure) => {
                    assert!(failure.body.is(&cli::WAIT_DISCONNECTED));
                    assert_eq!(failure.body.detail("state"), Some("unknown"));
                    assert_eq!(failure.exit_code(), 6);
                    break;
                }
            }
        }
        assert_eq!(&waits[..5], [250, 500, 1000, 2000, 2000]);
        assert_eq!(started.elapsed(), RECONNECT_BUDGET);
    }
}
