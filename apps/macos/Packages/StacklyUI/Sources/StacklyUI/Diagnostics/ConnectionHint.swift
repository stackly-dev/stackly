import Foundation

/// The plain words for connection problems: why a socket was not trusted, and what to do
/// about each connection error code (spec 0013, Diagnostics koneksi).
nonisolated public enum ConnectionHint {
    /// Why the socket failed its security check, in lowercase words for the middle of a
    /// sentence, from the daemon's `details.reason`.
    ///
    /// - Parameter cause: The untrusted cause.
    public static func untrustedReason(_ cause: ShellStatus.UntrustedCause) -> String {
        switch cause {
        case .peerRejected:
            return "stacklyd rejected this app’s user"
        case .endpoint(let reason):
            switch reason {
            case "not_a_socket": return "the path isn’t a socket"
            case "symlink": return "the path is a symbolic link"
            case "wrong_owner": return "the socket belongs to another user"
            case "group_or_other_access": return "other users can access the socket"
            case "parent_wrong_owner": return "the socket’s folder isn’t a folder owned by you"
            case "parent_group_or_other_access": return "other users can access the socket’s folder"
            case "peer_uid_mismatch": return "the daemon runs as another user"
            case "peer_credentials_unavailable": return "macOS didn’t report who runs the daemon"
            default: return "unexpected reason \(reason)"
            }
        }
    }

    /// What to do about the connection problem: the hint for the status or the last
    /// error's code, then the daemon's own hint when it sent one. `nil` when there is
    /// neither.
    ///
    /// - Parameters:
    ///   - status: The daemon's state.
    ///   - error: The last connection error, or `nil`.
    public static func text(status: ShellStatus, error: DiagnosticsError?) -> String? {
        let parts = [statusHint(status) ?? error.flatMap(codeHint), error?.daemonHint]
            .compactMap { $0 }
            .filter { $0.isEmpty == false }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }

    /// The hint a status gives regardless of the error code.
    private static func statusHint(_ status: ShellStatus) -> String? {
        switch status {
        case .refreshing(let previous):
            statusHint(previous)
        case .incompatible:
            "Install matching versions of Stackly and stacklyd."
        case .untrusted(let cause, _):
            untrustedHint(cause)
        case .recovery:
            "See the recovery report in stacklyd’s log folder."
        case .misconfigured(let problem, _):
            problem.message
        case .connected, .starting, .stopping, .connecting, .unreachable:
            nil
        }
    }

    /// The hint for an error code, or `nil` for a code without one.
    private static func codeHint(_ error: DiagnosticsError) -> String? {
        switch error.code {
        case "ipc.endpoint_missing":
            // provisional: F15 (the GUI starts the daemon itself)
            "stacklyd isn’t running. Start it with stacklyd, then click Reconnect."
        case "ipc.connection_refused":
            "A stale socket is left from a stopped stacklyd. Start stacklyd, then click Reconnect."
        case "ipc.io_error":
            "Stackly couldn’t reach the socket (\(error.operation ?? "unknown"): \(error.kind ?? "unknown"))."
        case "ipc.endpoint_untrusted":
            untrustedHint(.endpoint(reason: error.reason ?? "unknown"))
        case "ipc.peer_rejected":
            untrustedHint(.peerRejected)
        case "protocol.major_mismatch":
            "Install matching versions of Stackly and stacklyd."
        case "ipc.handshake_timeout":
            "stacklyd didn’t answer within 5 seconds."
        case "ipc.call_timeout":
            "stacklyd stopped answering."
        case "ipc.connection_closed":
            "stacklyd closed the connection."
        case "daemon.shutting_down":
            "stacklyd is shutting down."
        case "ipc.protocol_violation":
            "stacklyd sent a message Stackly couldn’t read. Check that both come from the same build."
        default:
            nil
        }
    }

    private static func untrustedHint(_ cause: ShellStatus.UntrustedCause) -> String {
        let reason = untrustedReason(cause)
        return reason.prefix(1).uppercased() + reason.dropFirst()
            + ". Stackly sent nothing to this socket."
    }
}
