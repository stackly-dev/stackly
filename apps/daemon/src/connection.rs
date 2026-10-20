//! One client connection (spec 0006, *Koneksi* and *Handshake*).
//!
//! A reader task takes frames one at a time. Until the handshake is
//! accepted it handles them in order, so nothing a client sends behind a
//! refused handshake is ever processed. After it, every request runs as
//! its own task, at most [`MAX_IN_FLIGHT_REQUESTS`] at once. A writer task
//! owns the socket's write half and sends what the bounded outbound queue
//! holds, replies and events alike; queueing waits at most
//! [`WRITE_TIMEOUT`], after which the connection closes.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use stackly_ipc::{
    FrameReader, FrameWriter, FramedConnection, IpcError, PeerCred, REJECT_WRITE_TIMEOUT,
};
use stackly_protocol::codes::{daemon, events, internal, ipc, protocol, request};
use stackly_protocol::events::Subscribed;
use stackly_protocol::handshake::{
    ClientInfo, ClientKind, DaemonIdentity, HandshakeAccepted, HandshakeParams, contract_drift,
};
use stackly_protocol::json;
use stackly_protocol::limits::{CONTROL_FRAME_MAX, MAX_IN_FLIGHT_REQUESTS, MAX_SUBSCRIPTIONS};
use stackly_protocol::{
    CONTRACT_DIGEST, Call, ErrorBody, ErrorCategory, ErrorFrame, Limits, ProtocolVersion, Reply,
    Request, Response, Sequence, ServerFrame,
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio::task::AbortHandle;
use tokio::time::{Instant, timeout};
use uuid::Uuid;

use crate::dispatch::{HandlerFuture, Handlers, RequestContext};
use crate::events::BusSubscription;
use crate::ipc::operation::{StartOutcome, StartRequest, TestStartingMethod, start_operation};
use crate::map;
use crate::shutdown::DrainGuard;
use crate::state::DaemonCore;

/// How long a client has to send its complete handshake after the accept.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long queueing an outbound frame may wait before the connection
/// closes with `ipc.write_timeout` in the log.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// How many frames wait for the writer per connection.
pub const OUTBOUND_QUEUE_LEN: usize = 64;
/// How often the flush at stop checks whether subscriptions still forward.
const FORWARD_POLL: Duration = Duration::from_millis(5);

/// What the writer task takes from the outbound queue.
enum Out {
    Frame(Vec<u8>),
    /// Send what came before, then shut the write direction down.
    Close,
}

/// The frame could not be queued: the connection closed or the client did
/// not read in time.
#[derive(Debug, Clone, Copy)]
struct Undelivered;

/// The open connections, for the flush at stop.
#[derive(Debug, Default)]
pub struct Connections {
    open: Mutex<HashMap<u64, Arc<Conn>>>,
    changed: Notify,
}

impl Connections {
    /// How many connections are being served.
    #[must_use]
    pub fn len(&self) -> usize {
        lock(&self.open).len()
    }

    /// Whether no connection is being served.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lets every subscription queue its events up to sequence `through`
    /// (the `daemon.shutting_down` of the stop), then asks every connection
    /// to send what its queue holds and close. Each phase waits up to
    /// `flush`; connections still open after that are closed at once.
    pub async fn close_all(&self, flush: Duration, through: Sequence) {
        let deadline = Instant::now() + flush;
        while Instant::now() < deadline
            && self
                .snapshot()
                .iter()
                .any(|conn| conn.forwarding_before(through))
        {
            tokio::time::sleep(FORWARD_POLL).await;
        }
        for conn in self.snapshot() {
            // A full queue is closed at the deadline below instead.
            let _ = conn.tx.try_send(Out::Close);
        }
        self.wait_empty_until(Instant::now() + flush).await;
        for conn in self.snapshot() {
            conn.close();
        }
        self.wait_empty_until(Instant::now() + REJECT_WRITE_TIMEOUT)
            .await;
    }

    /// Closes every connection at once, without a flush.
    pub fn close_now(&self) {
        for conn in self.snapshot() {
            conn.close();
        }
    }

    fn insert(&self, conn: Arc<Conn>) {
        lock(&self.open).insert(conn.id, conn);
    }

    fn remove(&self, id: u64) {
        lock(&self.open).remove(&id);
        self.changed.notify_waiters();
    }

    fn snapshot(&self) -> Vec<Arc<Conn>> {
        lock(&self.open).values().cloned().collect()
    }

    async fn wait_empty_until(&self, deadline: Instant) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.is_empty() || Instant::now() >= deadline {
                return;
            }
            if tokio::time::timeout_at(deadline, changed).await.is_err() {
                return;
            }
        }
    }
}

/// One served connection.
pub struct Conn {
    id: u64,
    peer: PeerCred,
    tx: mpsc::Sender<Out>,
    closed: watch::Sender<bool>,
    client: OnceLock<ClientInfo>,
    running: Mutex<HashSet<String>>,
    permits: Arc<Semaphore>,
    subscriptions: Mutex<SubscriptionTable>,
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn")
            .field("id", &self.id)
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct SubscriptionTable {
    next_id: u32,
    active: HashMap<u32, Slot>,
}

/// A subscription whose `Subscribed` reply is being queued, or whose
/// forwarder runs, with the last sequence it queued.
#[derive(Debug)]
enum Slot {
    Reserved,
    Running(AbortHandle, Arc<AtomicU64>),
}

impl Conn {
    fn new(id: u64, peer: PeerCred, tx: mpsc::Sender<Out>) -> Self {
        Self {
            id,
            peer,
            tx,
            closed: watch::Sender::new(false),
            client: OnceLock::new(),
            running: Mutex::new(HashSet::new()),
            permits: Arc::new(Semaphore::new(MAX_IN_FLIGHT_REQUESTS as usize)),
            subscriptions: Mutex::new(SubscriptionTable {
                next_id: 1,
                active: HashMap::new(),
            }),
        }
    }

    /// Closes the connection: the reader and the writer stop at once.
    fn close(&self) {
        self.closed.send_replace(true);
    }

    fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    async fn closed(&self) {
        let mut closed = self.closed.subscribe();
        // The sender lives in `self`, so this only ends on `true`.
        let _ = closed.wait_for(|closed| *closed).await;
    }

    fn client(&self) -> ClientInfo {
        self.client.get().cloned().unwrap_or_else(|| ClientInfo {
            kind: ClientKind::Unrecognized,
            version: String::new(),
            pid: None,
            interactive: None,
        })
    }

    /// The client kind for log lines; `none` before the handshake.
    fn client_kind(&self) -> &'static str {
        self.client
            .get()
            .map_or("none", |client| kind_name(client.kind))
    }

    /// Whether the client said it can show a dialog, for log lines; false
    /// before the handshake and when the client did not say.
    fn interactive(&self) -> bool {
        self.client.get().is_some_and(ClientInfo::is_interactive)
    }

    /// Queues one frame, waiting at most [`WRITE_TIMEOUT`].
    async fn enqueue(&self, frame: &ServerFrame) -> Result<(), Undelivered> {
        self.enqueue_bytes(encode(frame)).await
    }

    async fn enqueue_bytes(&self, bytes: Vec<u8>) -> Result<(), Undelivered> {
        if self.is_closed() {
            return Err(Undelivered);
        }
        let sent = tokio::select! {
            biased;
            () = self.closed() => return Err(Undelivered),
            sent = timeout(WRITE_TIMEOUT, self.tx.send(Out::Frame(bytes))) => sent,
        };
        match sent {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(Undelivered),
            Err(_) => {
                tracing::warn!(
                    connection_id = self.id,
                    code = ipc::WRITE_TIMEOUT.code,
                    "the client did not read for 30 seconds; closing the connection",
                );
                self.close();
                Err(Undelivered)
            }
        }
    }

    /// Queues an error frame and the close, for a refused handshake or a
    /// framing error; a client that does not read gets at most
    /// [`REJECT_WRITE_TIMEOUT`].
    async fn refuse(&self, request_id: Option<String>, error: ErrorBody) {
        let frame = encode(&ServerFrame::Error(ErrorFrame { request_id, error }));
        let queued = timeout(REJECT_WRITE_TIMEOUT, async {
            self.tx.send(Out::Frame(frame)).await.ok()?;
            self.tx.send(Out::Close).await.ok()
        })
        .await;
        if !matches!(queued, Ok(Some(()))) {
            self.close();
        }
    }

    fn reserve_id(&self, request_id: &str) -> bool {
        lock(&self.running).insert(request_id.to_owned())
    }

    fn release_id(&self, request_id: &str) {
        lock(&self.running).remove(request_id);
    }

    fn subscribe(
        &self,
        core: &DaemonCore,
        params: stackly_protocol::events::SubscribeParams,
    ) -> Result<BusSubscription, Box<ErrorBody>> {
        let id = {
            let mut table = lock(&self.subscriptions);
            if table.active.len() >= MAX_SUBSCRIPTIONS as usize {
                let error = ErrorBody::new(
                    &events::TOO_MANY_SUBSCRIPTIONS,
                    "the connection holds the most subscriptions it may",
                )
                .with_detail("max", MAX_SUBSCRIPTIONS.to_string());
                return Err(Box::new(error));
            }
            let id = table.next_id;
            table.next_id = table.next_id.saturating_add(1);
            table.active.insert(id, Slot::Reserved);
            id
        };
        core.bus().subscribe(id, params).inspect_err(|_| {
            lock(&self.subscriptions).active.remove(&id);
        })
    }

    /// Starts forwarding a subscription whose `Subscribed` reply is queued,
    /// unless it was ended in between.
    fn start_forwarder(self: &Arc<Self>, subscription: BusSubscription) {
        let id = subscription.subscription_id();
        let queued = Arc::new(AtomicU64::new(subscription.head().sequence.get()));
        let conn = Arc::clone(self);
        let task = tokio::spawn(forward(conn, subscription, Arc::clone(&queued)));
        let mut table = lock(&self.subscriptions);
        match table.active.get_mut(&id) {
            Some(slot @ Slot::Reserved) => *slot = Slot::Running(task.abort_handle(), queued),
            _ => task.abort(),
        }
    }

    fn forget_subscription(&self, id: u32) {
        lock(&self.subscriptions).active.remove(&id);
    }

    /// Whether a subscription of this connection has not queued its
    /// events up to `through` yet.
    fn forwarding_before(&self, through: Sequence) -> bool {
        lock(&self.subscriptions)
            .active
            .values()
            .any(|slot| match slot {
                Slot::Running(_, queued) => queued.load(Ordering::Acquire) < through.get(),
                Slot::Reserved => false,
            })
    }

    fn unsubscribe(&self, id: u32) -> Result<(), Box<ErrorBody>> {
        match lock(&self.subscriptions).active.remove(&id) {
            Some(Slot::Running(task, _)) => {
                task.abort();
                Ok(())
            }
            Some(Slot::Reserved) => Ok(()),
            None => Err(Box::new(
                ErrorBody::new(
                    &events::UNKNOWN_SUBSCRIPTION,
                    "no such subscription on this connection",
                )
                .with_detail("subscriptionId", id.to_string()),
            )),
        }
    }

    fn end_subscriptions(&self) {
        for (_, slot) in lock(&self.subscriptions).active.drain() {
            if let Slot::Running(task, _) = slot {
                task.abort();
            }
        }
    }
}

/// How the request loop ended.
enum End {
    /// The client closed its side cleanly between frames.
    Eof,
    /// An error frame and the close are queued.
    Refused,
    /// The connection was closed, by the daemon or by a failed write.
    Closed,
}

/// Serves one authorized connection until it closes. `slot` holds the
/// connection's place under the connection limit.
pub async fn serve(
    connection: FramedConnection,
    peer: PeerCred,
    accepted_at: Instant,
    core: Arc<DaemonCore>,
    handlers: Arc<dyn Handlers>,
    slot: OwnedSemaphorePermit,
) {
    let id = core.next_connection_id();
    let (mut reader, writer) = connection.into_split();
    let (tx, rx) = mpsc::channel(OUTBOUND_QUEUE_LEN);
    let conn = Arc::new(Conn::new(id, peer, tx));
    core.connections().insert(Arc::clone(&conn));
    core.counters().opened();
    let writer = tokio::spawn(write_loop(writer, rx, Arc::clone(&conn)));
    tracing::debug!(connection_id = id, peer_pid = peer.pid, "connection opened");

    let end = match handshake(&mut reader, &conn, &core, accepted_at).await {
        Handshake::Accepted => request_loop(&mut reader, &conn, &core, &handlers).await,
        Handshake::Refused => {
            core.counters().rejected();
            End::Refused
        }
        Handshake::Gone => End::Closed,
    };
    drop(reader);
    conn.end_subscriptions();
    if matches!(end, End::Eof) {
        // Let the running requests finish and their replies go out.
        tokio::select! {
            _ = Arc::clone(&conn.permits).acquire_many_owned(MAX_IN_FLIGHT_REQUESTS) => {}
            () = conn.closed() => {}
        }
        let _ = conn.tx.try_send(Out::Close);
    }
    if !matches!(end, End::Closed) && timeout(REJECT_WRITE_TIMEOUT, conn.closed()).await.is_err() {
        conn.close();
    }
    conn.close();
    let _ = writer.await;
    core.connections().remove(id);
    core.counters().closed();
    drop(slot);
    tracing::debug!(
        connection_id = id,
        peer_pid = peer.pid,
        client_kind = conn.client_kind(),
        interactive = conn.interactive(),
        "connection closed",
    );
}

async fn write_loop(mut writer: FrameWriter, mut rx: mpsc::Receiver<Out>, conn: Arc<Conn>) {
    loop {
        let out = tokio::select! {
            biased;
            () = conn.closed() => break,
            out = rx.recv() => out,
        };
        match out {
            None => break,
            Some(Out::Frame(bytes)) => {
                let written = tokio::select! {
                    biased;
                    () = conn.closed() => break,
                    written = writer.write_frame(&bytes) => written,
                };
                if let Err(error) = written {
                    tracing::debug!(connection_id = conn.id, error = %error, "a frame could not be written");
                    break;
                }
            }
            Some(Out::Close) => {
                let _ = timeout(REJECT_WRITE_TIMEOUT, writer.shutdown()).await;
                break;
            }
        }
    }
    conn.close();
}

enum Handshake {
    Accepted,
    Refused,
    Gone,
}

async fn handshake(
    reader: &mut FrameReader,
    conn: &Conn,
    core: &DaemonCore,
    accepted_at: Instant,
) -> Handshake {
    let started = Instant::now();
    let deadline = accepted_at + HANDSHAKE_TIMEOUT;
    let frame = tokio::select! {
        biased;
        () = conn.closed() => return Handshake::Gone,
        frame = reader.read_frame(Some(deadline)) => frame,
    };
    let bytes = match frame {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            tracing::debug!(
                connection_id = conn.id,
                "the client left before the handshake"
            );
            return Handshake::Gone;
        }
        Err(error) => {
            let Some(body) = read_failure(&error, core) else {
                return Handshake::Gone;
            };
            log_refusal(conn, None, &body);
            conn.refuse(None, body).await;
            return Handshake::Refused;
        }
    };

    if let Some(major) = json::handshake_major(&bytes)
        && major != ProtocolVersion::CURRENT.major
    {
        let error = major_mismatch();
        let request_id = json::request_id(&bytes);
        log_refusal(conn, request_id.as_deref(), &error);
        conn.refuse(request_id, error).await;
        return Handshake::Refused;
    }

    let (request_id, params) = match json::decode_request(&bytes) {
        // `handshake_major` reads only the usual object shape, so the
        // decoded major is checked again: both readers must agree.
        Ok(Request {
            request_id,
            call: Call::DaemonHandshake(params),
            ..
        }) if params.protocol.major != ProtocolVersion::CURRENT.major => {
            let error = major_mismatch();
            log_refusal(conn, Some(&request_id), &error);
            conn.refuse(Some(request_id), error).await;
            return Handshake::Refused;
        }
        Ok(Request {
            request_id,
            call: Call::DaemonHandshake(params),
            ..
        }) => (request_id, params),
        Ok(request) => {
            let error = handshake_required(Some(request.call.method()));
            log_refusal(conn, Some(&request.request_id), &error);
            conn.refuse(Some(request.request_id), error).await;
            return Handshake::Refused;
        }
        Err(rejection) if rejection.error.is(&request::UNKNOWN_METHOD) => {
            let error = handshake_required(rejection.error.detail("method"));
            log_refusal(conn, rejection.request_id.as_deref(), &error);
            conn.refuse(rejection.request_id, error).await;
            return Handshake::Refused;
        }
        Err(rejection) => {
            core.counters().frame_rejected();
            log_refusal(conn, rejection.request_id.as_deref(), &rejection.error);
            let frame = rejection.into_frame();
            conn.refuse(frame.request_id, frame.error).await;
            return Handshake::Refused;
        }
    };

    let accepted = accept(conn, core, &params);
    let _ = conn.client.set(params.client);
    let frame = ServerFrame::Response(Response {
        request_id: request_id.clone(),
        reply: Reply::DaemonHandshake(accepted),
    });
    if conn.enqueue(&frame).await.is_err() {
        return Handshake::Gone;
    }
    log_request(
        conn,
        Some(&request_id),
        Some("daemon.handshake"),
        started,
        None,
        None,
    );
    Handshake::Accepted
}

fn accept(conn: &Conn, core: &DaemonCore, params: &HandshakeParams) -> HandshakeAccepted {
    let drift = contract_drift(
        params.protocol,
        &params.contract_digest,
        ProtocolVersion::CURRENT,
        CONTRACT_DIGEST,
    );
    let interactive = params.client.is_interactive();
    if drift {
        tracing::warn!(
            connection_id = conn.id,
            contract_drift = true,
            client_contract_digest = params.contract_digest.as_str(),
            client_kind = kind_name(params.client.kind),
            client_version = params.client.version.as_str(),
            interactive,
            "contract_drift: the client has the same protocol version but another contract digest",
        );
    }
    tracing::debug!(
        connection_id = conn.id,
        peer_pid = conn.peer.pid,
        client_kind = kind_name(params.client.kind),
        client_version = params.client.version.as_str(),
        client_protocol_minor = params.protocol.minor,
        interactive,
        "handshake accepted",
    );
    let info = core.start_info();
    HandshakeAccepted {
        protocol: ProtocolVersion::CURRENT,
        server_instance_id: core.bus().server_instance_id(),
        daemon: DaemonIdentity {
            version: info.version.clone(),
            pid: info.pid,
        },
        contract_digest: CONTRACT_DIGEST.to_owned(),
        contract_drift: drift,
        limits: Limits::CURRENT,
        head: core.bus().head(),
    }
}

async fn request_loop(
    reader: &mut FrameReader,
    conn: &Arc<Conn>,
    core: &Arc<DaemonCore>,
    handlers: &Arc<dyn Handlers>,
) -> End {
    loop {
        let frame = tokio::select! {
            biased;
            () = conn.closed() => return End::Closed,
            frame = reader.read_frame(None) => frame,
        };
        match frame {
            Ok(None) => return End::Eof,
            Ok(Some(bytes)) => on_frame(&bytes, conn, core, handlers).await,
            Err(error) => {
                let Some(body) = read_failure(&error, core) else {
                    return End::Eof;
                };
                log_refusal(conn, None, &body);
                conn.refuse(None, body).await;
                return End::Refused;
            }
        }
    }
}

/// One frame after the handshake: answer it at once when it is refused,
/// else run it as its own task.
async fn on_frame(
    bytes: &[u8],
    conn: &Arc<Conn>,
    core: &Arc<DaemonCore>,
    handlers: &Arc<dyn Handlers>,
) {
    let received = Instant::now();
    // provisional: F10 (no contract method starts an operation yet; only a
    // test registers this one)
    if let Some(starting) = handlers.test_starting_method()
        && let Some(frame) = starting_frame(bytes, starting.method())
    {
        on_starting(frame, starting, conn, core, received).await;
        return;
    }
    let request = match json::decode_request(bytes) {
        Ok(request) => request,
        Err(rejection) => {
            core.counters().frame_rejected();
            let request_id = rejection.request_id.clone();
            log_request(
                conn,
                request_id.as_deref(),
                None,
                received,
                Some(&rejection.error),
                None,
            );
            let _ = conn
                .enqueue(&ServerFrame::Error(rejection.into_frame()))
                .await;
            return;
        }
    };
    let method = request.call.method();
    let repeated = matches!(request.call, Call::DaemonHandshake(_));
    let Some((permit, guard)) =
        admit(conn, core, &request.request_id, method, repeated, received).await
    else {
        return;
    };
    tokio::spawn(run_request(
        Arc::clone(conn),
        Arc::clone(core),
        Arc::clone(handlers),
        request,
        permit,
        guard,
        received,
    ));
}

/// Admits one request that decoded: counts it in the drain, then refuses
/// it when it repeats the handshake, the daemon stops, its `requestId`
/// still runs, or the connection runs the most requests it may. A refused
/// request is answered here and gives `None`.
async fn admit(
    conn: &Arc<Conn>,
    core: &Arc<DaemonCore>,
    request_id: &str,
    method: &'static str,
    repeated_handshake: bool,
    received: Instant,
) -> Option<(OwnedSemaphorePermit, DrainGuard)> {
    // Counted before the stop check, so a stop either sees this request in
    // the drain or this request sees the stop (spec 0006, *Berhenti*).
    let guard = core.drain().enter();
    let refused = if repeated_handshake {
        Some(ErrorBody::new(
            &protocol::HANDSHAKE_REPEATED,
            "the handshake was already accepted on this connection",
        ))
    } else if core.is_stopping() {
        Some(ErrorBody::new(
            &daemon::SHUTTING_DOWN,
            "the daemon is stopping",
        ))
    } else if !conn.reserve_id(request_id) {
        Some(ErrorBody::new(
            &request::DUPLICATE_ID,
            "another request with this requestId still runs on this connection",
        ))
    } else {
        None
    };
    if let Some(error) = refused {
        drop(guard);
        reply_error(conn, request_id.to_owned(), method, received, error).await;
        return None;
    }
    let Ok(permit) = Arc::clone(&conn.permits).try_acquire_owned() else {
        drop(guard);
        conn.release_id(request_id);
        let error = ErrorBody::new(
            &request::TOO_MANY_IN_FLIGHT,
            "the connection runs the most requests it may at once",
        )
        .with_detail("max", MAX_IN_FLIGHT_REQUESTS.to_string());
        reply_error(conn, request_id.to_owned(), method, received, error).await;
        return None;
    };
    Some((permit, guard))
}

async fn reply_error(
    conn: &Conn,
    request_id: String,
    method: &'static str,
    received: Instant,
    error: ErrorBody,
) {
    log_request(
        conn,
        Some(&request_id),
        Some(method),
        received,
        Some(&error),
        None,
    );
    let frame = ServerFrame::Error(ErrorFrame {
        request_id: Some(request_id),
        error,
    });
    let _ = conn.enqueue(&frame).await;
}

async fn run_request(
    conn: Arc<Conn>,
    core: Arc<DaemonCore>,
    handlers: Arc<dyn Handlers>,
    request: Request,
    permit: OwnedSemaphorePermit,
    guard: DrainGuard,
    received: Instant,
) {
    let Request {
        request_id, call, ..
    } = request;
    let method = call.method();
    let operation_id = named_operation(&call);
    let context = RequestContext::new(conn.id, request_id.clone(), conn.peer, conn.client());
    let (result, subscription) = dispatch(&conn, &core, &handlers, context, call).await;
    let subscription_id = subscription.as_ref().map(BusSubscription::subscription_id);
    let (mut bytes, mut error) = match result {
        Ok(reply) => (
            encode(&ServerFrame::Response(Response {
                request_id: request_id.clone(),
                reply,
            })),
            None,
        ),
        Err(error) => {
            let bytes = encode(&ServerFrame::Error(ErrorFrame {
                request_id: Some(request_id.clone()),
                error: (*error).clone(),
            }));
            (bytes, Some(*error))
        }
    };
    if bytes.len() > CONTROL_FRAME_MAX as usize {
        let too_large = ErrorBody::new(
            &ipc::REPLY_TOO_LARGE,
            "the reply is above the frame size limit",
        )
        .with_detail("method", method)
        .with_detail("length", bytes.len().to_string());
        bytes = encode(&ServerFrame::Error(ErrorFrame {
            request_id: Some(request_id.clone()),
            error: too_large.clone(),
        }));
        error = Some(too_large);
    }
    log_request(
        &conn,
        Some(&request_id),
        Some(method),
        received,
        error.as_ref(),
        operation_id,
    );
    let delivered = conn.enqueue_bytes(bytes).await;
    match (subscription, delivered, error) {
        (Some(subscription), Ok(()), None) => conn.start_forwarder(subscription),
        _ => {
            if let Some(id) = subscription_id {
                conn.forget_subscription(id);
            }
        }
    }
    if delivered.is_err() {
        tracing::debug!(
            connection_id = conn.id,
            request_id = request_id.as_str(),
            method,
            "the reply was dropped: the connection is gone",
        );
    }
    conn.release_id(&request_id);
    drop(permit);
    drop(guard);
}

/// The answer to a method the daemon does not serve right now: while the
/// registry is in recovery mode, every method whose table line says
/// `recovery refused` gets `registry.recovery_mode` before its handler
/// runs, so nothing reads or writes the registry (spec 0006 AC-18).
#[must_use]
pub fn recovery_refusal(core: &DaemonCore, allowed_in_recovery: bool) -> Option<Box<ErrorBody>> {
    if allowed_in_recovery {
        return None;
    }
    core.recovery()
        .map(|recovery| Box::new(map::recovery_mode(&recovery)))
}

/// Runs one request: a single `match` over the call.
async fn dispatch(
    conn: &Arc<Conn>,
    core: &Arc<DaemonCore>,
    handlers: &Arc<dyn Handlers>,
    context: RequestContext,
    call: Call,
) -> (Result<Reply, Box<ErrorBody>>, Option<BusSubscription>) {
    if let Some(error) = recovery_refusal(core, call.allowed_in_recovery()) {
        return (Err(error), None);
    }
    match call {
        Call::DaemonHandshake(_) => (
            Err(Box::new(ErrorBody::new(
                &protocol::HANDSHAKE_REPEATED,
                "the handshake was already accepted on this connection",
            ))),
            None,
        ),
        Call::DaemonGetInfo => (
            guarded(core, handlers, context, |h, c, x| h.get_info(c, x))
                .await
                .map(Reply::DaemonGetInfo),
            None,
        ),
        Call::DaemonGetHealth => (
            guarded(core, handlers, context, |h, c, x| h.get_health(c, x))
                .await
                .map(Reply::DaemonGetHealth),
            None,
        ),
        Call::DaemonGetCapabilities => (
            guarded(core, handlers, context, |h, c, x| h.get_capabilities(c, x))
                .await
                .map(Reply::DaemonGetCapabilities),
            None,
        ),
        Call::EventsSubscribe(params) => match conn.subscribe(core, params) {
            Ok(subscription) => {
                let subscribed = Subscribed {
                    subscription_id: subscription.subscription_id(),
                    head: subscription.head(),
                };
                (Ok(Reply::EventsSubscribe(subscribed)), Some(subscription))
            }
            Err(error) => (Err(error), None),
        },
        Call::EventsUnsubscribe(params) => (
            conn.unsubscribe(params.subscription_id)
                .map(|()| Reply::EventsUnsubscribe),
            None,
        ),
        Call::OperationGet(params) => (
            guarded_with(core, handlers, context, params, |h, c, x, p| {
                h.get_operation(c, x, p)
            })
            .await
            .map(|detail| Reply::OperationGet(Box::new(detail))),
            None,
        ),
        Call::OperationList(params) => (
            guarded_with(core, handlers, context, params, |h, c, x, p| {
                h.list_operations(c, x, p)
            })
            .await
            .map(Reply::OperationList),
            None,
        ),
        Call::OperationCancel(params) => (
            guarded_with(core, handlers, context, params, |h, c, x, p| {
                h.cancel_operation(c, x, p)
            })
            .await
            .map(|reply| Reply::OperationCancel(Box::new(reply))),
            None,
        ),
        Call::OperationResume(params) => (
            guarded_with(core, handlers, context, params, |h, c, x, p| {
                h.resume_operation(c, x, p)
            })
            .await
            .map(|summary| Reply::OperationResume(Box::new(summary))),
            None,
        ),
        Call::OperationAbandon(params) => (
            guarded_with(core, handlers, context, params, |h, c, x, p| {
                h.abandon_operation(c, x, p)
            })
            .await
            .map(|reply| Reply::OperationAbandon(Box::new(reply))),
            None,
        ),
        Call::OperationGetStats => (
            guarded(core, handlers, context, |h, c, x| {
                h.get_operation_stats(c, x)
            })
            .await
            .map(Reply::OperationGetStats),
            None,
        ),
        Call::CatalogListProviders => (
            guarded(core, handlers, context, |h, c, x| h.list_providers(c, x))
                .await
                .map(Reply::CatalogListProviders),
            None,
        ),
        Call::CatalogGetProvider(params) => (
            guarded_with(core, handlers, context, params, |h, c, x, p| {
                h.get_provider(c, x, p)
            })
            .await
            .map(Reply::CatalogGetProvider),
            None,
        ),
    }
}

/// The operation a request names in its params, for its log line.
fn named_operation(call: &Call) -> Option<Uuid> {
    match call {
        Call::OperationGet(params) => Some(params.operation_id),
        Call::OperationCancel(params) => Some(params.operation_id),
        Call::OperationResume(params) => Some(params.operation_id),
        Call::OperationAbandon(params) => Some(params.operation_id),
        _ => None,
    }
}

/// Runs a handler in its own task, so a panic becomes
/// `internal.unexpected` and a log line instead of a lost connection.
async fn guarded<T: Send + 'static>(
    core: &Arc<DaemonCore>,
    handlers: &Arc<dyn Handlers>,
    context: RequestContext,
    call: for<'a> fn(&'a dyn Handlers, &'a DaemonCore, &'a RequestContext) -> HandlerFuture<'a, T>,
) -> Result<T, Box<ErrorBody>> {
    let core = Arc::clone(core);
    let handlers = Arc::clone(handlers);
    let connection_id = context.connection_id;
    let request_id = context.request_id.clone();
    let task = tokio::spawn(async move { call(&*handlers, &core, &context).await });
    match task.await {
        Ok(result) => result,
        Err(failure) => {
            tracing::error!(
                connection_id,
                request_id = request_id.as_str(),
                panicked = failure.is_panic(),
                "a handler failed",
            );
            Err(unexpected("the request failed inside the daemon"))
        }
    }
}

/// [`guarded`] for a handler that takes the request's params.
async fn guarded_with<P: Send + 'static, T: Send + 'static>(
    core: &Arc<DaemonCore>,
    handlers: &Arc<dyn Handlers>,
    context: RequestContext,
    params: P,
    call: for<'a> fn(
        &'a dyn Handlers,
        &'a DaemonCore,
        &'a RequestContext,
        P,
    ) -> HandlerFuture<'a, T>,
) -> Result<T, Box<ErrorBody>> {
    let core = Arc::clone(core);
    let handlers = Arc::clone(handlers);
    let connection_id = context.connection_id;
    let request_id = context.request_id.clone();
    let task = tokio::spawn(async move { call(&*handlers, &core, &context, params).await });
    match task.await {
        Ok(result) => result,
        Err(failure) => {
            tracing::error!(
                connection_id,
                request_id = request_id.as_str(),
                panicked = failure.is_panic(),
                "a handler failed",
            );
            Err(unexpected("the request failed inside the daemon"))
        }
    }
}

/// A request for the test's starting method. provisional: F10
struct StartingFrame {
    request_id: String,
    idempotency_key: Option<String>,
    params: Option<serde_json::Value>,
}

/// Reads `bytes` as a request for the test's starting method `method`;
/// `None` when it names another method, so the contract decode handles it.
/// A request whose `requestId` or `idempotencyKey` breaks the protocol's
/// format still comes back, with the bad part left out, so it gets
/// `request.malformed` there. provisional: F10
fn starting_frame(bytes: &[u8], method: &str) -> Option<StartingFrame> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if value["call"]["type"].as_str() != Some(method) {
        return None;
    }
    Some(StartingFrame {
        request_id: value["requestId"].as_str().unwrap_or_default().to_owned(),
        idempotency_key: match &value["idempotencyKey"] {
            serde_json::Value::Null => None,
            other => Some(other.as_str().unwrap_or_default().to_owned()),
        },
        params: value["call"].get("content").cloned(),
    })
}

/// Admits and runs a request for the test's starting method like a
/// contract request. provisional: F10
async fn on_starting(
    frame: StartingFrame,
    starting: Arc<dyn TestStartingMethod>,
    conn: &Arc<Conn>,
    core: &Arc<DaemonCore>,
    received: Instant,
) {
    let method = starting.method();
    let key_ok = frame
        .idempotency_key
        .as_deref()
        .is_none_or(json::is_idempotency_key);
    if !json::is_request_id(&frame.request_id) || !key_ok {
        core.counters().frame_rejected();
        let request_id = json::is_request_id(&frame.request_id).then(|| frame.request_id.clone());
        let error = ErrorBody::new(
            &request::MALFORMED,
            "requestId or idempotencyKey has a bad format",
        );
        log_request(
            conn,
            request_id.as_deref(),
            None,
            received,
            Some(&error),
            None,
        );
        let _ = conn
            .enqueue(&ServerFrame::Error(ErrorFrame { request_id, error }))
            .await;
        return;
    }
    let Some((permit, guard)) = admit(conn, core, &frame.request_id, method, false, received).await
    else {
        return;
    };
    tokio::spawn(run_starting(
        Arc::clone(conn),
        Arc::clone(core),
        starting,
        frame,
        permit,
        guard,
        received,
    ));
}

/// Runs one request for the test's starting method: refused in recovery
/// mode like every method that writes, then [`start_operation`] on its own
/// task, and a raw response frame whose reply `type` is the method and
/// whose `content` is the [`OperationAccepted`](stackly_protocol::operation::OperationAccepted)
/// (null when nothing was started). provisional: F10
async fn run_starting(
    conn: Arc<Conn>,
    core: Arc<DaemonCore>,
    starting: Arc<dyn TestStartingMethod>,
    frame: StartingFrame,
    permit: OwnedSemaphorePermit,
    guard: DrainGuard,
    received: Instant,
) {
    let method = starting.method();
    let request_id = frame.request_id.clone();
    let context = RequestContext::new(conn.id, request_id.clone(), conn.peer, conn.client());
    let result = match recovery_refusal(&core, false) {
        Some(error) => Err(error),
        None => {
            let core = Arc::clone(&core);
            let task = tokio::spawn(async move {
                let prepared = starting.prepare(frame.params.as_ref())?;
                let start = StartRequest::new(
                    method,
                    frame.idempotency_key.as_deref(),
                    prepared.fingerprint,
                );
                start_operation(&core, &context, start, prepared.build).await
            });
            task.await.unwrap_or_else(|failure| {
                tracing::error!(
                    connection_id = conn.id,
                    request_id = request_id.as_str(),
                    panicked = failure.is_panic(),
                    "a handler failed",
                );
                Err(unexpected("the request failed inside the daemon"))
            })
        }
    };
    let (bytes, error, operation_id) = match result {
        Ok(outcome) => {
            let content = match &outcome {
                StartOutcome::Accepted(accepted) => {
                    serde_json::to_value(accepted).expect("an accepted operation always serializes")
                }
                StartOutcome::NothingToDo => serde_json::Value::Null,
            };
            let frame = serde_json::json!({
                "type": "response",
                "content": {
                    "requestId": request_id,
                    "reply": { "type": method, "content": content },
                },
            });
            let bytes = serde_json::to_vec(&frame).expect("a json value always serializes");
            (bytes, None, outcome.operation_id())
        }
        Err(error) => {
            let bytes = encode(&ServerFrame::Error(ErrorFrame {
                request_id: Some(request_id.clone()),
                error: (*error).clone(),
            }));
            (bytes, Some(*error), None)
        }
    };
    log_request(
        &conn,
        Some(&request_id),
        Some(method),
        received,
        error.as_ref(),
        operation_id,
    );
    let _ = conn.enqueue_bytes(bytes).await;
    conn.release_id(&request_id);
    drop(permit);
    drop(guard);
}

/// Forwards one subscription's events to the outbound queue, recording
/// the last sequence queued so the flush at stop can wait for
/// `daemon.shutting_down` to be queued.
async fn forward(conn: Arc<Conn>, mut subscription: BusSubscription, queued: Arc<AtomicU64>) {
    while let Some(event) = subscription.next().await {
        let sequence = event.sequence.get();
        if conn.enqueue(&ServerFrame::Event(event)).await.is_err() {
            break;
        }
        queued.fetch_max(sequence, Ordering::Release);
    }
}

/// The error to send for a failed read, or `None` when the client is
/// simply gone.
fn read_failure(error: &IpcError, core: &DaemonCore) -> Option<ErrorBody> {
    match error {
        IpcError::Frame(_) => {
            core.counters().frame_rejected();
            Some(error.to_error_body())
        }
        IpcError::FrameStalled | IpcError::HandshakeTimeout => Some(error.to_error_body()),
        _ => None,
    }
}

/// `protocol.major_mismatch` naming the daemon's version (spec 0006 AC-1).
fn major_mismatch() -> ErrorBody {
    let current = ProtocolVersion::CURRENT;
    ErrorBody::new(
        &protocol::MAJOR_MISMATCH,
        format!(
            "the daemon speaks protocol {}.{}",
            current.major, current.minor
        ),
    )
    .with_detail("serverMajor", current.major.to_string())
    .with_detail("serverMinor", current.minor.to_string())
}

fn handshake_required(method: Option<&str>) -> ErrorBody {
    let error = ErrorBody::new(
        &protocol::HANDSHAKE_REQUIRED,
        "the first request on a connection must be daemon.handshake",
    );
    match method {
        Some(method) => error.with_detail("method", method),
        None => error,
    }
}

fn unexpected(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&internal::UNEXPECTED, message))
}

fn encode(frame: &ServerFrame) -> Vec<u8> {
    serde_json::to_vec(frame)
        .expect("a server frame always serializes: every map key in it is a string")
}

const fn kind_name(kind: ClientKind) -> &'static str {
    match kind {
        ClientKind::Cli => "cli",
        ClientKind::Gui => "gui",
        ClientKind::Unrecognized => "unrecognized",
    }
}

fn log_refusal(conn: &Conn, request_id: Option<&str>, error: &ErrorBody) {
    tracing::info!(
        connection_id = conn.id,
        peer_pid = conn.peer.pid,
        request_id,
        outcome = "error",
        code = error.code(),
        "handshake refused",
    );
}

/// The line that closes one request: `debug` when it succeeded, `info`
/// when it failed, `error` for an `internal` failure. It carries
/// `operation_id` when the request names or starts an operation (spec
/// 0007 AC-19). Params, replies, and frame bytes are never logged.
fn log_request(
    conn: &Conn,
    request_id: Option<&str>,
    method: Option<&str>,
    received: Instant,
    error: Option<&ErrorBody>,
    operation_id: Option<Uuid>,
) {
    let duration_ms = u64::try_from(received.elapsed().as_millis()).unwrap_or(u64::MAX);
    let client_kind = conn.client_kind();
    let interactive = conn.interactive();
    let operation_id = operation_id.map(|id| id.to_string());
    let operation_id = operation_id.as_deref();
    match error {
        None => tracing::debug!(
            connection_id = conn.id,
            request_id,
            method,
            client_kind,
            interactive,
            operation_id,
            duration_ms,
            outcome = "ok",
            "request",
        ),
        Some(error) if error.category() == ErrorCategory::Internal => tracing::error!(
            connection_id = conn.id,
            request_id,
            method,
            client_kind,
            interactive,
            operation_id,
            duration_ms,
            outcome = "error",
            code = error.code(),
            "request",
        ),
        Some(error) => tracing::info!(
            connection_id = conn.id,
            request_id,
            method,
            client_kind,
            interactive,
            operation_id,
            duration_ms,
            outcome = "error",
            code = error.code(),
            "request",
        ),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    const METHOD: &str = "test.start_operation";

    fn frame(value: &serde_json::Value) -> Option<StartingFrame> {
        starting_frame(&serde_json::to_vec(value).unwrap(), METHOD)
    }

    #[test]
    fn a_request_for_another_method_or_broken_json_is_left_to_the_contract_decode() {
        let other = serde_json::json!({ "requestId": "r", "call": { "type": "daemon.get_info" } });

        assert!(frame(&other).is_none());
        assert!(starting_frame(b"{broken", METHOD).is_none());
        assert!(starting_frame(b"[]", METHOD).is_none());
    }

    #[test]
    fn a_request_for_the_starting_method_keeps_its_id_key_and_params() {
        let request = serde_json::json!({
            "requestId": "r-1",
            "idempotencyKey": "k-1",
            "call": { "type": METHOD, "content": { "script": "quick" } },
        });

        let read = frame(&request).unwrap();

        assert_eq!(read.request_id, "r-1");
        assert_eq!(read.idempotency_key.as_deref(), Some("k-1"));
        assert_eq!(read.params, Some(serde_json::json!({ "script": "quick" })));
    }

    #[test]
    fn a_missing_key_or_params_stay_absent() {
        let request = serde_json::json!({ "requestId": "r-2", "call": { "type": METHOD } });

        let read = frame(&request).unwrap();

        assert_eq!(read.idempotency_key, None);
        assert_eq!(read.params, None);
    }

    #[test]
    fn an_id_or_key_that_is_not_a_string_comes_back_empty_so_it_is_malformed() {
        let request = serde_json::json!({
            "requestId": 7,
            "idempotencyKey": 9,
            "call": { "type": METHOD },
        });

        let read = frame(&request).unwrap();

        assert!(!json::is_request_id(&read.request_id));
        assert_eq!(read.idempotency_key.as_deref(), Some(""));
        assert!(!json::is_idempotency_key(""));
    }
}
