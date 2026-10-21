//! Scripted stand-ins for the tests of the daemon commands: a daemon whose
//! answers depend on how long the test has waited (tokio's clock is
//! paused), a launch record that changes with time, Ctrl+C at a chosen
//! moment, and the setup that wires them to `FakeBackgroundAgent`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_protocol::codes::ipc;
use stackly_protocol::daemon::{DaemonInfo, DaemonRoots, ShutdownMode};
use stackly_protocol::{CONTRACT_DIGEST, ErrorBody, ProtocolVersion};
use stackly_testkit::{FakeBackgroundAgent, FakeClock};
use tokio::time::Instant;
use uuid::Uuid;

use super::agent::AgentEnv;
use super::wait::{BoxFuture, DaemonProbe, Interrupt, LaunchRecordSource, Reachable, Shutdown};
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
        }
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
            Ok(info(reachable.server_instance_id))
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
