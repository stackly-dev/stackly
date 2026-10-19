//! `stackly daemon status [--json]` (spec 0006, *CLI `stackly daemon
//! status`*).
//!
//! It derives the socket path from `STACKLY_ROOT` like the daemon does,
//! checks the endpoint, handshakes as client `cli`, then asks for
//! `daemon.get_info` and `daemon.get_health`. Any health status exits 0;
//! an error exits with the code of its category.
// provisional: F8 (spec 0008 replaces the output and the exit codes)

use std::ffi::OsString;
use std::process::ExitCode;

/// Runs the command and returns its exit code.
pub fn run(root_override: Option<OsString>, json: bool) -> ExitCode {
    ExitCode::from(imp::run(root_override, json))
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::OsString;
    use std::io::IsTerminal;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde::Serialize;
    use stackly_ipc::{Client, ConnectPolicy};
    use stackly_platform::{DirectoryLayout, UserId};
    use stackly_platform_macos::MacosPlatform;
    use stackly_protocol::codes::ipc;
    use stackly_protocol::daemon::{DaemonHealth, DaemonInfo, SubsystemStatus};
    use stackly_protocol::handshake::{ClientInfo, ClientKind, HandshakeAccepted};
    use stackly_protocol::{
        CONTRACT_DIGEST, Call, ErrorBody, ErrorCategory, ProtocolVersion, Reply,
    };
    use tokio::time::timeout;

    /// How long connecting and the handshake may take together.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    /// How long each call may take.
    const CALL_TIMEOUT: Duration = Duration::from_secs(10);
    /// The method a connect plus handshake that runs out of time names.
    const HANDSHAKE_METHOD: &str = "daemon.handshake";
    /// Exit code for a `STACKLY_ROOT` the daemon would refuse too.
    const EXIT_INVALID_INPUT: u8 = 2;
    /// Exit code when the CLI itself cannot run (no async runtime).
    const EXIT_INTERNAL: u8 = 1;

    /// The `--json` document.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Status<'a> {
        info: &'a DaemonInfo,
        health: Option<&'a DaemonHealth>,
        client: ClientBlock,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ClientBlock {
        protocol: ProtocolVersion,
        contract_digest: &'static str,
        contract_drift: bool,
    }

    pub(super) fn run(root_override: Option<OsString>, json: bool) -> u8 {
        let (socket, uid) = match endpoint(root_override) {
            Ok(endpoint) => endpoint,
            Err(message) => {
                eprintln!("stackly: {message}");
                return EXIT_INVALID_INPUT;
            }
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("stackly: cannot start the async runtime: {error}");
                return EXIT_INTERNAL;
            }
        };
        runtime.block_on(status(&socket, uid, json))
    }

    /// The socket path and the uid that must own it, from the same roots
    /// the daemon resolves. Nothing is created.
    fn endpoint(root_override: Option<OsString>) -> Result<(PathBuf, u32), String> {
        let platform = MacosPlatform::new(root_override).map_err(|error| error.to_string())?;
        let roots = platform
            .directories()
            .roots()
            .map_err(|error| error.to_string())?;
        let uid = match platform
            .identity()
            .current_user()
            .map_err(|error| error.to_string())?
        {
            UserId::Unix(uid) => uid,
            _ => return Err("the current user has no unix uid".to_owned()),
        };
        Ok((DirectoryLayout::new(roots).daemon_socket(), uid))
    }

    /// Whether the person at this CLI can answer a dialog: stdin and
    /// stderr are both terminals (spec 0006, *Handshake*; spec 0011).
    fn interactive() -> bool {
        std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }

    async fn status(socket: &Path, uid: u32, json: bool) -> u8 {
        let client = ClientInfo {
            kind: ClientKind::Cli,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: Some(std::process::id()),
            interactive: Some(interactive()),
        };
        let connected = timeout(
            CONNECT_TIMEOUT,
            Client::connect(socket, ConnectPolicy::same_user(uid), client),
        )
        .await;
        let (client, accepted) = match connected {
            Ok(Ok(connected)) => connected,
            Ok(Err(error)) => return fail(&error.to_error_body(), json, socket),
            Err(_) => {
                return fail(
                    &call_timeout(HANDSHAKE_METHOD, CONNECT_TIMEOUT),
                    json,
                    socket,
                );
            }
        };
        let info = match call(&client, Call::DaemonGetInfo).await {
            Ok(Reply::DaemonGetInfo(info)) => info,
            Ok(_) => return fail(&violation(), json, socket),
            Err(error) => return fail(&error, json, socket),
        };
        let health = match call(&client, Call::DaemonGetHealth).await {
            Ok(Reply::DaemonGetHealth(health)) => Ok(health),
            Ok(_) => Err(violation()),
            Err(error) => Err(error),
        };
        if json {
            print_json(&info, health.as_ref().ok(), &accepted);
        } else {
            print_text(&info, health.as_ref().ok(), &accepted);
        }
        match health {
            Ok(_) => 0,
            Err(error) => fail(&error, json, socket),
        }
    }

    async fn call(client: &Client, call: Call) -> Result<Reply, Box<ErrorBody>> {
        let method = call.method();
        match timeout(CALL_TIMEOUT, client.call(call)).await {
            Ok(result) => result,
            Err(_) => Err(Box::new(call_timeout(method, CALL_TIMEOUT))),
        }
    }

    /// `ipc.call_timeout` for `method` after `limit`. Connecting plus the
    /// handshake uses the method `daemon.handshake`; `ipc.handshake_timeout`
    /// stays the daemon's own code. These reads send no idempotency key.
    fn call_timeout(method: &str, limit: Duration) -> ErrorBody {
        ErrorBody::new(&ipc::CALL_TIMEOUT, "the daemon did not answer in time")
            .with_detail("method", method)
            .with_detail("seconds", limit.as_secs().to_string())
    }

    fn violation() -> Box<ErrorBody> {
        Box::new(ErrorBody::new(
            &ipc::PROTOCOL_VIOLATION,
            "the daemon answered with another reply",
        ))
    }

    fn print_json(info: &DaemonInfo, health: Option<&DaemonHealth>, accepted: &HandshakeAccepted) {
        let status = Status {
            info,
            health,
            client: ClientBlock {
                protocol: ProtocolVersion::CURRENT,
                contract_digest: CONTRACT_DIGEST,
                contract_drift: accepted.contract_drift,
            },
        };
        match serde_json::to_string(&status) {
            Ok(text) => println!("{text}"),
            Err(error) => eprintln!("stackly: cannot write the status as json: {error}"),
        }
    }

    fn print_text(info: &DaemonInfo, health: Option<&DaemonHealth>, accepted: &HandshakeAccepted) {
        let status =
            health.map_or_else(|| "unknown".to_owned(), |health| wire_name(&health.status));
        println!("status: {status}");
        println!("pid: {}", info.pid);
        println!(
            "uid: {}",
            info.uid
                .map_or_else(|| "-".to_owned(), |uid| uid.to_string())
        );
        println!("version: {}", info.version);
        println!("protocol: {}.{}", info.protocol.major, info.protocol.minor);
        if accepted.contract_drift {
            println!(
                "contract drift: the daemon uses another contract digest of this protocol version"
            );
        }
        println!("server instance: {}", info.server_instance_id);
        println!("started: {}", info.started_at);
        for subsystem in health.into_iter().flat_map(|health| &health.subsystems) {
            if subsystem.status != SubsystemStatus::Ok {
                let detail = subsystem
                    .detail
                    .as_deref()
                    .map_or_else(String::new, |detail| format!(" ({detail})"));
                println!(
                    "subsystem {}: {}{detail}",
                    subsystem.name,
                    wire_name(&subsystem.status)
                );
            }
        }
        println!("data root: {}", info.roots.data);
        println!("logs root: {}", info.roots.logs);
        println!("cache root: {}", info.roots.cache);
        println!("runtime root: {}", info.roots.runtime);
        println!("socket: {}", info.socket_path);
        println!(
            "root override: {}",
            if info.roots.root_override {
                "yes"
            } else {
                "no"
            }
        );
    }

    /// Prints the error to stderr and returns the exit code of its
    /// category.
    fn fail(error: &ErrorBody, json: bool, socket: &Path) -> u8 {
        if json {
            match serde_json::to_string(error) {
                Ok(text) => eprintln!("{text}"),
                Err(_) => eprintln!("stackly: {}", error.message()),
            }
        } else if error.is(&ipc::ENDPOINT_MISSING) || error.is(&ipc::CONNECTION_REFUSED) {
            eprintln!(
                "stackly: daemon is not running (no socket at {})",
                socket.display()
            );
            eprintln!("hint: start it with stacklyd");
        } else {
            eprintln!("stackly: {} ({})", error.message(), error.code());
            if let Some(hint) = error.hint() {
                eprintln!("hint: {hint}");
            }
        }
        exit_code(error.category())
    }

    /// The exit code of an error category (spec 0006; feature 8 sets the
    /// final values for `not_found` and `internal`).
    const fn exit_code(category: ErrorCategory) -> u8 {
        match category {
            ErrorCategory::InvalidInput | ErrorCategory::NotFound => 2,
            ErrorCategory::Conflict => 3,
            ErrorCategory::Unauthorized => 4,
            ErrorCategory::Unsupported => 5,
            ErrorCategory::Unavailable => 6,
            ErrorCategory::Failed => 7,
            ErrorCategory::Timeout => 8,
            ErrorCategory::Cancelled => 9,
            ErrorCategory::Internal => 1,
        }
    }

    /// The wire name of an enum value, such as `ready` or `not_built`.
    fn wire_name<T: Serialize>(value: &T) -> String {
        serde_json::to_value(value)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::ffi::OsString;

    pub(super) fn run(_root_override: Option<OsString>, _json: bool) -> u8 {
        eprintln!("stackly: the daemon only runs on macOS for now");
        5
    }
}
