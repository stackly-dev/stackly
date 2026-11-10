//! Waiting for the daemon to appear or disappear (spec 0015, *Perintah CLI*
//! and *Penantian restart*), and the three small traits the daemon commands
//! decide through: [`DaemonProbe`] (connect, handshake, and ask the daemon
//! to stop), [`LaunchRecordSource`] (the launch record the daemon writes),
//! and [`Interrupt`] (Ctrl+C). The live versions sit at the end of the file;
//! tests use scripted ones and a paused tokio clock.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use stackly_ipc::EventStream;
use stackly_protocol::codes::{daemon, ipc};
use stackly_protocol::daemon::{DaemonInfo, ShutdownAccepted, ShutdownMode, ShutdownParams};
use stackly_protocol::events::{EventKind, SubscribeParams};
use stackly_protocol::instance::{ListInstancesParams, StartupPolicyKind};
use stackly_protocol::json::DecodedFrame;
use stackly_protocol::restart::{PlanState, RestartPlanView, RestartPreview};
use stackly_protocol::stop_all::StopAllPreview;
use stackly_protocol::{Call, Reply};
use tokio::time::Instant;
use uuid::Uuid;

use crate::failure::CliFailure;
use crate::output::stderr::StderrSink;
use crate::session::{CallSpec, Connector, HANDSHAKE_METHOD, RequestTimeout, Session};

/// How often the daemon is tried while waiting for it.
pub const DAEMON_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How long `install`, `start`, and `restart` wait for a daemon to answer.
pub const DAEMON_START_WAIT: Duration = Duration::from_secs(30);
/// How long `stop` and `restart` wait for the old daemon to go.
pub const DAEMON_STOP_WAIT: Duration = Duration::from_secs(20);
/// How long `restart` follows a restart plan, from the preview to the last
/// instance (spec 0030).
pub const DAEMON_RESTORE_WAIT: Duration = Duration::from_secs(600);
/// How long past the throttle interval `restart` lets launchd take to start
/// the daemon again before it starts it through macOS.
pub const RESPAWN_GRACE: Duration = Duration::from_secs(5);
/// A launch record younger than this means the daemon may be starting or
/// waiting out its throttle interval, so `start` waits before it relaunches.
pub const RECENT_LAUNCH_WINDOW: Duration = Duration::from_secs(15);

/// A boxed future, so the traits below can be used as `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A daemon that answered a handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reachable {
    /// The daemon run that answered.
    pub server_instance_id: Uuid,
}

/// How a shutdown request went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shutdown {
    /// The daemon run that was asked to stop.
    pub server_instance_id: Uuid,
    /// Whether the daemon closed the connection within the limit.
    pub eof: bool,
}

/// What `daemon.shutdown { mode: restart }` is asked with (spec 0030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartRequest {
    /// The `digest` of the preview the person confirmed.
    pub expected_plan_digest: Option<String>,
    /// Restart although an operation that cannot resume still runs.
    pub force: bool,
    /// The request's idempotency key, which a restart needs.
    pub idempotency_key: String,
}

/// How a restart request went on the old daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartHandover {
    /// The daemon run that accepted the restart.
    pub server_instance_id: Uuid,
    /// The plan the daemon wrote, or `None` when it wrote none.
    pub plan_id: Option<Uuid>,
    /// The last state of that plan the old daemon published.
    pub plan: Option<RestartPlanView>,
    /// Whether the old daemon closed the connection within the limit.
    pub eof: bool,
}

/// What the Turn Off question needs (spec 0031, *Matikan agent*, step 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOffPreview {
    /// What `instance.preview_stop_all` found.
    pub preview: StopAllPreview,
    /// How many instances have the policy `at_login`.
    pub at_login: usize,
}

/// How an `agent_off` request went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOffReply {
    /// The daemon's answer.
    pub accepted: ShutdownAccepted,
    /// Whether the daemon closed the connection within the limit; read only
    /// when it said `exiting`.
    pub eof: bool,
}

/// What [`DaemonProbe::watch_plan`] saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWatch {
    /// The plan as last seen, or `None` when the daemon never showed it.
    pub plan: Option<RestartPlanView>,
    /// Whether the plan reached a final state before the limit.
    pub settled: bool,
}

/// Whether `state` is final: no daemon changes the plan any more.
#[must_use]
pub const fn plan_is_final(state: PlanState) -> bool {
    matches!(
        state,
        PlanState::Finished | PlanState::Aborted | PlanState::Expired
    )
}

/// What the commands need from the daemon.
pub trait DaemonProbe: Send + Sync {
    /// One connect plus handshake, with nothing retried.
    fn attempt(&self) -> BoxFuture<'_, Result<Reachable, CliFailure>>;

    /// Connects and reads `daemon.get_info`.
    fn info(&self) -> BoxFuture<'_, Result<DaemonInfo, CliFailure>>;

    /// Sends `daemon.shutdown { mode }` and reads the same connection until
    /// the daemon closes it, for up to `eof_limit`. An answer of
    /// `daemon.shutting_down` counts as accepted.
    fn shutdown(
        &self,
        mode: ShutdownMode,
        eof_limit: Duration,
    ) -> BoxFuture<'_, Result<Shutdown, CliFailure>>;

    /// Reads `daemon.restart_preview` (spec 0030).
    fn restart_preview(&self) -> BoxFuture<'_, Result<RestartPreview, CliFailure>>;

    /// Sends `daemon.shutdown { mode: restart }` under the interactive
    /// time limit, then reads the same connection until the daemon closes
    /// it or publishes an `aborted` plan, for up to `eof_limit`. An answer
    /// of `daemon.shutting_down` counts as accepted, without a plan.
    fn restart(
        &self,
        request: &RestartRequest,
        eof_limit: Duration,
    ) -> BoxFuture<'_, Result<RestartHandover, CliFailure>>;

    /// Reads `instance.preview_stop_all` and counts the `at_login`
    /// instances of `instance.list` (spec 0031).
    fn agent_off_preview(&self) -> BoxFuture<'_, Result<AgentOffPreview, CliFailure>>;

    /// Sends `daemon.shutdown { mode: agent_off }` under the interactive
    /// time limit. When the daemon answers `exiting`, reads the connection
    /// until the daemon closes it, for up to `eof_limit`.
    fn agent_off(
        &self,
        idempotency_key: &str,
        eof_limit: Duration,
    ) -> BoxFuture<'_, Result<AgentOffReply, CliFailure>>;

    /// Follows plan `plan_id` on the daemon that runs now, connecting
    /// again when the connection ends, until the plan is final or `limit`
    /// passes.
    fn watch_plan(
        &self,
        plan_id: Uuid,
        limit: Duration,
    ) -> BoxFuture<'_, Result<PlanWatch, CliFailure>>;
}

/// The launch record the daemon writes under launchd.
pub trait LaunchRecordSource: Send + Sync {
    /// Whether the file exists and is a plain file.
    fn exists(&self) -> bool;

    /// `lastLaunchedAt`, or `None` when the file is missing, unreadable,
    /// above 4 KiB, or holds no valid RFC 3339 time.
    fn last_launched_at(&self) -> Option<jiff::Timestamp>;

    /// `bundlePath`, the app bundle launchd last started the daemon from,
    /// or `None` when the file is missing, unreadable, above 4 KiB, or
    /// names no bundle.
    fn bundle_path(&self) -> Option<PathBuf> {
        None
    }
}

/// Ctrl+C, as the commands see it.
pub trait Interrupt: Send {
    /// Whether Ctrl+C arrived since the start; it stays true.
    fn pending(&mut self) -> bool;

    /// Resolves once Ctrl+C arrived.
    fn wait(&mut self) -> BoxFuture<'_, ()>;

    /// Starts catching Ctrl+C, for an interrupt that leaves the default
    /// behavior (the shell ends the command, exit 130) while the command
    /// reads an answer it cannot give up. Catching goes on to the end.
    fn arm(&mut self) {}
}

/// Whether `failure` only means the daemon is not up yet: no socket, a
/// refused or closed connection, a handshake that ran out of time, a daemon
/// that still starts or already stops. Anything else (an untrusted
/// endpoint, a refused peer, another major) stops a wait.
#[must_use]
pub fn not_up_yet(failure: &CliFailure) -> bool {
    let body = &failure.body;
    body.is(&ipc::ENDPOINT_MISSING)
        || body.is(&ipc::CONNECTION_REFUSED)
        || body.is(&ipc::CONNECTION_CLOSED)
        || (body.is(&ipc::CALL_TIMEOUT) && body.detail("method") == Some(HANDSHAKE_METHOD))
        || body.is(&daemon::STARTING)
        || body.is(&daemon::SHUTTING_DOWN)
}

/// Whether `failure` says no daemon listens: no socket, or a refused
/// connection.
#[must_use]
pub fn daemon_absent(failure: &CliFailure) -> bool {
    failure.body.is(&ipc::ENDPOINT_MISSING) || failure.body.is(&ipc::CONNECTION_REFUSED)
}

/// How a wait for a daemon ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitEnd {
    /// A daemon that `accept` took answered.
    Up(Reachable),
    /// The limit passed.
    TimedOut,
}

/// Sleeps for `duration`, or fails `cli.interrupted` when Ctrl+C comes
/// first.
///
/// # Errors
///
/// `cli.interrupted` (exit 130) with `details.state` `waiting_for_daemon`.
pub async fn sleep_or_interrupt(
    interrupt: &mut dyn Interrupt,
    duration: Duration,
) -> Result<(), CliFailure> {
    tokio::select! {
        () = tokio::time::sleep(duration) => Ok(()),
        () = interrupt.wait() => Err(CliFailure::interrupted_at("waiting_for_daemon")),
    }
}

/// Tries the daemon every [`DAEMON_POLL_INTERVAL`] until `accept` takes an
/// answer, the `deadline` passes, or an error that is not "not up yet"
/// comes.
///
/// # Errors
///
/// The failure of an attempt that is not [`not_up_yet`], and
/// `cli.interrupted` on Ctrl+C.
pub async fn wait_for_daemon(
    probe: &dyn DaemonProbe,
    interrupt: &mut dyn Interrupt,
    deadline: Instant,
    accept: impl Fn(&Reachable) -> bool + Send,
) -> Result<WaitEnd, CliFailure> {
    loop {
        match probe.attempt().await {
            Ok(reachable) if accept(&reachable) => return Ok(WaitEnd::Up(reachable)),
            Ok(_) => {}
            Err(failure) if not_up_yet(&failure) => {}
            Err(failure) => return Err(failure),
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(WaitEnd::TimedOut);
        }
        sleep_or_interrupt(interrupt, DAEMON_POLL_INTERVAL.min(deadline - now)).await?;
    }
}

/// The live [`DaemonProbe`]: a fresh connection to the daemon socket for
/// each call, with no `-v` lines.
#[derive(Debug, Clone)]
pub struct LiveProbe {
    connector: Connector,
}

impl LiveProbe {
    /// A probe that connects through `connector`.
    #[must_use]
    pub const fn new(connector: Connector) -> Self {
        Self { connector }
    }

    async fn session(&self) -> Result<Session, CliFailure> {
        let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
        Session::connect(&self.connector, &mut sink, false).await
    }
}

impl DaemonProbe for LiveProbe {
    fn attempt(&self) -> BoxFuture<'_, Result<Reachable, CliFailure>> {
        Box::pin(async move {
            let session = self.session().await?;
            Ok(Reachable {
                server_instance_id: session.accepted().server_instance_id,
            })
        })
    }

    fn info(&self) -> BoxFuture<'_, Result<DaemonInfo, CliFailure>> {
        Box::pin(async move {
            let session = self.session().await?;
            let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
            match session
                .call(&Call::DaemonGetInfo, &CallSpec::control(), &mut sink, false)
                .await?
            {
                Reply::DaemonGetInfo(info) => Ok(info),
                _ => Err(CliFailure::unexpected_reply(None)),
            }
        })
    }

    fn shutdown(
        &self,
        mode: ShutdownMode,
        eof_limit: Duration,
    ) -> BoxFuture<'_, Result<Shutdown, CliFailure>> {
        Box::pin(async move {
            let session = self.session().await?;
            let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
            // The subscription is how the connection is read to its end:
            // the stream ends when the daemon closes the connection.
            let subscribe = SubscribeParams {
                kinds: vec!["daemon.".to_owned()],
                ..SubscribeParams::default()
            };
            let mut stream = match session.subscribe(subscribe, &mut sink, false).await {
                Ok((_, stream)) => Some(stream),
                Err(failure) if failure.body.is(&daemon::SHUTTING_DOWN) => None,
                Err(failure) => return Err(failure),
            };
            let call = Call::DaemonShutdown(ShutdownParams::new(mode));
            let server_instance_id = match session
                .call_once(call, &CallSpec::control(), &mut sink, false)
                .await
            {
                Ok(Reply::DaemonShutdown(accepted)) => accepted.server_instance_id,
                Ok(_) => return Err(CliFailure::unexpected_reply(None)),
                Err(failure) if failure.body.is(&daemon::SHUTTING_DOWN) => {
                    session.accepted().server_instance_id
                }
                Err(failure) => return Err(failure),
            };
            let eof = match stream.as_mut() {
                Some(stream) => tokio::time::timeout(eof_limit, read_to_end(stream))
                    .await
                    .is_ok(),
                None => tokio::time::timeout(eof_limit, self.until_absent())
                    .await
                    .is_ok(),
            };
            Ok(Shutdown {
                server_instance_id,
                eof,
            })
        })
    }

    fn restart_preview(&self) -> BoxFuture<'_, Result<RestartPreview, CliFailure>> {
        Box::pin(async move {
            let session = self.session().await?;
            let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
            match session
                .call(
                    &Call::DaemonRestartPreview,
                    &CallSpec::control(),
                    &mut sink,
                    false,
                )
                .await?
            {
                Reply::DaemonRestartPreview(preview) => Ok(preview),
                _ => Err(CliFailure::unexpected_reply(None)),
            }
        })
    }

    fn restart(
        &self,
        request: &RestartRequest,
        eof_limit: Duration,
    ) -> BoxFuture<'_, Result<RestartHandover, CliFailure>> {
        let request = request.clone();
        Box::pin(async move {
            let session = self.session().await?;
            let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
            let subscribe = SubscribeParams {
                kinds: vec!["daemon.".to_owned()],
                ..SubscribeParams::default()
            };
            let mut stream = match session.subscribe(subscribe, &mut sink, false).await {
                Ok((_, stream)) => Some(stream),
                Err(failure) if failure.body.is(&daemon::SHUTTING_DOWN) => None,
                Err(failure) => return Err(failure),
            };
            let params = ShutdownParams {
                mode: ShutdownMode::Restart,
                expected_plan_digest: request.expected_plan_digest,
                force: request.force,
            };
            let spec = CallSpec {
                timeout: RequestTimeout::Interactive,
                idempotency_key: Some(request.idempotency_key),
            };
            let accepted = match session
                .call_once(Call::DaemonShutdown(params), &spec, &mut sink, false)
                .await
            {
                Ok(Reply::DaemonShutdown(accepted)) => Some(accepted),
                Ok(_) => return Err(CliFailure::unexpected_reply(None)),
                Err(failure) if failure.body.is(&daemon::SHUTTING_DOWN) => None,
                Err(failure) => return Err(failure),
            };
            let server_instance_id = accepted.as_ref().map_or_else(
                || session.accepted().server_instance_id,
                |accepted| accepted.server_instance_id,
            );
            let plan_id = accepted.as_ref().and_then(|accepted| accepted.plan_id);
            let mut plan = None;
            let eof = match stream.as_mut() {
                Some(stream) => {
                    tokio::time::timeout(eof_limit, read_plan_to_end(stream, plan_id, &mut plan))
                        .await
                        .is_ok_and(|ended| ended)
                }
                None => tokio::time::timeout(eof_limit, self.until_absent())
                    .await
                    .is_ok(),
            };
            Ok(RestartHandover {
                server_instance_id,
                plan_id,
                plan,
                eof,
            })
        })
    }

    fn agent_off_preview(&self) -> BoxFuture<'_, Result<AgentOffPreview, CliFailure>> {
        Box::pin(async move {
            let session = self.session().await?;
            let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
            let preview = match session
                .call(
                    &Call::InstancePreviewStopAll,
                    &CallSpec::control(),
                    &mut sink,
                    false,
                )
                .await?
            {
                Reply::InstancePreviewStopAll(preview) => *preview,
                _ => return Err(CliFailure::unexpected_reply(None)),
            };
            let mut at_login = 0;
            let mut before = None;
            loop {
                let params = ListInstancesParams {
                    limit: None,
                    before: before.take(),
                };
                match session
                    .call(
                        &Call::InstanceList(params),
                        &CallSpec::control(),
                        &mut sink,
                        false,
                    )
                    .await?
                {
                    Reply::InstanceList(page) => {
                        at_login += page
                            .items
                            .iter()
                            .filter(|item| item.autostart.policy == StartupPolicyKind::AtLogin)
                            .count();
                        match page.next {
                            Some(next) => before = Some(next),
                            None => break,
                        }
                    }
                    _ => return Err(CliFailure::unexpected_reply(None)),
                }
            }
            Ok(AgentOffPreview { preview, at_login })
        })
    }

    fn agent_off(
        &self,
        idempotency_key: &str,
        eof_limit: Duration,
    ) -> BoxFuture<'_, Result<AgentOffReply, CliFailure>> {
        let key = idempotency_key.to_owned();
        Box::pin(async move {
            let session = self.session().await?;
            let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
            let subscribe = SubscribeParams {
                kinds: vec!["daemon.".to_owned()],
                ..SubscribeParams::default()
            };
            let mut stream = match session.subscribe(subscribe, &mut sink, false).await {
                Ok((_, stream)) => Some(stream),
                Err(failure) if failure.body.is(&daemon::SHUTTING_DOWN) => None,
                Err(failure) => return Err(failure),
            };
            let spec = CallSpec {
                timeout: RequestTimeout::Interactive,
                idempotency_key: Some(key),
            };
            let call = Call::DaemonShutdown(ShutdownParams::new(ShutdownMode::AgentOff));
            let accepted = match session.call_once(call, &spec, &mut sink, false).await {
                Ok(Reply::DaemonShutdown(accepted)) => accepted,
                Ok(_) => return Err(CliFailure::unexpected_reply(None)),
                Err(failure) => return Err(failure),
            };
            let eof = if accepted.exiting {
                match stream.as_mut() {
                    Some(stream) => tokio::time::timeout(eof_limit, read_to_end(stream))
                        .await
                        .is_ok(),
                    None => tokio::time::timeout(eof_limit, self.until_absent())
                        .await
                        .is_ok(),
                }
            } else {
                false
            };
            Ok(AgentOffReply { accepted, eof })
        })
    }

    fn watch_plan(
        &self,
        plan_id: Uuid,
        limit: Duration,
    ) -> BoxFuture<'_, Result<PlanWatch, CliFailure>> {
        Box::pin(async move {
            let deadline = Instant::now() + limit;
            let mut last: Option<RestartPlanView> = None;
            loop {
                if Instant::now() >= deadline {
                    return Ok(PlanWatch {
                        plan: last,
                        settled: false,
                    });
                }
                let step = tokio::time::timeout_at(deadline, self.watch_once(plan_id, &mut last));
                match step.await {
                    Ok(Ok(true)) => {
                        return Ok(PlanWatch {
                            plan: last,
                            settled: true,
                        });
                    }
                    Ok(Ok(false)) => {}
                    Ok(Err(failure)) if not_up_yet(&failure) => {}
                    Ok(Err(failure)) => return Err(failure),
                    Err(_) => {
                        return Ok(PlanWatch {
                            plan: last,
                            settled: false,
                        });
                    }
                }
                tokio::time::sleep(DAEMON_POLL_INTERVAL).await;
            }
        })
    }
}

impl LiveProbe {
    /// One connection to the daemon: subscribes to the plan events, reads
    /// `daemon.get_info` for what happened before, then the events. True
    /// when the plan reached a final state; false when the connection
    /// ended first.
    async fn watch_once(
        &self,
        plan_id: Uuid,
        last: &mut Option<RestartPlanView>,
    ) -> Result<bool, CliFailure> {
        let session = self.session().await?;
        let mut sink = StderrSink::new(Box::new(std::io::sink()), false);
        let subscribe = SubscribeParams {
            kinds: vec!["daemon.restart_plan".to_owned()],
            ..SubscribeParams::default()
        };
        // Subscribing first and reading the info second leaves no gap.
        let (_, mut stream) = session.subscribe(subscribe, &mut sink, false).await?;
        let info = match session
            .call(&Call::DaemonGetInfo, &CallSpec::control(), &mut sink, false)
            .await?
        {
            Reply::DaemonGetInfo(info) => info,
            _ => return Err(CliFailure::unexpected_reply(None)),
        };
        if let Some(view) = info.restart.filter(|view| view.id == plan_id) {
            let settled = plan_is_final(view.state);
            *last = Some(*view);
            if settled {
                return Ok(true);
            }
        }
        loop {
            match stream.next().await {
                Ok(Some(DecodedFrame::Event(event))) => {
                    if let EventKind::DaemonRestartPlan(view) = event.kind
                        && view.id == plan_id
                    {
                        let settled = plan_is_final(view.state);
                        *last = Some(*view);
                        if settled {
                            return Ok(true);
                        }
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => return Ok(false),
            }
        }
    }
}

impl LiveProbe {
    /// Polls until no daemon listens, for a request that found the daemon
    /// already stopping and so has no connection to read to its end.
    async fn until_absent(&self) {
        loop {
            if let Err(failure) = self.attempt().await
                && daemon_absent(&failure)
            {
                return;
            }
            tokio::time::sleep(DAEMON_POLL_INTERVAL).await;
        }
    }
}

/// Reads `stream` until the daemon ends it, keeping the last state of plan
/// `plan_id` in `plan`. Returns false when the plan was `aborted`: the
/// daemon then keeps running, so waiting for its end would be in vain.
async fn read_plan_to_end(
    stream: &mut EventStream,
    plan_id: Option<Uuid>,
    plan: &mut Option<RestartPlanView>,
) -> bool {
    loop {
        match stream.next().await {
            Ok(Some(DecodedFrame::Event(event))) => {
                if let EventKind::DaemonRestartPlan(view) = event.kind
                    && Some(view.id) == plan_id
                {
                    let aborted = view.state == PlanState::Aborted;
                    *plan = Some(*view);
                    if aborted {
                        return false;
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return true,
        }
    }
}

/// Reads `stream` until the daemon ends it.
async fn read_to_end(stream: &mut EventStream) {
    loop {
        match stream.next().await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return,
        }
    }
}

/// The live [`LaunchRecordSource`]: the file `<data>/background-agent.json`
/// of the default root, or nothing when `STACKLY_ROOT` is set.
#[derive(Debug, Clone)]
pub struct FileRecords {
    path: Option<PathBuf>,
}

impl FileRecords {
    /// A source that reads `path`; `None` makes every read empty.
    #[must_use]
    pub const fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

/// The most bytes read from the launch record.
const RECORD_MAX_LEN: u64 = 4096;

impl LaunchRecordSource for FileRecords {
    fn exists(&self) -> bool {
        self.path
            .as_deref()
            .is_some_and(|path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file()))
    }

    fn last_launched_at(&self) -> Option<jiff::Timestamp> {
        self.record()?.get("lastLaunchedAt")?.as_str()?.parse().ok()
    }

    fn bundle_path(&self) -> Option<PathBuf> {
        self.record()?
            .get("bundlePath")?
            .as_str()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    }
}

impl FileRecords {
    /// The record as JSON, or `None` when the file is missing, not a plain
    /// file, above 4 KiB, or not JSON.
    fn record(&self) -> Option<serde_json::Value> {
        use std::io::Read;

        let path = self.path.as_deref()?;
        if !std::fs::symlink_metadata(path).ok()?.is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(RECORD_MAX_LEN + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > RECORD_MAX_LEN {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    }
}

/// The live [`Interrupt`]: SIGINT, caught from the moment it is installed,
/// so Ctrl+C is held back until the command looks at it.
#[cfg(unix)]
#[derive(Debug)]
pub struct SigInt {
    signal: tokio::signal::unix::Signal,
    hit: bool,
}

#[cfg(unix)]
impl SigInt {
    /// Starts catching SIGINT; call it inside the tokio runtime.
    ///
    /// # Errors
    ///
    /// The I/O error of the signal registration.
    pub fn install() -> std::io::Result<Self> {
        Ok(Self {
            signal: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
            hit: false,
        })
    }
}

#[cfg(unix)]
impl Interrupt for SigInt {
    fn pending(&mut self) -> bool {
        if !self.hit {
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            self.hit = self.signal.poll_recv(&mut cx).is_ready();
        }
        self.hit
    }

    fn wait(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if !self.hit {
                self.signal.recv().await;
                self.hit = true;
            }
        })
    }
}
