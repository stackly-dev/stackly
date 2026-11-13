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

    /// The limit for `daemon.shutdown { restart }`, the interactive one of the secret calls
    /// (190 seconds): the daemon waits up to 30 seconds for the operations in flight and then
    /// up to 40 more for the plan to be written before it answers, about 70 seconds in all
    /// (spec 0030, *API surface*), so a shorter limit would report a slow quiesce as a lost
    /// answer while the daemon goes on to stop the instances.
    public static let restartTimeout: Duration = SecretsClient.interactiveTimeout

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
            mode: .restart, expectedPlanDigest: expectedPlanDigest, force: force,
            overrideLeases: nil)
        let reply = try await send(
            .daemonShutdown(params), idempotencyKey, CallOptions(timeout: Self.restartTimeout))
        guard case .daemonShutdown(let accepted) = reply else { throw Self.unexpectedReply() }
        return accepted
    }

    /// `instance.preview_stop_all`: the instances a Turn Off of the background agent would stop,
    /// the ones already stopping, and the ones the daemon must leave alone (spec 0031).
    ///
    /// - Parameter overrideLeases: Put the instances an environment lease holds among the ones
    ///   to stop instead of the ones left alone (spec 0047, AC-16, AC-24).
    /// - Throws: ``ClientError``, such as `daemon.shutting_down`.
    public func previewTurnOff(overrideLeases: Bool = false) async throws(ClientError)
        -> StopAllPreview
    {
        let reply = try await send(
            .instancePreviewStopAll(
                StopAllPreviewParams(overrideLeases: overrideLeases ? true : nil)),
            nil, .standard)
        guard case .instancePreviewStopAll(let preview) = reply else {
            throw Self.unexpectedReply()
        }
        return preview
    }

    /// `daemon.shutdown { mode: agent_off }`: asks the daemon to stop its instances and to exit
    /// when none is left. The answer comes after the stops, so it uses the long time limit; a
    /// retry after a lost answer reuses the key and gets the same answer.
    ///
    /// - Parameters:
    ///   - idempotencyKey: The key of the action.
    ///   - overrideLeases: Stop the instances an environment lease holds too, so the daemon can
    ///     exit; their leases stay active (spec 0047, AC-17).
    /// - Throws: ``ClientError``, such as `daemon.restart_pending`, `daemon.shutting_down`, or
    ///   `registry.recovery_mode`.
    public func agentOff(idempotencyKey: String, overrideLeases: Bool = false)
        async throws(ClientError) -> ShutdownAccepted
    {
        let params = ShutdownParams(
            mode: .agentOff, expectedPlanDigest: nil, force: false,
            overrideLeases: overrideLeases ? true : nil)
        let reply = try await send(
            .daemonShutdown(params), idempotencyKey, CallOptions(timeout: Self.restartTimeout))
        guard case .daemonShutdown(let accepted) = reply else { throw Self.unexpectedReply() }
        return accepted
    }

    /// `instance.preview_stop_all`: the instances Stop All would stop, the ones already
    /// stopping, and the ones the daemon leaves alone. The menu bar shows it before it asks
    /// to stop (spec 0032, AC-6); it is the same read as ``previewTurnOff()``.
    ///
    /// - Parameter overrideLeases: Put the instances an environment lease holds among the ones
    ///   to stop (spec 0047, AC-24).
    /// - Throws: ``ClientError``.
    public func previewStopAll(overrideLeases: Bool = false) async throws(ClientError)
        -> StopAllPreview
    {
        try await previewTurnOff(overrideLeases: overrideLeases)
    }

    /// `instance.stop_all { expectedInstanceIds }`: stops exactly the instances of the preview
    /// the person confirmed. The daemon refuses with `instance.stop_all_changed`, before it
    /// stops anything, when the instances it would stop are not exactly `expectedInstanceIds`
    /// (spec 0032, AC-7 and AC-8). The answer comes after the operation ended, so it uses the
    /// long limit; a retry after a lost answer reuses the key and gets the same answer.
    ///
    /// - Parameters:
    ///   - expectedInstanceIds: The IDs of ``StopAllPreview/items``.
    ///   - idempotencyKey: The key of the action.
    ///   - overrideLeases: Stop the instances an environment lease holds too; their leases
    ///     stay active (spec 0047, AC-16). It must match the preview that was shown.
    /// - Throws: ``ClientError``, such as `instance.stop_all_changed`.
    public func stopAll(
        expectedInstanceIds: [UUID], idempotencyKey: String, overrideLeases: Bool = false
    ) async throws(ClientError) -> StopAllResult {
        let params = StopAllParams(
            expectedInstanceIds: expectedInstanceIds, overrideLeases: overrideLeases ? true : nil)
        let reply = try await send(
            .instanceStopAll(params), idempotencyKey, CallOptions(timeout: Self.restartTimeout))
        guard case .instanceStopAll(let result) = reply else { throw Self.unexpectedReply() }
        return result
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
