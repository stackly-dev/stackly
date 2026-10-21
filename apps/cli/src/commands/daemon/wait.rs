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
use stackly_protocol::daemon::{DaemonInfo, ShutdownMode, ShutdownParams};
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::{Call, Reply};
use tokio::time::Instant;
use uuid::Uuid;

use crate::failure::CliFailure;
use crate::output::stderr::StderrSink;
use crate::session::{CallSpec, Connector, HANDSHAKE_METHOD, Session};

/// How often the daemon is tried while waiting for it.
pub const DAEMON_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How long `install`, `start`, and `restart` wait for a daemon to answer.
pub const DAEMON_START_WAIT: Duration = Duration::from_secs(30);
/// How long `stop` and `restart` wait for the old daemon to go.
pub const DAEMON_STOP_WAIT: Duration = Duration::from_secs(20);
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
}

/// The launch record the daemon writes under launchd.
pub trait LaunchRecordSource: Send + Sync {
    /// Whether the file exists and is a plain file.
    fn exists(&self) -> bool;

    /// `lastLaunchedAt`, or `None` when the file is missing, unreadable,
    /// above 4 KiB, or holds no valid RFC 3339 time.
    fn last_launched_at(&self) -> Option<jiff::Timestamp>;
}

/// Ctrl+C, as the commands see it.
pub trait Interrupt: Send {
    /// Whether Ctrl+C arrived since the start; it stays true.
    fn pending(&mut self) -> bool;

    /// Resolves once Ctrl+C arrived.
    fn wait(&mut self) -> BoxFuture<'_, ()>;
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
            let call = Call::DaemonShutdown(ShutdownParams { mode });
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
        let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        record.get("lastLaunchedAt")?.as_str()?.parse().ok()
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
