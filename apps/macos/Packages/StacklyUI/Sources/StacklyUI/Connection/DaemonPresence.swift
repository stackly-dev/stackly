import Foundation
import StacklyClient

/// The daemon as the shell presents it (spec 0013, *Model shell di main
/// actor*): a pure function of the link's status, the stores' refresh, and
/// the daemon's health.
public enum DaemonPresence: Equatable, Sendable {
    /// The first attempt after launch; `slow` after one second.
    case connecting(slow: Bool)
    /// Connected and refreshed, with the daemon's health.
    case connected(health: DaemonHealthLevel)
    /// Connected; the stores are loading the latest state.
    case refreshing
    /// Connected while the daemon starts.
    case starting
    /// Connected while the daemon shuts down.
    case stopping
    /// Connected while the registry is in recovery mode.
    case recovery
    /// The daemon cannot be reached; the state shown is from `lastKnownAt`.
    case unreachable(lastKnownAt: Date?)
    /// The daemon speaks another protocol.
    case incompatible(IncompatibleReason)
    /// The socket or the daemon failed a security check.
    case untrusted(reason: String, path: String)
    /// The app's own configuration stops it.
    case misconfigured(MisconfigurationReason)

    /// What the presence is built from.
    struct Inputs: Equatable {
        /// The last status that is not `connecting`, or `nil` before the first
        /// attempt ended.
        var stable: ConnectionState?
        /// Whether the first attempt has run for more than one second.
        var slow = false
        /// Whether every store refreshed since the current connection opened.
        var refreshed = false
        /// When the last frame arrived after a completed snapshot.
        var lastKnownAt: Date?
        /// Whether the daemon reports registry recovery.
        var recovery = false
    }

    /// The presence for `inputs`. An attempt that runs after a failure never
    /// changes it, so the banner and the footer never flicker.
    static func derive(_ inputs: Inputs) -> DaemonPresence {
        switch inputs.stable {
        case nil, .connecting:
            return .connecting(slow: inputs.slow)
        case .connected(let daemon):
            guard inputs.refreshed else { return .refreshing }
            if inputs.recovery { return .recovery }
            switch daemon.health?.status {
            case .starting: return .starting
            case .stopping: return .stopping
            case .ready: return .connected(health: .ready)
            case .degraded: return .connected(health: .degraded)
            case .unknown, nil: return .connected(health: .unknown)
            }
        case .unreachable:
            return .unreachable(lastKnownAt: inputs.lastKnownAt)
        case .incompatible(let reason, _, _):
            return .incompatible(reason)
        case .untrusted(let error, _, _):
            return .untrusted(
                reason: error.code == "ipc.peer_rejected" ? "" : error.detail("reason") ?? "",
                path: error.detail("path") ?? "")
        case .misconfigured(let reason):
            return .misconfigured(reason)
        }
    }

    /// Whether the presence is connected and refreshed, so mutations may run.
    var isConnected: Bool {
        if case .connected = self { return true }
        return false
    }
}

/// Turns presences and link states into the shell's presentation types.
enum ShellStatusMapping {
    /// The app's protocol in `M.m` form.
    static var appProtocol: String {
        text(ContractInfo.protocolVersion)
    }

    /// `version` in `M.m` form.
    static func text(_ version: ProtocolVersion) -> String {
        "\(version.major).\(version.minor)"
    }

    /// The shell status of `presence`.
    ///
    /// - Parameters:
    ///   - presence: The presence.
    ///   - stable: The link state it came from, for `since` and the error.
    ///   - launchedAt: When the app launched, for the first attempt.
    ///   - before: The status before the current connection opened, which
    ///     `refreshing` keeps showing.
    static func status(
        for presence: DaemonPresence,
        stable: ConnectionState?,
        launchedAt: Date,
        before: ShellStatus
    ) -> ShellStatus {
        let connectedAt: Date =
            if case .connected(let daemon) = stable { daemon.connectedAt } else { launchedAt }
        switch presence {
        case .connecting(let slow):
            return .connecting(slow: slow, since: launchedAt)
        case .connected(let health):
            return .connected(health)
        case .refreshing:
            return .refreshing(after: before)
        case .starting:
            return .starting(since: connectedAt)
        case .stopping:
            return .stopping(since: connectedAt)
        case .recovery:
            return .recovery(since: connectedAt)
        case .unreachable(let lastKnownAt):
            return .unreachable(lastKnownAt: lastKnownAt, since: since(of: stable) ?? launchedAt)
        case .incompatible(let reason):
            let since = since(of: stable) ?? launchedAt
            switch reason {
            case .majorMismatch(let server, _):
                return .incompatible(
                    .majorMismatch(daemonProtocol: text(server), appProtocol: appProtocol),
                    since: since)
            case .handshakeRejected(let error):
                return .incompatible(.handshakeRejected(code: error.code), since: since)
            }
        case .untrusted(let reason, _):
            let cause: ShellStatus.UntrustedCause =
                reason.isEmpty ? .peerRejected : .endpoint(reason: reason)
            return .untrusted(cause, since: since(of: stable) ?? launchedAt)
        case .misconfigured(let reason):
            return .misconfigured(misconfiguration(reason), since: launchedAt)
        }
    }

    /// The shell's words for a misconfiguration.
    static func misconfiguration(_ reason: MisconfigurationReason) -> ShellStatus.Misconfiguration {
        switch reason {
        case .invalidClientVersion: .invalidClientVersion
        case .rootOverride(.empty): .rootEmpty
        case .rootOverride(.notAbsolute): .rootNotAbsolute
        case .rootOverride(.doesNotExist): .rootMissing
        case .rootOverride(.unreadable): .rootUnreadable
        case .rootOverride(.runtimeRootTooLong): .rootTooLong
        case .rootOverride(.tempDirUnavailable): .tempDirUnavailable
        }
    }

    /// When the problem of a failure state started.
    static func since(of state: ConnectionState?) -> Date? {
        switch state {
        case .unreachable(_, let since, _), .incompatible(_, let since, _),
            .untrusted(_, let since, _):
            since
        default:
            nil
        }
    }

    /// When the next attempt runs, for a failure state.
    static func nextAttempt(of state: ConnectionState?) -> Date? {
        switch state {
        case .unreachable(_, _, let next), .incompatible(_, _, let next),
            .untrusted(_, _, let next):
            next
        default:
            nil
        }
    }
}
