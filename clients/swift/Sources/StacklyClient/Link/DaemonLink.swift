import Foundation
import Synchronization
import os

/// Keeps one connection to `stacklyd` open for as long as the app runs: it
/// connects, retries with the waits of ``ReconnectPolicy``, sends the
/// heartbeat, and publishes one ``ConnectionState`` (spec 0013, *Status
/// koneksi dan reconnect*).
///
/// The status changes only through a failed open or handshake, a transport
/// failure, a heartbeat time out, or `daemon.shutting_down`. A daemon error
/// for an ordinary call never changes it. The link never starts `stacklyd`
/// or any other program.
public actor DaemonLink {
    /// Opens one connection; tests may replace it.
    public typealias Opener =
        @Sendable (SocketEndpoint, ConnectPolicy, ClientInfo, CallOptions) async throws(ClientError)
        -> (DaemonConnection, HandshakeAccepted)

    /// What the last failed attempt was, for its wait and its `since`.
    private enum Problem: Equatable {
        case unreachable
        case incompatible
        case untrusted
    }

    /// Where to connect and as whom, or why the link never tries.
    private enum Target {
        case ready(SocketEndpoint, ClientInfo)
        case misconfigured(MisconfigurationReason)
    }

    private let target: Target
    private let policy: ConnectPolicy
    private let reconnect: ReconnectPolicy
    private let heartbeat: Heartbeat
    private let clock: any Clock<Duration>
    private let now: @Sendable () -> Date
    private let opener: Opener
    private let broadcast = StateBroadcast<ConnectionState>()
    private let active = Mutex<DaemonConnection?>(nil)
    private let logger = Logger(subsystem: "com.ojiepermana.stackly", category: "client")

    private var connected: (connection: DaemonConnection, daemon: ConnectedDaemon)?
    private var loop: Task<Void, Never>?
    private var started = false
    private var stopped = false
    private var attempt = 0
    /// Unreachable failures since the last completed handshake.
    private var failureIndex = 0
    private var problem: (kind: Problem, since: Date)?
    private var retryWaiter: CheckedContinuation<Void, Never>?
    private var retryTimer: Task<Void, Never>?
    /// The reason the link itself closed the current connection.
    private var closeReason: ErrorBody?

    /// The last connection error, kept after the link reconnects.
    public private(set) var lastError: ErrorBody?

    /// Every status change, starting with the current status, for each
    /// listener.
    public nonisolated var states: AsyncStream<ConnectionState> {
        broadcast.stream()
    }

    /// The current status, or `nil` before ``start()``.
    public nonisolated var currentState: ConnectionState? {
        broadcast.latest
    }

    /// Creates a link that is not started yet.
    ///
    /// - Parameters:
    ///   - endpoint: The socket path, or why there is none.
    ///   - clientVersion: The app's version, sent in the handshake. It must be
    ///     1 to 64 printable ASCII characters without spaces.
    ///   - pid: This process's ID.
    ///   - policy: Whose socket and server to accept.
    ///   - reconnect: The waits between attempts.
    ///   - heartbeat: The heartbeat's interval and limit.
    ///   - clock: The clock every wait runs on.
    ///   - now: The wall clock for `since` and `nextAttempt`.
    ///   - opener: Opens one connection.
    public init(
        endpoint: Result<SocketEndpoint, RootOverrideProblem>,
        clientVersion: String,
        pid: UInt32,
        policy: ConnectPolicy = .sameUser,
        reconnect: ReconnectPolicy = .standard,
        heartbeat: Heartbeat = .standard,
        clock: any Clock<Duration> = ContinuousClock(),
        now: @escaping @Sendable () -> Date = { Date() },
        opener: @escaping Opener = { endpoint, policy, client, options throws(ClientError) in
            try await DaemonConnection.open(
                endpoint: endpoint, policy: policy, client: client, options: options)
        }
    ) {
        switch endpoint {
        case .failure(let problem):
            target = .misconfigured(.rootOverride(problem))
        case .success(let endpoint) where Self.isValidClientVersion(clientVersion):
            target = .ready(endpoint, ClientInfo(kind: .gui, version: clientVersion, pid: pid))
        case .success:
            target = .misconfigured(.invalidClientVersion)
        }
        self.policy = policy
        self.reconnect = reconnect
        self.heartbeat = heartbeat
        self.clock = clock
        self.now = now
        self.opener = opener
    }

    /// Whether `version` can be sent in the handshake: 1 to 64 printable ASCII
    /// characters without spaces.
    public static func isValidClientVersion(_ version: String) -> Bool {
        (1...64).contains(version.utf8.count)
            && version.utf8.allSatisfy { (0x21...0x7e).contains($0) }
    }

    /// Starts connecting, or publishes `misconfigured` and never tries.
    public func start() {
        guard started == false else { return }
        started = true
        switch target {
        case .misconfigured(let reason):
            broadcast.publish(.misconfigured(reason))
        case .ready:
            loop = Task { await self.run() }
        }
    }

    /// Starts an attempt now when the link waits to retry, without changing
    /// the order of waits. Does nothing while connecting, connected, or
    /// misconfigured.
    public func retryNow() {
        wakeRetry()
    }

    /// Stops the link: no more attempts, and the connection closes.
    public func stop() async {
        stopped = true
        wakeRetry()
        if let connection = connected?.connection {
            await connection.close()
        }
        await loop?.value
        broadcast.finish()
    }

    /// Closes the socket synchronously from any thread, for app termination.
    /// Nothing is sent to the daemon.
    public nonisolated func stopNow() {
        active.withLock { $0 }?.closeNow()
    }

    /// Sends `call` on the open connection.
    ///
    /// - Throws: ``ClientError/notSent(_:)`` with the last connection error,
    ///   or `ipc.not_connected`, while not connected, without touching a
    ///   socket; otherwise the errors of ``DaemonConnection/call(_:idempotencyKey:options:)``.
    public func call(
        _ call: Call,
        idempotencyKey: String? = nil,
        options: CallOptions = .standard
    ) async throws(ClientError) -> Reply {
        guard let connection = connected?.connection else {
            throw .notSent(lastError ?? ClientErrorCatalog.notConnected())
        }
        return try await connection.call(call, idempotencyKey: idempotencyKey, options: options)
    }

    /// Subscribes on the open connection and returns the reply, its stream,
    /// and the handshake of that connection, whose `serverInstanceId` the
    /// cursor belongs to.
    public func subscribe(
        _ params: SubscribeParams,
        options: CallOptions = .standard
    ) async throws(ClientError) -> (Subscribed, DaemonConnection.EventStream, HandshakeAccepted) {
        guard let (connection, daemon) = connected else {
            throw .notSent(lastError ?? ClientErrorCatalog.notConnected())
        }
        let (subscribed, stream) = try await connection.subscribe(params, options: options)
        return (subscribed, stream, daemon.accepted)
    }

    /// The daemon announced `daemon.shutting_down`: the link closes the
    /// connection at once and reports `unreachable`, without waiting for EOF.
    public func daemonShuttingDown() async {
        guard let connection = connected?.connection else { return }
        closeReason = ClientErrorCatalog.daemonShuttingDown()
        await connection.close()
    }

    /// Asks the connected daemon for its info and health again (Diagnostics
    /// Refresh).
    public func refreshDaemon() async {
        guard let connection = connected?.connection else { return }
        let options = CallOptions(timeout: heartbeat.timeout, clock: clock)
        let info = try? await connection.call(.daemonGetInfo, options: options)
        let health = try? await connection.call(.daemonGetHealth, options: options)
        guard var daemon = connected?.daemon, connected?.connection === connection else { return }
        if case .daemonGetInfo(let value) = info { daemon.info = value }
        if case .daemonGetHealth(let value) = health { daemon.health = value }
        publishConnected(daemon)
    }

    // MARK: - Attempts

    private func run() async {
        while stopped == false {
            attempt += 1
            broadcast.publish(.connecting(attempt: attempt))
            let delay = await attemptOnce()
            guard stopped == false else { break }
            await waitForRetry(delay)
        }
    }

    /// Runs one attempt and, when it connects, the whole connection. Returns
    /// the wait before the next attempt.
    private func attemptOnce() async -> Duration {
        guard case .ready(let endpoint, let client) = target else { return reconnect.blockedDelay }
        let options = CallOptions(timeout: ContractLimits.handshakeTimeout, clock: clock)
        let connection: DaemonConnection
        let accepted: HandshakeAccepted
        do throws(ClientError) {
            (connection, accepted) = try await opener(endpoint, policy, client, options)
        } catch {
            return fail(error)
        }
        guard stopped == false else {
            await connection.close()
            return .zero
        }
        failureIndex = 0
        problem = nil
        closeReason = nil
        active.withLock { $0 = connection }
        let reason = await stayConnected(connection, accepted: accepted)
        active.withLock { $0 = nil }
        connected = nil
        lastError = reason
        logger.info("lost stacklyd: \(reason.code, privacy: .public)")
        return failUnreachable(reason)
    }

    /// Publishes the status for a failed attempt and returns its wait.
    private func fail(_ error: ClientError) -> Duration {
        let body = error.body
        lastError = body
        logger.info("cannot connect to stacklyd: \(body.code, privacy: .public)")
        if body.code == "protocol.major_mismatch" {
            let server = ProtocolVersion(
                major: UInt32(body.detail("serverMajor") ?? "") ?? 0,
                minor: UInt32(body.detail("serverMinor") ?? "") ?? 0)
            return failBlocked(.incompatible) {
                .incompatible(
                    .majorMismatch(server: server, error: body), since: $0, nextAttempt: $1)
            }
        }
        if case .rejected = error, body.code.hasPrefix("request.") {
            return failBlocked(.incompatible) {
                .incompatible(.handshakeRejected(body), since: $0, nextAttempt: $1)
            }
        }
        if body.code == "ipc.endpoint_untrusted" || body.code == "ipc.peer_rejected" {
            return failBlocked(.untrusted) { .untrusted(body, since: $0, nextAttempt: $1) }
        }
        return failUnreachable(body)
    }

    private func failUnreachable(_ body: ErrorBody) -> Duration {
        let delay = reconnect.unreachableDelay(at: failureIndex)
        failureIndex += 1
        let since = sinceFor(.unreachable)
        broadcast.publish(
            .unreachable(body, since: since, nextAttempt: now().addingTimeInterval(delay.seconds)))
        return delay
    }

    private func failBlocked(
        _ kind: Problem,
        _ state: (Date, Date) -> ConnectionState
    ) -> Duration {
        let delay = reconnect.blockedDelay
        broadcast.publish(state(sinceFor(kind), now().addingTimeInterval(delay.seconds)))
        return delay
    }

    /// When the problem of `kind` started: kept while attempts keep failing
    /// the same way.
    private func sinceFor(_ kind: Problem) -> Date {
        if let problem, problem.kind == kind { return problem.since }
        let since = now()
        problem = (kind, since)
        return since
    }

    private func waitForRetry(_ delay: Duration) async {
        guard stopped == false else { return }
        await withCheckedContinuation { continuation in
            retryWaiter = continuation
            retryTimer = Task {
                do {
                    try await clock.sleep(for: delay)
                } catch {
                    return
                }
                self.wakeRetry()
            }
        }
    }

    private func wakeRetry() {
        retryTimer?.cancel()
        retryTimer = nil
        retryWaiter?.resume()
        retryWaiter = nil
    }

    // MARK: - Connected

    private func stayConnected(
        _ connection: DaemonConnection,
        accepted: HandshakeAccepted
    ) async -> ErrorBody {
        let options = CallOptions(timeout: heartbeat.timeout, clock: clock)
        var daemon = ConnectedDaemon(accepted: accepted, info: nil, health: nil, connectedAt: now())
        if case .daemonGetInfo(let info) = try? await connection.call(
            .daemonGetInfo, options: options)
        {
            daemon.info = info
        }
        if case .daemonGetHealth(let health) = try? await connection.call(
            .daemonGetHealth, options: options)
        {
            daemon.health = health
        }
        connected = (connection, daemon)
        publishConnected(daemon)
        let reason = await withTaskGroup(of: ErrorBody?.self) { group in
            group.addTask {
                for await reason in connection.termination { return reason }
                return ClientErrorCatalog.connectionClosed()
            }
            group.addTask {
                await self.beat(connection)
                return nil
            }
            var reason = ClientErrorCatalog.connectionClosed()
            for await value in group {
                if let value {
                    reason = value
                    break
                }
            }
            group.cancelAll()
            return reason
        }
        return closeReason ?? reason
    }

    private func publishConnected(_ daemon: ConnectedDaemon) {
        guard let connection = connected?.connection else { return }
        connected = (connection, daemon)
        broadcast.publish(.connected(daemon))
    }

    /// Sends `daemon.get_health` every interval until the connection ends; a
    /// time out closes it with `ipc.call_timeout`.
    private func beat(_ connection: DaemonConnection) async {
        let options = CallOptions(timeout: heartbeat.timeout, clock: clock)
        while Task.isCancelled == false {
            let status = connected?.daemon.health?.status
            do {
                try await clock.sleep(for: heartbeat.interval(for: status))
            } catch {
                return
            }
            do {
                let reply = try await connection.call(.daemonGetHealth, options: options)
                if case .daemonGetHealth(let health) = reply, var daemon = connected?.daemon,
                    connected?.connection === connection
                {
                    daemon.health = health
                    publishConnected(daemon)
                }
            } catch {
                switch error {
                case .rejected:
                    continue
                case .notSent(let body), .outcomeUnknown(let body):
                    if body.code == "ipc.call_timeout" {
                        closeReason = body
                        await connection.close()
                    }
                    return
                }
            }
        }
    }
}

extension ClientErrorCatalog {
    /// `daemon.shutting_down` as the link reports it after the event.
    static func daemonShuttingDown() -> ErrorBody {
        ErrorBody(
            code: "daemon.shutting_down", category: .unavailable,
            message: "the daemon is shutting down", retryable: true, resource: nil, fields: nil,
            hint: nil, details: nil, operationId: nil, rule: nil)
    }
}

extension Duration {
    /// The duration in seconds, as a `TimeInterval`.
    var seconds: TimeInterval {
        let (seconds, attoseconds) = components
        return TimeInterval(seconds) + TimeInterval(attoseconds) / 1e18
    }
}
