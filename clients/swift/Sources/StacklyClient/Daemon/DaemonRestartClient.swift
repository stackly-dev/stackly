import Foundation

/// The two calls behind Restart Daemon (spec 0030, *API surface*) as typed methods.
///
/// It wraps one `send` closure, the same shape ``CreateClient`` uses, so a test can answer
/// with a fixed reply and the app builds it over its link. ``preview()`` only reads;
/// ``restart(expectedPlanDigest:force:idempotencyKey:)`` is the one mutation and needs the
/// key of the user's action.
public struct DaemonRestartClient: Sendable {
    /// Sends one call with its idempotency key, when it has one, and returns the reply.
    public typealias Send =
        @Sendable (Call, String?, CallOptions) async throws(ClientError) -> Reply

    /// The limit for `daemon.shutdown { restart }`: the daemon waits up to 30 seconds for the
    /// operations in flight before it answers (spec 0030, *API surface*), so the standard 10
    /// seconds would report a slow quiesce as a lost answer.
    public static let restartTimeout: Duration = .seconds(45)

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its idempotency key and time limit and returns
    ///   the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `daemon.restart_preview`: the plan a restart asked for now would write, and the
    /// operations that would refuse it.
    ///
    /// - Throws: ``ClientError``, such as `daemon.shutting_down`.
    public func preview() async throws(ClientError) -> RestartPreview {
        let reply = try await send(.daemonRestartPreview, nil, .standard)
        guard case .daemonRestartPreview(let preview) = reply else { throw Self.unexpectedReply() }
        return preview
    }

    /// `daemon.shutdown { mode: restart }`: asks the daemon to write the plan, stop what it
    /// cannot hand over, and exit so launchd starts it again. The reply is the only one; the
    /// progress follows in `daemon.restart_plan` events.
    ///
    /// - Parameters:
    ///   - expectedPlanDigest: The ``RestartPreview/digest`` the person confirmed.
    ///   - force: Restart although an operation that cannot resume still runs.
    ///   - idempotencyKey: The key of the action; a retry after a lost answer reuses it.
    /// - Throws: ``ClientError``, such as `daemon.restart_plan_changed`, `daemon.restart_busy`,
    ///   `daemon.restart_pending`, `daemon.restart_unavailable`, or `daemon.shutting_down`.
    public func restart(
        expectedPlanDigest: String, force: Bool = false, idempotencyKey: String
    ) async throws(ClientError) -> ShutdownAccepted {
        let params = ShutdownParams(
            mode: .restart, expectedPlanDigest: expectedPlanDigest, force: force)
        let reply = try await send(
            .daemonShutdown(params), idempotencyKey, CallOptions(timeout: Self.restartTimeout))
        guard case .daemonShutdown(let accepted) = reply else { throw Self.unexpectedReply() }
        return accepted
    }

    /// A reply of another kind than the call asked for is a broken contract, which the
    /// caller treats as an unknown outcome.
    private static func unexpectedReply() -> ClientError {
        .rejected(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The restart calls over this link, for the app.
    public nonisolated var restart: DaemonRestartClient {
        DaemonRestartClient { [self] call, key, options throws(ClientError) in
            try await self.call(call, idempotencyKey: key, options: options)
        }
    }
}
