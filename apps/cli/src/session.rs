//! The connection to the daemon (spec 0008, *Koneksi ke daemon*): a lazy
//! connect plus handshake bounded at 5 seconds, a time limit per request
//! that the calling command picks, a `requestId` made before sending, the
//! `daemon.starting` retry, the `-v` lines, and the CLI's own hints for
//! some codes. The CLI never starts the daemon.

use std::path::{Path, PathBuf};
use std::time::Duration;

use stackly_ipc::{CallOptions, Client, ConnectPolicy, EventStream};
use stackly_protocol::codes::{daemon, ipc, request};
use stackly_protocol::events::{SubscribeParams, Subscribed};
use stackly_protocol::handshake::{ClientInfo, ClientKind, HandshakeAccepted};
use stackly_protocol::{Call, ErrorBody, Reply};
use tokio::time::Instant;
use uuid::Uuid;

use crate::failure::CliFailure;
use crate::output::stderr::StderrSink;

/// How long connecting plus the handshake may take together.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The method a connect plus handshake that ran out of time names.
pub const HANDSHAKE_METHOD: &str = "daemon.handshake";
/// The waits between `daemon.starting` retries; the last one repeats.
pub const STARTING_BACKOFF: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_secs(1),
];
/// How long `daemon.starting` is retried after its first answer.
pub const STARTING_BUDGET: Duration = Duration::from_secs(10);

/// The time limit of one request, chosen by the calling command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTimeout {
    /// 10 seconds, for every call that cannot wait on a Keychain dialog.
    Control,
    /// 200 seconds (the 180 second `SecretWorker` queue of spec 0011 plus a
    /// margin), for a call that sends `AllowPrompt`.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "spec 0011 step 8 sends AllowPrompt calls with it")
    )]
    Interactive,
}

impl RequestTimeout {
    /// The limit.
    #[must_use]
    pub const fn duration(self) -> Duration {
        match self {
            Self::Control => Duration::from_secs(10),
            Self::Interactive => Duration::from_secs(200),
        }
    }
}

/// How to send one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSpec {
    /// Its time limit.
    pub timeout: RequestTimeout,
    /// Its idempotency key, for a method that starts an operation.
    pub idempotency_key: Option<String>,
}

impl CallSpec {
    /// A control call: 10 seconds, no idempotency key.
    #[must_use]
    pub const fn control() -> Self {
        Self {
            timeout: RequestTimeout::Control,
            idempotency_key: None,
        }
    }
}

/// Where the daemon is and how this client introduces itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connector {
    /// The socket path.
    pub socket: PathBuf,
    /// The uid that must own the socket and the daemon peer.
    pub uid: u32,
    /// Whether the person at this CLI can answer a dialog (spec 0006
    /// AC-19).
    pub interactive: bool,
}

/// A connection that passed the endpoint checks and the handshake.
#[derive(Debug)]
pub struct Session {
    client: Client,
    accepted: HandshakeAccepted,
}

impl Session {
    /// Connects and handshakes as client `cli`, within [`CONNECT_TIMEOUT`].
    ///
    /// # Errors
    ///
    /// The transport's error (such as `ipc.endpoint_missing`,
    /// `ipc.endpoint_untrusted`, or `protocol.major_mismatch`), or
    /// `ipc.call_timeout` naming `daemon.handshake` and 5 seconds.
    pub async fn connect(
        connector: &Connector,
        sink: &mut StderrSink,
        verbose: bool,
    ) -> Result<Self, CliFailure> {
        let client = ClientInfo {
            kind: ClientKind::Cli,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: Some(std::process::id()),
            interactive: Some(connector.interactive),
        };
        trace(sink, verbose, &format!("-> {HANDSHAKE_METHOD}"));
        let started = Instant::now();
        let connected = tokio::time::timeout(
            CONNECT_TIMEOUT,
            Client::connect(
                &connector.socket,
                ConnectPolicy::same_user(connector.uid),
                client,
            ),
        )
        .await;
        match connected {
            Ok(Ok((client, accepted))) => {
                trace_done(sink, verbose, "ok", started);
                Ok(Self { client, accepted })
            }
            Ok(Err(error)) => {
                let body = error.to_error_body();
                trace_done(sink, verbose, body.code(), started);
                Err(CliFailure::daemon(body, None))
            }
            Err(_) => {
                trace_done(sink, verbose, "timeout", started);
                Err(CliFailure::call_timeout(
                    HANDSHAKE_METHOD,
                    CONNECT_TIMEOUT,
                    None,
                    None,
                ))
            }
        }
    }

    /// The daemon's handshake answer.
    #[must_use]
    pub const fn accepted(&self) -> &HandshakeAccepted {
        &self.accepted
    }

    /// Sends `call` once: a fresh lowercase UUID v7 `requestId`, the first
    /// `-v` line, the call under its time limit, then the second `-v` line.
    ///
    /// # Errors
    ///
    /// The daemon's error with this `requestId`, or `ipc.call_timeout`
    /// with the same `requestId`, so the daemon log line still matches.
    pub async fn call_once(
        &self,
        call: Call,
        spec: &CallSpec,
        sink: &mut StderrSink,
        verbose: bool,
    ) -> Result<Reply, CliFailure> {
        let method = call.method();
        let request_id = Uuid::now_v7().to_string();
        trace(sink, verbose, &format!("-> {method} {request_id}"));
        let started = Instant::now();
        let mut options = CallOptions::default().with_request_id(request_id.clone());
        if let Some(key) = &spec.idempotency_key {
            options = options.with_idempotency_key(key.clone());
        }
        let limit = spec.timeout.duration();
        match tokio::time::timeout(limit, self.client.call_detailed(call, options)).await {
            Ok(Ok(reply)) => {
                trace_done(sink, verbose, "ok", started);
                Ok(reply)
            }
            Ok(Err(body)) => {
                trace_done(sink, verbose, body.code(), started);
                Err(CliFailure::daemon(body, Some(request_id)))
            }
            Err(_) => {
                trace_done(sink, verbose, "timeout", started);
                Err(CliFailure::call_timeout(
                    method,
                    limit,
                    spec.idempotency_key.as_deref(),
                    Some(request_id),
                ))
            }
        }
    }

    /// Sends `call`, retrying `daemon.starting` with [`STARTING_BACKOFF`]
    /// for up to [`STARTING_BUDGET`] after its first answer. No other code
    /// is retried, retryable or not.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::call_once`], and the last `daemon.starting`
    /// once the budget ran out.
    pub async fn call(
        &self,
        call: &Call,
        spec: &CallSpec,
        sink: &mut StderrSink,
        verbose: bool,
    ) -> Result<Reply, CliFailure> {
        let mut first_starting: Option<Instant> = None;
        let mut attempt = 0;
        loop {
            match self.call_once(call.clone(), spec, sink, verbose).await {
                Err(failure) if failure.body.is(&daemon::STARTING) => {
                    let since = *first_starting.get_or_insert_with(Instant::now);
                    let elapsed = since.elapsed();
                    if elapsed >= STARTING_BUDGET {
                        return Err(failure);
                    }
                    let delay = STARTING_BACKOFF
                        .get(attempt)
                        .copied()
                        .unwrap_or(Duration::from_secs(1));
                    attempt += 1;
                    tokio::time::sleep(delay.min(STARTING_BUDGET - elapsed)).await;
                }
                other => return other,
            }
        }
    }

    /// Starts an event subscription, with a `requestId` and `-v` lines like
    /// [`Self::call_once`], under the control time limit.
    ///
    /// # Errors
    ///
    /// The daemon's error, or `ipc.call_timeout`.
    pub async fn subscribe(
        &self,
        params: SubscribeParams,
        sink: &mut StderrSink,
        verbose: bool,
    ) -> Result<(Subscribed, EventStream), CliFailure> {
        let method = Call::EventsSubscribe(SubscribeParams::default()).method();
        let request_id = Uuid::now_v7().to_string();
        trace(sink, verbose, &format!("-> {method} {request_id}"));
        let started = Instant::now();
        let limit = RequestTimeout::Control.duration();
        let options = CallOptions::default().with_request_id(request_id.clone());
        match tokio::time::timeout(limit, self.client.subscribe_detailed(params, options)).await {
            Ok(Ok(subscribed)) => {
                trace_done(sink, verbose, "ok", started);
                Ok(subscribed)
            }
            Ok(Err(body)) => {
                trace_done(sink, verbose, body.code(), started);
                Err(CliFailure::daemon(body, Some(request_id)))
            }
            Err(_) => {
                trace_done(sink, verbose, "timeout", started);
                Err(CliFailure::call_timeout(
                    method,
                    limit,
                    None,
                    Some(request_id),
                ))
            }
        }
    }
}

/// The first `-v` line of a request.
fn trace(sink: &mut StderrSink, verbose: bool, line: &str) {
    if verbose {
        sink.line(line);
    }
}

/// The second `-v` line: `<- <ok|code|timeout> <ms> ms`.
fn trace_done(sink: &mut StderrSink, verbose: bool, outcome: &str, started: Instant) {
    if verbose {
        let ms = started.elapsed().as_millis();
        sink.line(&format!("<- {outcome} {ms} ms"));
    }
}

/// The CLI's own wording and hints for an error, for text only; the
/// `ErrorBody` never changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HumanHints {
    /// A message that replaces the body's in text.
    pub message: Option<String>,
    /// Hints written after the body's own hint.
    pub hints: Vec<String>,
}

/// The CLI's wording and hints for `body`. `socket` is the daemon socket
/// path, for the message when no daemon runs.
#[must_use]
pub fn human_hints(body: &ErrorBody, socket: Option<&Path>) -> HumanHints {
    if body.is(&ipc::ENDPOINT_MISSING) || body.is(&ipc::CONNECTION_REFUSED) {
        let path = socket.map_or_else(
            || body.detail("path").unwrap_or("the socket path").to_owned(),
            |socket| socket.display().to_string(),
        );
        return HumanHints {
            message: Some(format!("daemon is not running (no socket at {path})")),
            // provisional: F15 (`stackly daemon start` replaces this hint)
            hints: vec!["start it with stacklyd".to_owned()],
        };
    }
    if body.is(&request::UNKNOWN_METHOD) {
        let method = body.detail("method").unwrap_or("this method");
        return HumanHints {
            message: None,
            hints: vec![format!(
                "update stacklyd to a version that supports {method}"
            )],
        };
    }
    if body.is(&ipc::CALL_TIMEOUT)
        && let Some(key) = body.detail("idempotencyKey")
    {
        return HumanHints {
            message: None,
            hints: vec![format!("retry safely with: --idempotency-key {key}")],
        };
    }
    HumanHints::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_calls_get_10_seconds_and_interactive_ones_200() {
        assert_eq!(RequestTimeout::Control.duration(), Duration::from_secs(10));
        assert_eq!(
            RequestTimeout::Interactive.duration(),
            Duration::from_secs(200)
        );
        assert_eq!(CallSpec::control().timeout, RequestTimeout::Control);
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn an_interactive_limit_stops_waiting_after_200_seconds_of_tokio_time() {
        let started = Instant::now();
        let limit = RequestTimeout::Interactive.duration();
        let waited = tokio::time::timeout(limit, std::future::pending::<()>()).await;
        assert!(waited.is_err());
        assert_eq!(started.elapsed(), Duration::from_secs(200));
    }

    #[test]
    fn the_starting_schedule_doubles_to_one_second_within_ten() {
        assert_eq!(
            STARTING_BACKOFF.map(|delay| delay.as_millis()),
            [100, 200, 400, 800, 1000]
        );
        assert_eq!(STARTING_BUDGET, Duration::from_secs(10));
    }

    #[test]
    fn no_daemon_is_worded_with_the_socket_path_and_a_hint() {
        for spec in [&ipc::ENDPOINT_MISSING, &ipc::CONNECTION_REFUSED] {
            let body = ErrorBody::new(spec, "x").with_detail("path", "/detail.sock");
            let hints = human_hints(&body, Some(Path::new("/run/stacklyd.sock")));
            assert_eq!(
                hints.message.as_deref(),
                Some("daemon is not running (no socket at /run/stacklyd.sock)")
            );
            assert_eq!(hints.hints, ["start it with stacklyd"]);
            assert_eq!(
                human_hints(&body, None).message.as_deref(),
                Some("daemon is not running (no socket at /detail.sock)")
            );
        }
    }

    #[test]
    fn an_unknown_method_and_a_keyed_timeout_get_their_hints() {
        let unknown = ErrorBody::new(&request::UNKNOWN_METHOD, "unknown")
            .with_detail("method", "operation.list");
        assert_eq!(
            human_hints(&unknown, None).hints,
            ["update stacklyd to a version that supports operation.list"]
        );
        let keyed = CliFailure::call_timeout("x.y", Duration::from_secs(10), Some("k-1"), None);
        assert_eq!(
            human_hints(&keyed.body, None).hints,
            ["retry safely with: --idempotency-key k-1"]
        );
        let unkeyed = CliFailure::call_timeout("x.y", Duration::from_secs(10), None, None);
        assert_eq!(human_hints(&unkeyed.body, None), HumanHints::default());
        assert_eq!(
            human_hints(&CliFailure::panic().body, None),
            HumanHints::default()
        );
    }
}
