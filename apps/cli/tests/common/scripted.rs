//! A scripted daemon on the real `stackly_ipc::Listener`, bound at the
//! test root's `daemon_socket()`. It answers the handshake the way a test
//! says, then runs the test's handler for every request: a reply, an error
//! with any code (synthetic ones such as `test.failed` included), a pause,
//! no answer, a closed connection, or events. It records the method,
//! `requestId`, idempotency key, and raw JSON of every request.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use stackly_ipc::{Accepted, FrameWriter, Listener};
use stackly_protocol::events::{Event, EventCursor, EventKind};
use stackly_protocol::handshake::{DaemonIdentity, HandshakeAccepted};
use stackly_protocol::{
    CONTRACT_DIGEST, Call, ErrorBody, ErrorFrame, Limits, ProtocolVersion, Reply, Request,
    Response, Sequence, ServerFrame, Timestamp,
};
use stackly_testkit::TestRoot;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use super::{HandMadeRuntime, paths, uid};

/// The server instance of the first connection; later ones count up.
pub const SERVER_INSTANCE: Uuid = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d0e);

/// How the daemon answers a handshake.
#[derive(Debug, Clone)]
pub enum Handshake {
    /// Accept with this answer.
    Accept(Box<HandshakeAccepted>),
    /// Close the connection with this error.
    Refuse(Box<ErrorBody>),
    /// Never answer.
    Hang,
}

/// The usual handshake answer for connection `index`, with its own server
/// instance.
pub fn accepted(index: usize) -> Handshake {
    Handshake::Accept(Box::new(handshake_answer(index, false)))
}

/// A handshake answer for connection `index`.
pub fn handshake_answer(index: usize, contract_drift: bool) -> HandshakeAccepted {
    let instance = server_instance(index);
    HandshakeAccepted {
        protocol: ProtocolVersion::CURRENT,
        server_instance_id: instance,
        daemon: DaemonIdentity {
            version: "0.1.0".to_owned(),
            pid: 4242,
        },
        contract_digest: if contract_drift {
            "0".repeat(64)
        } else {
            CONTRACT_DIGEST.to_owned()
        },
        contract_drift,
        limits: Limits::CURRENT,
        head: EventCursor {
            server_instance_id: instance,
            sequence: Sequence::ZERO,
        },
    }
}

/// The server instance of connection `index`.
pub fn server_instance(index: usize) -> Uuid {
    Uuid::from_u128(SERVER_INSTANCE.as_u128() + index as u128)
}

/// One thing the daemon does for a request.
#[derive(Debug, Clone)]
pub enum Step {
    /// Answer with this reply.
    Reply(Reply),
    /// Answer with this error.
    Fail(ErrorBody),
    /// Wait before the next step.
    Sleep(Duration),
    /// Close the connection.
    Close,
    /// Send an event of this kind to the connection's last subscription.
    Event(EventKind),
}

/// What the handler knows about a request.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    /// The connection, counted from 0.
    pub connection: usize,
    /// How many requests with this method came before this one, on every
    /// connection.
    pub nth: usize,
}

/// One request the daemon read.
#[derive(Debug, Clone)]
pub struct Logged {
    /// The connection, counted from 0.
    pub connection: usize,
    /// The method.
    pub method: String,
    /// The `requestId`.
    pub request_id: String,
    /// The idempotency key.
    pub idempotency_key: Option<String>,
    /// The request as JSON.
    pub raw: Value,
}

type HandshakeFn = dyn Fn(usize) -> Handshake + Send + Sync;
type HandlerFn = dyn Fn(&Call, Ctx) -> Vec<Step> + Send + Sync;

struct Shared {
    log: Mutex<Vec<Logged>>,
    connections: AtomicUsize,
    handshake: Box<HandshakeFn>,
    handler: Box<HandlerFn>,
    sequence: AtomicUsize,
}

/// A running scripted daemon; dropping it stops it and removes its socket.
pub struct ScriptedDaemon {
    runtime: tokio::runtime::Runtime,
    shared: Arc<Shared>,
    accept: Option<tokio::task::JoinHandle<()>>,
    listener: Option<Arc<Listener>>,
    socket: PathBuf,
    _runtime_dir: Option<HandMadeRuntime>,
}

impl ScriptedDaemon {
    /// Binds the socket of `root` and serves with `handshake` and `handler`.
    pub fn start(
        root: &TestRoot,
        handshake: impl Fn(usize) -> Handshake + Send + Sync + 'static,
        handler: impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static,
    ) -> Self {
        let paths = paths(root);
        let runtime_dir = (!paths.runtime.exists()).then(|| HandMadeRuntime::new(&paths.runtime));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime starts");
        let shared = Arc::new(Shared {
            log: Mutex::new(Vec::new()),
            connections: AtomicUsize::new(0),
            handshake: Box::new(handshake),
            handler: Box::new(handler),
            sequence: AtomicUsize::new(0),
        });
        let mut daemon = Self {
            runtime,
            shared,
            accept: None,
            listener: None,
            socket: paths.socket,
            _runtime_dir: runtime_dir,
        };
        daemon.listen();
        daemon
    }

    /// A daemon that accepts every handshake and answers with `handler`.
    pub fn serve(
        root: &TestRoot,
        handler: impl Fn(&Call, Ctx) -> Vec<Step> + Send + Sync + 'static,
    ) -> Self {
        Self::start(root, accepted, handler)
    }

    /// Binds the socket and accepts connections again after [`Self::go_away`].
    pub fn listen(&mut self) {
        let listener = self
            .runtime
            .block_on(async { Listener::bind(&self.socket, uid()) })
            .map(Arc::new)
            .expect("the scripted daemon binds the socket");
        let shared = Arc::clone(&self.shared);
        let accepting = Arc::clone(&listener);
        self.accept = Some(self.runtime.spawn(async move {
            loop {
                let Ok(Accepted::Authorized { connection, .. }) = accepting.accept().await else {
                    continue;
                };
                let index = shared.connections.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve_connection(connection, index, Arc::clone(&shared)));
            }
        }));
        self.listener = Some(listener);
    }

    /// Stops accepting and removes the socket, so new connections are
    /// refused. Open connections stay until their handler closes them.
    pub fn go_away(&mut self) {
        if let Some(accept) = self.accept.take() {
            accept.abort();
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.unlink_if_own();
        }
    }

    /// Every request read so far.
    pub fn log(&self) -> Vec<Logged> {
        self.shared
            .log
            .lock()
            .expect("the log lock is never poisoned")
            .clone()
    }

    /// The methods read so far, in order, handshakes included.
    pub fn methods(&self) -> Vec<String> {
        self.log().into_iter().map(|logged| logged.method).collect()
    }

    /// How many connections were accepted.
    pub fn connections(&self) -> usize {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// The socket path.
    pub fn socket(&self) -> &std::path::Path {
        &self.socket
    }
}

impl Drop for ScriptedDaemon {
    fn drop(&mut self) {
        self.go_away();
    }
}

async fn serve_connection(
    connection: stackly_ipc::FramedConnection,
    index: usize,
    shared: Arc<Shared>,
) {
    let (mut reader, writer) = connection.into_split();
    let writer = Arc::new(AsyncMutex::new(writer));
    let mut subscription: Option<u32> = None;
    let mut handshaken = false;
    while let Ok(Some(frame)) = reader.read_frame(None).await {
        let raw: Value = serde_json::from_slice(&frame).unwrap_or(Value::Null);
        let Ok(request) = serde_json::from_slice::<Request>(&frame) else {
            continue;
        };
        let method = request.call.method().to_owned();
        let nth = {
            let mut log = shared.log.lock().expect("the log lock is never poisoned");
            let nth = log.iter().filter(|logged| logged.method == method).count();
            log.push(Logged {
                connection: index,
                method: method.clone(),
                request_id: request.request_id.clone(),
                idempotency_key: request.idempotency_key.clone(),
                raw,
            });
            nth
        };
        if !handshaken {
            handshaken = true;
            match (shared.handshake)(index) {
                Handshake::Accept(answer) => {
                    respond(
                        &writer,
                        &request.request_id,
                        Reply::DaemonHandshake(*answer),
                    )
                    .await;
                }
                Handshake::Refuse(error) => {
                    let mut writer = writer.lock().await;
                    let frame = ServerFrame::Error(ErrorFrame {
                        request_id: Some(request.request_id),
                        error: *error,
                    });
                    let _ = writer.write_server_frame(&frame).await;
                    let _ = writer.shutdown().await;
                    return;
                }
                Handshake::Hang => {}
            }
            continue;
        }
        let steps = (shared.handler)(
            &request.call,
            Ctx {
                connection: index,
                nth,
            },
        );
        for step in &steps {
            if let Step::Reply(Reply::EventsSubscribe(subscribed)) = step {
                subscription = Some(subscribed.subscription_id);
            }
        }
        let writer = Arc::clone(&writer);
        let shared = Arc::clone(&shared);
        let request_id = request.request_id;
        let closes = steps.iter().any(|step| matches!(step, Step::Close));
        let run = run_steps(steps, writer, request_id, subscription, shared);
        if closes {
            run.await;
            return;
        }
        tokio::spawn(run);
    }
}

async fn run_steps(
    steps: Vec<Step>,
    writer: Arc<AsyncMutex<FrameWriter>>,
    request_id: String,
    subscription: Option<u32>,
    shared: Arc<Shared>,
) {
    for step in steps {
        match step {
            Step::Reply(reply) => respond(&writer, &request_id, reply).await,
            Step::Fail(error) => {
                let frame = ServerFrame::Error(ErrorFrame {
                    request_id: Some(request_id.clone()),
                    error,
                });
                let _ = writer.lock().await.write_server_frame(&frame).await;
            }
            Step::Sleep(duration) => tokio::time::sleep(duration).await,
            Step::Close => {
                let _ = writer.lock().await.shutdown().await;
                return;
            }
            Step::Event(kind) => {
                let Some(subscription_id) = subscription else {
                    continue;
                };
                let next = shared.sequence.fetch_add(1, Ordering::SeqCst) + 1;
                let resource = match &kind {
                    EventKind::OperationStateChanged(summary)
                    | EventKind::OperationProgress(summary) => {
                        Some(stackly_protocol::ResourceRef {
                            kind: stackly_protocol::ResourceKind::Operation,
                            id: summary.id.to_string(),
                        })
                    }
                    _ => None,
                };
                let event = Event {
                    subscription_id,
                    sequence: Sequence::new(next as u64).expect("test sequences are small"),
                    at: Timestamp::UNIX_EPOCH,
                    resource,
                    revision: None,
                    kind,
                };
                let _ = writer
                    .lock()
                    .await
                    .write_server_frame(&ServerFrame::Event(event))
                    .await;
            }
        }
    }
}

async fn respond(writer: &AsyncMutex<FrameWriter>, request_id: &str, reply: Reply) {
    let frame = ServerFrame::Response(Response {
        request_id: request_id.to_owned(),
        reply,
    });
    let _ = writer.lock().await.write_server_frame(&frame).await;
}

/// An error body with any code and category, as a daemon of any version
/// may send it.
pub fn error(code: &str, category: &str, message: &str) -> ErrorBody {
    serde_json::from_value(serde_json::json!({
        "code": code,
        "category": category,
        "message": message,
        "retryable": false,
    }))
    .expect("a test error body decodes")
}
