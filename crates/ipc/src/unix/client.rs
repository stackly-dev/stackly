//! The verified client: endpoint checks, the handshake, calls matched to
//! replies by `requestId`, and event streams that skip kinds this build
//! does not know.
//!
//! One background task reads every daemon frame and routes it: a response
//! or error to the call waiting on its `requestId`, an event to its
//! subscription's queue. A connection level error (an error frame without
//! a `requestId`), the end of the stream, or a frame that does not decode
//! ends the task and fails every waiting call.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use stackly_protocol::codes::ipc;
use stackly_protocol::events::{SubscribeParams, Subscribed};
use stackly_protocol::handshake::{ClientInfo, HandshakeAccepted, HandshakeParams};
use stackly_protocol::json::{DecodedFrame, decode_server_frame};
use stackly_protocol::limits::EVENT_BUFFER_LEN;
use stackly_protocol::{
    CONTRACT_DIGEST, Call, ErrorBody, ErrorFrame, ProtocolVersion, Reply, Request, Response,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

use super::connection::{FrameReader, FrameWriter, FramedConnection};
use super::endpoint::{ConnectPolicy, connect_verified};
use crate::{IpcError, PENDING_READ_TIMEOUT};

/// Events queued per subscription before the reader waits for the
/// [`EventStream`] to take some, the same as the daemon's broadcast
/// buffer.
const EVENT_QUEUE_LEN: usize = EVENT_BUFFER_LEN as usize;

/// What a call is answered with: the reply, plus the event queue when the
/// reply started a subscription.
struct Answer {
    reply: Reply,
    events: Option<mpsc::Receiver<DecodedFrame>>,
}

type Answered = Result<Answer, Box<ErrorBody>>;

/// A call waiting for its reply.
struct Waiter {
    /// The method called, to check the reply's tag.
    method: &'static str,
    reply: oneshot::Sender<Answered>,
}

/// Why the connection ended.
enum Closed {
    /// The daemon ended the stream between frames.
    Eof,
    /// The client was dropped.
    Local,
    /// The connection broke with this error.
    Error(Box<ErrorBody>),
}

impl Closed {
    /// The error a call gets once the connection ended.
    fn call_error(&self) -> Box<ErrorBody> {
        match self {
            Self::Eof | Self::Local => Box::new(IpcError::ConnectionClosed.to_error_body()),
            Self::Error(error) => error.clone(),
        }
    }
}

#[derive(Default)]
struct State {
    pending: HashMap<String, Waiter>,
    subscriptions: HashMap<u32, mpsc::Sender<DecodedFrame>>,
    closed: Option<Closed>,
}

/// The state the client, its reader task, and its event streams share.
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds a waiting call, unless the connection already ended. Both
    /// happen under one lock with [`Self::close`], so no waiter is left
    /// behind unanswered.
    fn register(&self, request_id: String, waiter: Waiter) -> Result<(), Box<ErrorBody>> {
        let mut state = self.lock();
        if let Some(closed) = &state.closed {
            return Err(closed.call_error());
        }
        state.pending.insert(request_id, waiter);
        Ok(())
    }

    fn forget(&self, request_id: &str) {
        self.lock().pending.remove(request_id);
    }

    /// Hands a response to its call. A subscription's queue is registered
    /// here, before the reader reads the next frame, so its first event
    /// always finds it.
    fn answer(&self, response: Response) {
        let mut state = self.lock();
        let Some(waiter) = state.pending.remove(&response.request_id) else {
            tracing::debug!(request_id = %response.request_id, "reply for a call that stopped waiting");
            return;
        };
        if response.reply.method() != waiter.method {
            let _ = waiter.reply.send(Err(violation(
                "the daemon reply does not match the method called",
            )));
            return;
        }
        let subscription = match &response.reply {
            Reply::EventsSubscribe(subscribed) => Some(subscribed.subscription_id),
            _ => None,
        };
        let events = subscription.map(|id| {
            let (sender, receiver) = mpsc::channel(EVENT_QUEUE_LEN);
            state.subscriptions.insert(id, sender);
            receiver
        });
        let answer = Answer {
            reply: response.reply,
            events,
        };
        if waiter.reply.send(Ok(answer)).is_err()
            && let Some(id) = subscription
        {
            state.subscriptions.remove(&id);
        }
    }

    /// Hands an error frame to its call.
    fn fail(&self, request_id: &str, error: ErrorBody) {
        let waiter = self.lock().pending.remove(request_id);
        match waiter {
            Some(waiter) => {
                let _ = waiter.reply.send(Err(Box::new(error)));
            }
            None => {
                tracing::debug!(
                    request_id,
                    code = error.code(),
                    "error for a call that stopped waiting"
                );
            }
        }
    }

    fn subscription(&self, id: u32) -> Option<mpsc::Sender<DecodedFrame>> {
        self.lock().subscriptions.get(&id).cloned()
    }

    fn unsubscribe(&self, id: u32) {
        self.lock().subscriptions.remove(&id);
    }

    /// Ends the connection once: fails every waiting call and closes every
    /// event queue.
    fn close(&self, closed: Closed) {
        let mut state = self.lock();
        if state.closed.is_some() {
            return;
        }
        for (_, waiter) in state.pending.drain() {
            let _ = waiter.reply.send(Err(closed.call_error()));
        }
        state.subscriptions.clear();
        state.closed = Some(closed);
    }

    /// The error that ended the connection, for an event stream that ran
    /// dry. `None` for a clean end.
    fn stream_error(&self) -> Option<Box<ErrorBody>> {
        match &self.lock().closed {
            Some(Closed::Error(error)) => Some(error.clone()),
            _ => None,
        }
    }
}

/// Removes a waiting call when its future is dropped, for example by a
/// timeout around [`Client::call`].
struct Pending<'a> {
    shared: &'a Shared,
    request_id: &'a str,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.shared.forget(self.request_id);
    }
}

/// A connection to the daemon that passed the endpoint checks and the
/// handshake. Credentials and mutations go only through a `Client`, so
/// they are only ever sent after both (REQ-016, AC-070).
///
/// Calls may run concurrently from several tasks; each gets its own
/// lowercase UUID v7 `requestId`. Dropping the client closes the
/// connection and ends its event streams.
pub struct Client {
    writer: tokio::sync::Mutex<FrameWriter>,
    shared: Arc<Shared>,
    reader: JoinHandle<()>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    /// Connects to the socket at `path` through
    /// [`connect_verified`](crate::connect_verified), then sends the
    /// handshake with [`ProtocolVersion::CURRENT`], `client`, and
    /// [`CONTRACT_DIGEST`].
    ///
    /// # Errors
    ///
    /// The errors of [`connect_verified`](crate::connect_verified), and
    /// [`IpcError::Wire`] with the daemon's error (such as
    /// `protocol.major_mismatch` or `ipc.peer_rejected`),
    /// `ipc.connection_closed`, or `ipc.protocol_violation`.
    pub async fn connect(
        path: &Path,
        policy: ConnectPolicy,
        client: ClientInfo,
    ) -> Result<(Self, HandshakeAccepted), IpcError> {
        let connection = connect_verified(path, policy).await?;
        Self::handshake(connection, client).await
    }

    async fn handshake(
        connection: FramedConnection,
        client: ClientInfo,
    ) -> Result<(Self, HandshakeAccepted), IpcError> {
        let (reader, writer) = connection.into_split();
        let shared = Arc::new(Shared::default());
        let task = tokio::spawn(read_loop(reader, Arc::clone(&shared)));
        let this = Self {
            writer: tokio::sync::Mutex::new(writer),
            shared,
            reader: task,
        };
        let params = HandshakeParams {
            protocol: ProtocolVersion::CURRENT,
            client,
            contract_digest: CONTRACT_DIGEST.to_owned(),
        };
        match this.call(Call::DaemonHandshake(params)).await {
            Ok(Reply::DaemonHandshake(accepted)) => Ok((this, accepted)),
            Ok(_) => Err(IpcError::Wire(violation(
                "the daemon answered the handshake with another reply",
            ))),
            Err(error) => Err(IpcError::Wire(error)),
        }
    }

    /// Calls one method and waits for its reply.
    ///
    /// When writing fails because the daemon closed the connection, the
    /// client keeps reading for up to
    /// [`PENDING_READ_TIMEOUT`](crate::PENDING_READ_TIMEOUT) and reports the
    /// daemon's error frame when one arrives.
    ///
    /// # Errors
    ///
    /// Returns the daemon's error for this call; `ipc.protocol_violation`
    /// when the reply's tag does not match the call or a daemon frame does
    /// not decode; `ipc.frame_too_large` when the request is above the
    /// frame limit, before anything is sent; `ipc.connection_closed` when
    /// the connection ended before the reply; or the connection level error
    /// that ended it. The error is boxed, like every `Result` carrying an
    /// [`ErrorBody`].
    pub async fn call(&self, call: Call) -> Result<Reply, Box<ErrorBody>> {
        self.request(call).await.map(|answer| answer.reply)
    }

    /// Starts an event subscription.
    ///
    /// Read the stream steadily: while its queue (1024 events) is full, the
    /// reader task waits for it, and replies to later calls wait behind
    /// that event. The daemon then sees a slow subscriber and sends
    /// `events.gap` with reason `overflow`.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::call`], such as
    /// `events.too_many_subscriptions`.
    pub async fn subscribe(
        &self,
        params: SubscribeParams,
    ) -> Result<(Subscribed, EventStream), Box<ErrorBody>> {
        match self.request(Call::EventsSubscribe(params)).await? {
            Answer {
                reply: Reply::EventsSubscribe(subscribed),
                events: Some(events),
            } => Ok((
                subscribed,
                EventStream {
                    subscription_id: subscribed.subscription_id,
                    events,
                    shared: Arc::clone(&self.shared),
                },
            )),
            _ => Err(violation(
                "the daemon answered a subscribe with another reply",
            )),
        }
    }

    async fn request(&self, call: Call) -> Answered {
        let request_id = Uuid::now_v7().to_string();
        let (sender, receiver) = oneshot::channel();
        let waiter = Waiter {
            method: call.method(),
            reply: sender,
        };
        self.shared.register(request_id.clone(), waiter)?;
        let _pending = Pending {
            shared: &self.shared,
            request_id: &request_id,
        };
        let request = Request {
            request_id: request_id.clone(),
            idempotency_key: None,
            call,
        };
        let payload = serde_json::to_vec(&request)
            .expect("a request always serializes: every map key in it is a string");
        let written = self.writer.lock().await.write_frame(&payload).await;
        match written {
            Ok(()) => receiver.await.unwrap_or_else(|_| Err(closed())),
            Err(IpcError::Frame(error)) => Err(Box::new(error.to_error_body())),
            Err(_) => match timeout(PENDING_READ_TIMEOUT, receiver).await {
                Ok(Ok(answered)) => answered,
                Ok(Err(_)) | Err(_) => Err(closed()),
            },
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.reader.abort();
        self.shared.close(Closed::Local);
    }
}

/// The events of one subscription, in sequence order: known kinds as
/// [`DecodedFrame::Event`], unknown ones as [`DecodedFrame::SkippedEvent`],
/// whose sequence still advances the cursor.
///
/// Dropping the stream stops queueing its events. It does not end the
/// subscription on the daemon; call `events.unsubscribe` for that.
pub struct EventStream {
    subscription_id: u32,
    events: mpsc::Receiver<DecodedFrame>,
    shared: Arc<Shared>,
}

impl fmt::Debug for EventStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStream")
            .field("subscription_id", &self.subscription_id)
            .finish_non_exhaustive()
    }
}

impl EventStream {
    /// The subscription this stream belongs to.
    #[must_use]
    pub const fn subscription_id(&self) -> u32 {
        self.subscription_id
    }

    /// Waits for the next event. `Ok(None)` means the stream ended cleanly:
    /// the daemon closed the connection between frames, or the client was
    /// dropped. Events queued before the end are always handed out first.
    ///
    /// # Errors
    ///
    /// Returns the error that broke the connection, such as
    /// `ipc.protocol_violation` or a connection level error frame.
    pub async fn next(&mut self) -> Result<Option<DecodedFrame>, Box<ErrorBody>> {
        match self.events.recv().await {
            Some(frame) => Ok(Some(frame)),
            None => self.shared.stream_error().map_or(Ok(None), Err),
        }
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.shared.unsubscribe(self.subscription_id);
    }
}

/// Reads and routes every daemon frame until the connection ends.
async fn read_loop(mut reader: FrameReader, shared: Arc<Shared>) {
    let closed = loop {
        let payload = match reader.read_frame(None).await {
            Ok(Some(payload)) => payload,
            Ok(None) => break Closed::Eof,
            Err(error) => break Closed::Error(Box::new(read_failure(&error))),
        };
        match decode_server_frame(&payload) {
            Ok(DecodedFrame::Response(response)) => shared.answer(response),
            Ok(DecodedFrame::Error(ErrorFrame {
                request_id: Some(request_id),
                error,
            })) => shared.fail(&request_id, error),
            Ok(DecodedFrame::Error(ErrorFrame {
                request_id: None,
                error,
            })) => break Closed::Error(Box::new(error)),
            Ok(event) => deliver(&shared, event).await,
            Err(violation) => break Closed::Error(violation),
        }
    };
    shared.close(closed);
}

/// Queues an event for its subscription; events for a subscription this
/// client no longer reads are dropped.
async fn deliver(shared: &Shared, event: DecodedFrame) {
    let id = match &event {
        DecodedFrame::Event(event) => event.subscription_id,
        DecodedFrame::SkippedEvent {
            subscription_id, ..
        } => *subscription_id,
        DecodedFrame::Response(_) | DecodedFrame::Error(_) => return,
    };
    if let Some(sender) = shared.subscription(id)
        && sender.send(event).await.is_err()
    {
        shared.unsubscribe(id);
    }
}

/// What a read failure means to calls still waiting: a broken frame from
/// the daemon is a protocol violation; anything else closed the
/// connection, so the call may have run.
fn read_failure(error: &IpcError) -> ErrorBody {
    match error {
        IpcError::Frame(_) | IpcError::FrameStalled => ErrorBody::new(
            &ipc::PROTOCOL_VIOLATION,
            "the daemon sent a frame the codec refuses or never finished it",
        ),
        _ => IpcError::ConnectionClosed.to_error_body(),
    }
}

fn violation(message: &str) -> Box<ErrorBody> {
    Box::new(ErrorBody::new(&ipc::PROTOCOL_VIOLATION, message))
}

fn closed() -> Box<ErrorBody> {
    Box::new(IpcError::ConnectionClosed.to_error_body())
}
