import Foundation

/// The daemon as the link knows it while connected.
public struct ConnectedDaemon: Equatable, Sendable {
    /// The handshake reply.
    public let accepted: HandshakeAccepted
    /// The reply of `daemon.get_info`, when it answered.
    public var info: DaemonInfo?
    /// The last reply of `daemon.get_health`, from the heartbeat.
    public var health: DaemonHealth?
    /// When the handshake completed.
    public let connectedAt: Date

    /// Creates the facts of one connection.
    public init(
        accepted: HandshakeAccepted,
        info: DaemonInfo?,
        health: DaemonHealth?,
        connectedAt: Date
    ) {
        self.accepted = accepted
        self.info = info
        self.health = health
        self.connectedAt = connectedAt
    }
}

/// Why the daemon and this client cannot talk.
public enum IncompatibleReason: Equatable, Sendable {
    /// `protocol.major_mismatch`, with the daemon's version from `details`.
    case majorMismatch(server: ProtocolVersion, error: ErrorBody)
    /// The daemon refused the handshake with a `request.*` error.
    case handshakeRejected(ErrorBody)

    /// The daemon's error.
    public var error: ErrorBody {
        switch self {
        case .majorMismatch(_, let error), .handshakeRejected(let error): error
        }
    }
}

/// Why the client cannot connect at all until it is relaunched.
public enum MisconfigurationReason: Equatable, Sendable {
    /// `STACKLY_ROOT` or the user's temporary folder gives no socket path.
    case rootOverride(RootOverrideProblem)
    /// The bundle version is not 1 to 64 printable ASCII characters.
    case invalidClientVersion
}

/// The one connection status the link holds (spec 0013, *Status koneksi dan
/// reconnect*).
public enum ConnectionState: Equatable, Sendable {
    /// An attempt runs; `attempt` counts from 1 since the link started.
    case connecting(attempt: Int)
    /// The handshake completed.
    case connected(ConnectedDaemon)
    /// The daemon cannot be reached, since `since`; the next attempt runs at
    /// `nextAttempt`.
    case unreachable(ErrorBody, since: Date, nextAttempt: Date)
    /// The daemon speaks another protocol.
    case incompatible(IncompatibleReason, since: Date, nextAttempt: Date)
    /// The socket or the daemon failed a security check; nothing was sent.
    case untrusted(ErrorBody, since: Date, nextAttempt: Date)
    /// The client's own configuration stops it; it never tries.
    case misconfigured(MisconfigurationReason)
}

/// The waits between attempts (spec 0013).
public struct ReconnectPolicy: Equatable, Sendable {
    /// The waits after the first failures while unreachable, in order.
    public let unreachableDelays: [Duration]
    /// The wait after every later failure while unreachable.
    public let unreachableSteady: Duration
    /// The wait while incompatible or untrusted.
    public let blockedDelay: Duration

    /// 0.5, 1, 2, and 4 seconds, then every 5 seconds; 30 seconds while
    /// incompatible or untrusted.
    public static let standard = ReconnectPolicy(
        unreachableDelays: [.milliseconds(500), .seconds(1), .seconds(2), .seconds(4)],
        unreachableSteady: .seconds(5),
        blockedDelay: .seconds(30)
    )

    /// Creates a policy.
    public init(unreachableDelays: [Duration], unreachableSteady: Duration, blockedDelay: Duration)
    {
        self.unreachableDelays = unreachableDelays
        self.unreachableSteady = unreachableSteady
        self.blockedDelay = blockedDelay
    }

    /// The wait after the unreachable failure at `index`, counted from 0 since
    /// the last completed handshake.
    public func unreachableDelay(at index: Int) -> Duration {
        index < unreachableDelays.count ? unreachableDelays[index] : unreachableSteady
    }
}

/// How often the link asks a connected daemon for its health, and how long it
/// waits for the answer (spec 0013).
public struct Heartbeat: Equatable, Sendable {
    /// The interval while the daemon is not starting.
    public let interval: Duration
    /// The interval while the daemon reports `starting`.
    public let startingInterval: Duration
    /// The limit for one `daemon.get_health`; a time out closes the connection.
    public let timeout: Duration

    /// Every 15 seconds, every second while starting, 10 second limit.
    public static let standard = Heartbeat(
        interval: .seconds(15), startingInterval: .seconds(1), timeout: .seconds(10))

    /// Creates a heartbeat.
    public init(interval: Duration, startingInterval: Duration, timeout: Duration) {
        self.interval = interval
        self.startingInterval = startingInterval
        self.timeout = timeout
    }

    /// The wait before the next heartbeat for a daemon with `status`.
    public func interval(for status: HealthStatus?) -> Duration {
        status == .starting ? startingInterval : interval
    }
}
