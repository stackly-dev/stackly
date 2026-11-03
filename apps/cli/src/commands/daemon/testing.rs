//! Scripted stand-ins for the tests of the daemon commands: a daemon whose
//! answers depend on how long the test has waited (tokio's clock is
//! paused), a launch record that changes with time, Ctrl+C at a chosen
//! moment, and the setup that wires them to `FakeBackgroundAgent`.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_protocol::codes::ipc;
use stackly_protocol::daemon::{DaemonInfo, DaemonRoots, LaunchContextKind, ShutdownMode};
use stackly_protocol::restart::{RestartPlanView, RestartPreview};
use stackly_protocol::{CONTRACT_DIGEST, ErrorBody, ProtocolVersion};
use stackly_testkit::{FakeBackgroundAgent, FakeClock};
use tokio::time::Instant;
use uuid::Uuid;

use super::agent::AgentEnv;
use super::plan::PlanPrompt;
use super::wait::{
    AgentOffPreview, AgentOffReply, BoxFuture, DaemonProbe, Interrupt, LaunchRecordSource,
    PlanWatch, Reachable, RestartHandover, RestartRequest, Shutdown,
};
use crate::failure::CliFailure;

/// The bundle executable the tests run from.
pub const EXECUTABLE: &str = "/Applications/Stackly.app/Contents/MacOS/stackly-cli";

/// A daemon run id from a small number.
pub fn run(n: u128) -> Uuid {
    Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_0000 + n)
}

/// The failure of a socket that is not there.
pub fn missing() -> CliFailure {
    CliFailure::daemon(
        ErrorBody::new(&ipc::ENDPOINT_MISSING, "nothing at the socket"),
        None,
    )
}

/// A `daemon.get_info` reply of run `id`.
pub fn info(id: Uuid) -> DaemonInfo {
    DaemonInfo {
        version: "0.1.0".to_owned(),
        protocol: ProtocolVersion::CURRENT,
        contract_digest: CONTRACT_DIGEST.to_owned(),
        server_instance_id: id,
        pid: 4242,
        uid: Some(501),
        started_at: "2026-10-18T03:04:05.000Z"
            .parse()
            .expect("a fixed time parses"),
        roots: DaemonRoots {
            data: "/d".to_owned(),
            logs: "/l".to_owned(),
            cache: "/c".to_owned(),
            runtime: "/r".to_owned(),
            root_override: false,
        },
        socket_path: "/r/stacklyd.sock".to_owned(),
        registry_schema: Some(2),
        secret_store: None,
        launch: None,
        restart: None,
        session: None,
    }
}

type AttemptFn = dyn Fn(usize, Duration) -> Result<Reachable, CliFailure> + Send + Sync;

/// A daemon scripted by the number of attempts and the time since the probe
/// was made.
pub struct ScriptedProbe {
    started: Instant,
    attempts: Mutex<usize>,
    attempt: Box<AttemptFn>,
    shutdown: Mutex<Option<Result<Shutdown, CliFailure>>>,
    shutdown_modes: Mutex<Vec<ShutdownMode>>,
    previews: Mutex<VecDeque<Result<RestartPreview, CliFailure>>>,
    restarts: Mutex<VecDeque<Result<RestartHandover, CliFailure>>>,
    requests: Mutex<Vec<RestartRequest>>,
    watch: Mutex<Option<Result<PlanWatch, CliFailure>>>,
    watch_hangs: bool,
    restart_hangs: bool,
    infos: Mutex<Option<DaemonInfo>>,
    off_preview: Mutex<Option<Result<AgentOffPreview, CliFailure>>>,
    off_reply: Mutex<Option<Result<AgentOffReply, CliFailure>>>,
    off_keys: Mutex<Vec<String>>,
}

/// A preview with no instances for a launchd daemon.
pub fn empty_preview() -> RestartPreview {
    RestartPreview {
        digest: "0".repeat(64),
        launch_context: LaunchContextKind::Launchd,
        supervisor_available: true,
        instances: Vec::new(),
        blocking_operations: Vec::new(),
    }
}

impl ScriptedProbe {
    /// A probe whose `n`th attempt, `elapsed` after the probe was made,
    /// answers `attempt(n, elapsed)`.
    pub fn new(
        attempt: impl Fn(usize, Duration) -> Result<Reachable, CliFailure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            started: Instant::now(),
            attempts: Mutex::new(0),
            attempt: Box::new(attempt),
            shutdown: Mutex::new(None),
            shutdown_modes: Mutex::new(Vec::new()),
            previews: Mutex::new(VecDeque::new()),
            restarts: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
            watch: Mutex::new(None),
            watch_hangs: false,
            restart_hangs: false,
            infos: Mutex::new(None),
            off_preview: Mutex::new(None),
            off_reply: Mutex::new(None),
            off_keys: Mutex::new(Vec::new()),
        }
    }

    /// The answer to the Turn Off preview; with none, the probe follows its
    /// attempts.
    #[must_use]
    pub fn on_agent_off_preview(self, result: Result<AgentOffPreview, CliFailure>) -> Self {
        *self
            .off_preview
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(result);
        self
    }

    /// The answer to the `agent_off` request.
    #[must_use]
    pub fn on_agent_off(self, result: Result<AgentOffReply, CliFailure>) -> Self {
        *self
            .off_reply
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(result);
        self
    }

    /// The idempotency keys of the `agent_off` requests so far.
    pub fn agent_off_keys(&self) -> Vec<String> {
        self.off_keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The next preview the probe answers; with none queued, an empty one.
    #[must_use]
    pub fn on_preview(self, result: Result<RestartPreview, CliFailure>) -> Self {
        self.previews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(result);
        self
    }

    /// The next answer to a restart request; with none queued, the
    /// shutdown answer without a plan.
    #[must_use]
    pub fn on_restart(self, result: Result<RestartHandover, CliFailure>) -> Self {
        self.restarts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(result);
        self
    }

    /// What following the plan sees.
    #[must_use]
    pub fn on_watch(self, result: Result<PlanWatch, CliFailure>) -> Self {
        *self.watch.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        self
    }

    /// Following the plan never ends, so only Ctrl+C or the limit does.
    #[must_use]
    pub fn hang_on_watch(mut self) -> Self {
        self.watch_hangs = true;
        self
    }

    /// The restart request never gets an answer, so only Ctrl+C ends it.
    #[must_use]
    pub fn hang_on_restart(mut self) -> Self {
        self.restart_hangs = true;
        self
    }

    /// The `daemon.get_info` of the new daemon, with its `restart` field.
    #[must_use]
    pub fn with_restart(self, plan: RestartPlanView) -> Self {
        let mut info = info(run(2));
        info.restart = Some(Box::new(plan));
        *self.infos.lock().unwrap_or_else(PoisonError::into_inner) = Some(info);
        self
    }

    /// The restart requests so far.
    pub fn requests(&self) -> Vec<RestartRequest> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A probe with no daemon.
    pub fn no_daemon() -> Self {
        Self::new(|_, _| Err(missing()))
    }

    /// A probe where run `id` always answers.
    pub fn running(id: Uuid) -> Self {
        Self::new(move |_, _| {
            Ok(Reachable {
                server_instance_id: id,
            })
        })
    }

    /// The answer to the shutdown request.
    #[must_use]
    pub fn on_shutdown(self, result: Result<Shutdown, CliFailure>) -> Self {
        *self.shutdown.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        self
    }

    /// The modes of the shutdown requests so far.
    pub fn shutdown_modes(&self) -> Vec<ShutdownMode> {
        self.shutdown_modes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl DaemonProbe for ScriptedProbe {
    fn attempt(&self) -> BoxFuture<'_, Result<Reachable, CliFailure>> {
        Box::pin(async move {
            let n = {
                let mut attempts = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
                *attempts += 1;
                *attempts - 1
            };
            (self.attempt)(n, self.started.elapsed())
        })
    }

    fn info(&self) -> BoxFuture<'_, Result<DaemonInfo, CliFailure>> {
        Box::pin(async move {
            let reachable = (self.attempt)(usize::MAX, self.started.elapsed())?;
            let scripted = self
                .infos
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            Ok(scripted.map_or_else(
                || info(reachable.server_instance_id),
                |mut scripted| {
                    scripted.server_instance_id = reachable.server_instance_id;
                    scripted
                },
            ))
        })
    }

    fn shutdown(
        &self,
        mode: ShutdownMode,
        _eof_limit: Duration,
    ) -> BoxFuture<'_, Result<Shutdown, CliFailure>> {
        Box::pin(async move {
            self.shutdown_modes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(mode);
            self.shutdown
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .unwrap_or_else(|| Err(missing()))
        })
    }

    fn restart_preview(&self) -> BoxFuture<'_, Result<RestartPreview, CliFailure>> {
        Box::pin(async move {
            if let Some(result) = self
                .previews
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
            {
                return result;
            }
            // A probe with a scripted shutdown has a daemon to preview; any
            // other follows its attempts, so no daemon fails the call.
            if self
                .shutdown
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
            {
                return Ok(empty_preview());
            }
            (self.attempt)(usize::MAX, self.started.elapsed()).map(|_| empty_preview())
        })
    }

    fn restart(
        &self,
        request: &RestartRequest,
        _eof_limit: Duration,
    ) -> BoxFuture<'_, Result<RestartHandover, CliFailure>> {
        let request = request.clone();
        Box::pin(async move {
            self.shutdown_modes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(ShutdownMode::Restart);
            self.requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(request);
            if self.restart_hangs {
                std::future::pending::<()>().await;
            }
            if let Some(result) = self
                .restarts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
            {
                return result;
            }
            self.shutdown
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .unwrap_or_else(|| Err(missing()))
                .map(|shutdown| RestartHandover {
                    server_instance_id: shutdown.server_instance_id,
                    plan_id: None,
                    plan: None,
                    eof: shutdown.eof,
                })
        })
    }

    fn agent_off_preview(&self) -> BoxFuture<'_, Result<AgentOffPreview, CliFailure>> {
        Box::pin(async move {
            if let Some(result) = self
                .off_preview
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
            {
                return result;
            }
            Err((self.attempt)(usize::MAX, self.started.elapsed())
                .err()
                .unwrap_or_else(missing))
        })
    }

    fn agent_off(
        &self,
        idempotency_key: &str,
        _eof_limit: Duration,
    ) -> BoxFuture<'_, Result<AgentOffReply, CliFailure>> {
        let key = idempotency_key.to_owned();
        Box::pin(async move {
            self.off_keys
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(key);
            self.off_reply
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .unwrap_or_else(|| Err(missing()))
        })
    }

    fn watch_plan(
        &self,
        _plan_id: Uuid,
        _limit: Duration,
    ) -> BoxFuture<'_, Result<PlanWatch, CliFailure>> {
        Box::pin(async move {
            if self.watch_hangs {
                std::future::pending::<()>().await;
            }
            self.watch
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .unwrap_or(Ok(PlanWatch {
                    plan: None,
                    settled: false,
                }))
        })
    }
}

/// A [`PlanPrompt`] with scripted answers.
pub struct ScriptedPrompt {
    can_ask: bool,
    answers: VecDeque<bool>,
    /// The digests of the previews shown.
    pub shown: Vec<String>,
    /// How many questions were asked, and whether each was an `again`.
    pub asked: Vec<bool>,
}

impl ScriptedPrompt {
    /// A prompt where a person answers `answers` in turn.
    pub fn answering(answers: &[bool]) -> Self {
        Self {
            can_ask: true,
            answers: answers.iter().copied().collect(),
            shown: Vec::new(),
            asked: Vec::new(),
        }
    }

    /// A prompt where nobody can answer and `--yes` was not given.
    pub fn nobody() -> Self {
        Self {
            can_ask: false,
            ..Self::answering(&[])
        }
    }

    /// A prompt where `--yes` was given, on a terminal or not.
    pub fn yes(can_ask: bool) -> Self {
        Self {
            can_ask,
            // Marks "yes": `confirm(false)` passes without asking.
            answers: VecDeque::new(),
            ..Self::answering(&[])
        }
        .with_yes()
    }

    fn with_yes(mut self) -> Self {
        self.asked.clear();
        self.shown.push(String::from("yes"));
        self
    }

    fn approved(&self) -> bool {
        self.shown.first().is_some_and(|first| first == "yes")
    }
}

impl PlanPrompt for ScriptedPrompt {
    fn can_ask(&self) -> bool {
        self.can_ask
    }

    fn show(&mut self, preview: &RestartPreview) {
        self.shown.push(preview.digest.clone());
    }

    fn confirm(&mut self, again: bool) -> Result<(), CliFailure> {
        if !again && self.approved() {
            return Ok(());
        }
        if !self.can_ask {
            return Err(CliFailure::confirmation_required("--yes"));
        }
        self.asked.push(again);
        match self.answers.pop_front() {
            Some(true) => Ok(()),
            _ => Err(CliFailure::confirmation_declined()),
        }
    }
}

/// A launch record whose `lastLaunchedAt` depends on the time since the
/// test began.
pub struct ScriptedRecords {
    started: Instant,
    exists: bool,
    last: Box<dyn Fn(Duration) -> Option<jiff::Timestamp> + Send + Sync>,
}

impl ScriptedRecords {
    /// A record that exists exactly when `last` has a time.
    pub fn new(last: impl Fn(Duration) -> Option<jiff::Timestamp> + Send + Sync + 'static) -> Self {
        Self {
            started: Instant::now(),
            exists: false,
            last: Box::new(last),
        }
    }

    /// No launch record at all.
    pub fn none() -> Self {
        Self::new(|_| None)
    }

    /// A record that exists but holds no time.
    #[must_use]
    pub fn unreadable() -> Self {
        Self {
            exists: true,
            ..Self::none()
        }
    }
}

impl LaunchRecordSource for ScriptedRecords {
    fn exists(&self) -> bool {
        self.exists || (self.last)(self.started.elapsed()).is_some()
    }

    fn last_launched_at(&self) -> Option<jiff::Timestamp> {
        (self.last)(self.started.elapsed())
    }
}

/// Ctrl+C that arrives after a chosen time, or never.
pub struct AfterInterrupt {
    started: Instant,
    after: Option<Duration>,
}

impl AfterInterrupt {
    /// Ctrl+C `after` the start; `None` for never.
    pub fn new(after: Option<Duration>) -> Self {
        Self {
            started: Instant::now(),
            after,
        }
    }

    /// No Ctrl+C.
    pub fn never() -> Self {
        Self::new(None)
    }
}

impl Interrupt for AfterInterrupt {
    fn pending(&mut self) -> bool {
        self.after
            .is_some_and(|after| self.started.elapsed() >= after)
    }

    fn wait(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            match self.after {
                Some(after) => tokio::time::sleep_until(self.started + after).await,
                None => std::future::pending().await,
            }
        })
    }
}

/// Everything a command test runs against.
pub struct Rig {
    /// The agent port, to script and to read the call log from.
    pub fake: FakeBackgroundAgent,
    /// The wall clock.
    pub clock: FakeClock,
    /// What the commands see.
    pub env: AgentEnv,
}

impl Rig {
    /// A rig for the bundle executable, with launch record `records`.
    pub fn new(records: ScriptedRecords, root_override: bool) -> Self {
        Self::with_fake(FakeBackgroundAgent::new(), records, root_override)
    }

    /// A rig around `fake`, for a launch record that depends on its calls.
    pub fn with_fake(
        fake: FakeBackgroundAgent,
        records: ScriptedRecords,
        root_override: bool,
    ) -> Self {
        let clock = FakeClock::new();
        let env = AgentEnv::new(
            Arc::new(fake.clone()),
            PathBuf::from(EXECUTABLE),
            Arc::new(records),
            root_override,
            Arc::new(clock.clone()),
        )
        .inline();
        Self { fake, clock, env }
    }

    /// A rig without a launch record or a root override.
    pub fn plain() -> Self {
        Self::new(ScriptedRecords::none(), false)
    }
}
