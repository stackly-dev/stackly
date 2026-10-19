use std::io;
use std::path::{Path, PathBuf};

use stackly_protocol::ErrorBody;
use stackly_protocol::codes::{UntrustedReason, ipc};
use stackly_protocol::frame::FrameError;

/// The `op` of an [`IpcError::Io`] that failed reading a connection.
const READ_OP: &str = "read";
/// The `op` of an [`IpcError::Io`] that failed writing a connection.
const WRITE_OP: &str = "write";

/// What went wrong in the transport. [`IpcError::to_error_body`] turns
/// every variant into the catalog error the CLI and the GUI work with.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IpcError {
    /// Nothing exists at the socket path: `ipc.endpoint_missing`.
    #[error("nothing exists at the socket path {}", .path.display())]
    EndpointMissing {
        /// The socket path.
        path: PathBuf,
    },
    /// The socket path or the server peer failed a trust check:
    /// `ipc.endpoint_untrusted` (AC-070).
    #[error("the socket endpoint is not trusted: {}", .reason.as_str())]
    EndpointUntrusted {
        /// The socket path, when the check knew it. [`verify_peer`] only
        /// sees the stream, and [`connect_verified`] fills the path in.
        ///
        /// [`verify_peer`]: crate::verify_peer
        /// [`connect_verified`]: crate::connect_verified
        path: Option<PathBuf>,
        /// Which check failed.
        reason: UntrustedReason,
    },
    /// Connecting was refused, so no process listens on the socket:
    /// `ipc.connection_refused`.
    #[error("connecting to {} was refused", .path.display())]
    ConnectionRefused {
        /// The socket path.
        path: PathBuf,
    },
    /// The peer closed the connection in the middle of a frame, or before
    /// a reply arrived: `ipc.connection_closed`.
    #[error("the connection closed before the frame or reply was complete")]
    ConnectionClosed,
    /// Another OS error. `op` names the step, one of `stat`, `connect`,
    /// `bind`, `chmod`, `rename`, `unlink`, `accept`, `read`, or `write`.
    /// It maps to `ipc.io_error`, except `read` and `write`: they fail on a
    /// connection that may already carry the request, so they map to
    /// `ipc.connection_closed`, which is not retryable.
    #[error("{op} failed: {source}")]
    Io {
        /// The step that failed.
        op: &'static str,
        /// The OS error.
        #[source]
        source: io::Error,
    },
    /// The socket path holds something that is not a socket, so the
    /// listener removes nothing and does not bind: `ipc.endpoint_occupied`.
    #[error("the socket path {} holds a {kind}, not a socket", .path.display())]
    EndpointOccupied {
        /// The path that holds it: the socket path or its temporary name.
        path: PathBuf,
        /// What it holds: `file`, `directory`, `symlink`, `fifo`,
        /// `block_device`, `char_device`, or `other`.
        kind: &'static str,
    },
    /// A frame header announced a length the codec refuses:
    /// `ipc.frame_empty` or `ipc.frame_too_large`.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// A frame that had started was still incomplete after
    /// [`FRAME_STALL_TIMEOUT`](crate::FRAME_STALL_TIMEOUT):
    /// `ipc.frame_stalled`.
    #[error("a started frame was still incomplete after 30 seconds")]
    FrameStalled,
    /// The frame was not complete by the deadline the caller gave, which
    /// is the daemon's handshake deadline: `ipc.handshake_timeout`.
    #[error("the handshake frame was not complete in time")]
    HandshakeTimeout,
    /// An error already in wire form: one the daemon sent, or a protocol
    /// violation found while reading the daemon's frames.
    #[error("{}: {}", .0.code(), .0.message())]
    Wire(Box<ErrorBody>),
}

impl IpcError {
    /// The catalog error for this failure, with the `details` its code
    /// allows. Paths travel in `details.path` as lossy text, never in the
    /// message.
    #[must_use]
    pub fn to_error_body(&self) -> ErrorBody {
        match self {
            Self::EndpointMissing { path } => {
                ErrorBody::new(&ipc::ENDPOINT_MISSING, "nothing exists at the socket path")
                    .with_detail("path", path_text(path))
            }
            Self::EndpointUntrusted { path, reason } => {
                let body = ErrorBody::new(&ipc::ENDPOINT_UNTRUSTED, untrusted_message(*reason))
                    .with_detail("reason", reason.as_str());
                match path {
                    Some(path) => body.with_detail("path", path_text(path)),
                    None => body,
                }
            }
            Self::ConnectionRefused { path } => ErrorBody::new(
                &ipc::CONNECTION_REFUSED,
                "connecting to the socket was refused, so no daemon listens on it",
            )
            .with_detail("path", path_text(path)),
            Self::ConnectionClosed => ErrorBody::new(
                &ipc::CONNECTION_CLOSED,
                "the daemon closed the connection before replying",
            ),
            Self::Io {
                op: op @ (READ_OP | WRITE_OP),
                ..
            } => ErrorBody::new(
                &ipc::CONNECTION_CLOSED,
                format!("{op} on the daemon connection failed, so the request may have run"),
            ),
            Self::Io { op, source } => {
                ErrorBody::new(&ipc::IO_ERROR, format!("{op} failed with an os error"))
                    .with_detail("op", *op)
                    .with_detail("kind", io_kind_name(source.kind()))
            }
            Self::EndpointOccupied { path, kind } => ErrorBody::new(
                &ipc::ENDPOINT_OCCUPIED,
                "the socket path holds something that is not a socket",
            )
            .with_detail("path", path_text(path))
            .with_detail("kind", *kind),
            Self::Frame(error) => error.to_error_body(),
            Self::FrameStalled => ErrorBody::new(
                &ipc::FRAME_STALLED,
                "a started frame was still incomplete after 30 seconds",
            ),
            Self::HandshakeTimeout => ErrorBody::new(
                &ipc::HANDSHAKE_TIMEOUT,
                "the handshake frame was not complete in time",
            ),
            Self::Wire(body) => (**body).clone(),
        }
    }

    /// Fills in the socket path of an [`IpcError::EndpointUntrusted`] that
    /// came from a check on the stream alone.
    pub(crate) fn at_path(self, at: &Path) -> Self {
        match self {
            Self::EndpointUntrusted { path: None, reason } => Self::EndpointUntrusted {
                path: Some(at.to_path_buf()),
                reason,
            },
            other => other,
        }
    }
}

/// The message of `ipc.endpoint_untrusted` for each reason.
fn untrusted_message(reason: UntrustedReason) -> &'static str {
    match reason {
        UntrustedReason::NotASocket => "the socket path is not a socket",
        UntrustedReason::Symlink => "the socket path or its folder is a symbolic link",
        UntrustedReason::WrongOwner => "another user owns the socket",
        UntrustedReason::GroupOrOtherAccess => "the socket grants access to its group or to others",
        UntrustedReason::ParentWrongOwner => "another user owns the folder holding the socket",
        UntrustedReason::ParentGroupOrOtherAccess => {
            "the folder holding the socket grants access to its group or to others"
        }
        UntrustedReason::PeerUidMismatch => "the server process runs as another user",
        UntrustedReason::PeerCredentialsUnavailable => {
            "the credentials of the server process cannot be read"
        }
        _ => "the socket endpoint failed a trust check",
    }
}

/// A path as wire text: lossy UTF-8, as resolved, never canonicalized.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `ErrorKind` in snake case, such as `permission_denied`: the
/// `details.kind` of `ipc.io_error`. The Swift client maps errno to the same
/// names (spec 0013, `protocols/fixtures/endpoint/checks.json`).
#[must_use]
pub fn io_kind_name(kind: io::ErrorKind) -> String {
    let camel = format!("{kind:?}");
    let mut snake = String::with_capacity(camel.len() + 4);
    for (index, c) in camel.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        } else {
            snake.push(c);
        }
    }
    snake
}

#[cfg(test)]
mod tests {
    use stackly_protocol::codes;

    use super::*;

    fn every_variant() -> Vec<IpcError> {
        let path = PathBuf::from("/tmp/skt-x/stackly-0011223344556677/stacklyd.sock");
        let mut errors = vec![
            IpcError::EndpointMissing { path: path.clone() },
            IpcError::ConnectionRefused { path: path.clone() },
            IpcError::ConnectionClosed,
            IpcError::Io {
                op: "stat",
                source: io::Error::from(io::ErrorKind::PermissionDenied),
            },
            IpcError::EndpointOccupied {
                path: path.clone(),
                kind: "file",
            },
            IpcError::Frame(FrameError::Empty),
            IpcError::Frame(FrameError::TooLarge {
                len: 1_048_577,
                max: 1_048_576,
            }),
            IpcError::FrameStalled,
            IpcError::HandshakeTimeout,
            IpcError::Wire(Box::new(ErrorBody::new(
                &codes::protocol::MAJOR_MISMATCH,
                "the daemon speaks protocol major 1",
            ))),
        ];
        for reason in UntrustedReason::ALL {
            errors.push(IpcError::EndpointUntrusted {
                path: Some(path.clone()),
                reason,
            });
            errors.push(IpcError::EndpointUntrusted { path: None, reason });
        }
        errors
    }

    #[test]
    fn every_client_side_error_is_a_catalog_error_with_allowed_details() {
        for error in every_variant() {
            let body = error.to_error_body();
            let spec = body
                .spec()
                .unwrap_or_else(|| panic!("{} is not in the catalog", body.code()));
            assert_eq!(body.category(), spec.category, "{error:?}");
            assert_eq!(body.retryable(), spec.retryable, "{error:?}");
            for detail in body.details() {
                assert!(
                    spec.details.contains(&detail.key()),
                    "{} carries {} it may not",
                    body.code(),
                    detail.key()
                );
            }
            let message = body.message();
            assert!(!message.is_empty() && message.chars().count() <= 200);
            assert!(
                message.starts_with(|c: char| c.is_ascii_lowercase()),
                "{message}"
            );
            assert!(!message.ends_with('.'), "{message}");
            assert!(
                !message.contains("skt-x"),
                "paths stay out of messages: {message}"
            );
        }
    }

    #[test]
    fn each_variant_maps_to_its_code() {
        let codes: Vec<String> = every_variant()
            .iter()
            .map(|error| error.to_error_body().code().to_owned())
            .collect();
        assert_eq!(
            &codes[..10],
            [
                "ipc.endpoint_missing",
                "ipc.connection_refused",
                "ipc.connection_closed",
                "ipc.io_error",
                "ipc.endpoint_occupied",
                "ipc.frame_empty",
                "ipc.frame_too_large",
                "ipc.frame_stalled",
                "ipc.handshake_timeout",
                "protocol.major_mismatch",
            ]
        );
        assert!(
            codes[10..]
                .iter()
                .all(|code| code == "ipc.endpoint_untrusted")
        );
    }

    #[test]
    fn io_errors_carry_the_step_and_the_kind_in_snake_case() {
        let body = IpcError::Io {
            op: "connect",
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        }
        .to_error_body();
        assert_eq!(body.detail("op"), Some("connect"));
        assert_eq!(body.detail("kind"), Some("permission_denied"));
        assert_eq!(body.message(), "connect failed with an os error");
        assert_eq!(io_kind_name(io::ErrorKind::NotFound), "not_found");
        assert_eq!(io_kind_name(io::ErrorKind::Other), "other");
    }

    #[test]
    fn read_and_write_failures_map_to_the_connection_closed_code() {
        for op in ["read", "write"] {
            let body = IpcError::Io {
                op,
                source: io::Error::from(io::ErrorKind::ConnectionReset),
            }
            .to_error_body();
            assert_eq!(body.code(), "ipc.connection_closed", "{op}");
            assert!(!body.retryable(), "{op}");
            assert!(body.details().is_empty(), "{op}");
            assert!(body.message().starts_with(op), "{}", body.message());
        }
        for op in [
            "stat", "connect", "bind", "chmod", "rename", "unlink", "accept",
        ] {
            let body = IpcError::Io {
                op,
                source: io::Error::from(io::ErrorKind::PermissionDenied),
            }
            .to_error_body();
            assert_eq!(body.code(), "ipc.io_error", "{op}");
            assert_eq!(body.detail("op"), Some(op));
        }
    }

    #[test]
    fn an_untrusted_endpoint_names_its_reason_and_the_path_when_known() {
        let path = PathBuf::from("/tmp/skt-y/stacklyd.sock");
        let body = IpcError::EndpointUntrusted {
            path: None,
            reason: UntrustedReason::PeerUidMismatch,
        }
        .at_path(&path)
        .to_error_body();
        assert_eq!(body.detail("reason"), Some("peer_uid_mismatch"));
        assert_eq!(body.detail("path"), Some("/tmp/skt-y/stacklyd.sock"));

        let without = IpcError::EndpointUntrusted {
            path: None,
            reason: UntrustedReason::PeerCredentialsUnavailable,
        }
        .to_error_body();
        assert_eq!(without.detail("path"), None);
        assert_eq!(
            without.detail("reason"),
            Some("peer_credentials_unavailable")
        );
    }

    #[test]
    fn a_wire_error_is_passed_through_unchanged() {
        let sent = ErrorBody::new(&codes::ipc::PEER_REJECTED, "peer rejected");
        let body = IpcError::Wire(Box::new(sent.clone())).to_error_body();
        assert_eq!(body, sent);
    }
}
