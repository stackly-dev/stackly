import Foundation

/// The read side of editing an instance (spec 0029, *Method IPC*) as typed calls:
/// `instance.config.get`, `instance.config.validate`, and the dry run of
/// `instance.change_endpoints`.
///
/// It wraps one `send` closure, the same shape ``InstanceDetailClient`` uses, so a test answers
/// with a fixed reply and the app passes ``DaemonLink`` through ``DaemonLink/instanceConfig``.
/// None of these calls takes an idempotency key: they write no registry row, no file, and no
/// journal entry, and they never carry a secret value. The mutations (`apply`, `keep_revision`,
/// `rename`, and a real `change_endpoints`) go through the GUI's mutation path.
public struct InstanceConfigClient: Sendable {
    /// Sends one call and returns the reply.
    public typealias Send = @Sendable (Call, CallOptions) async throws(ClientError) -> Reply

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its time limit and returns the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `instance.config.get`: the fields, the active revision, the history, the effective files,
    /// and the drift of one instance.
    ///
    /// - Parameter instanceId: The instance; a retained one is refused.
    /// - Returns: The view, built from the pinned package and the active revision.
    /// - Throws: ``ClientError``, such as `resource.not_found` or
    ///   `instance.provider_unavailable`.
    public func get(instanceId: UUID) async throws(ClientError) -> InstanceConfigView {
        let reply = try await send(
            .instanceConfigGet(GetConfigParams(instanceId: instanceId)), .standard)
        guard case .instanceConfigGet(let view) = reply else { throw Self.unexpectedReply() }
        return view
    }

    /// `instance.config.validate`: judges a change with the rules of apply and writes nothing.
    ///
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - baseRevision: The active revision the client read.
    ///   - change: The proposed settings, a restore of an older revision, or a roll back.
    /// - Returns: The field errors, the diff, the impact, and the preview of the files.
    /// - Throws: ``ClientError``, such as `config.revision_conflict` when `baseRevision` is
    ///   no longer the active one.
    public func validate(
        instanceId: UUID, baseRevision: UInt64, change: ConfigChangeRequest
    ) async throws(ClientError) -> ConfigValidation {
        let params = ValidateConfigParams(
            instanceId: instanceId, baseRevision: baseRevision, change: change)
        let reply = try await send(.instanceConfigValidate(params), .standard)
        guard case .instanceConfigValidate(let validation) = reply else {
            throw Self.unexpectedReply()
        }
        return validation
    }

    /// `instance.change_endpoints` with `dryRun`: judges new ports with the checks of create
    /// and changes nothing.
    ///
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - baseRevision: The active revision the client read.
    ///   - endpoints: The IP endpoints that get another port request.
    /// - Returns: The verdict, with a suggested port for a fixed port that conflicts.
    /// - Throws: ``ClientError``, such as `config.revision_conflict` or
    ///   `config.drift_detected`.
    public func validateEndpoints(
        instanceId: UUID, baseRevision: UInt64, endpoints: [EndpointRequest]
    ) async throws(ClientError) -> EndpointValidation {
        let params = ChangeEndpointsParams(
            instanceId: instanceId, baseRevision: baseRevision, endpoints: endpoints,
            dryRun: true, overrideLeases: nil)
        let reply = try await send(.instanceChangeEndpoints(params), .standard)
        guard case .instanceChangeEndpoints(.dryRun(let validation)) = reply else {
            throw Self.unexpectedReply()
        }
        return validation
    }

    /// The error for a reply of the wrong kind: the contract promised another outcome.
    private static func unexpectedReply() -> ClientError {
        .rejected(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The config read methods over this link, for the app.
    public nonisolated var instanceConfig: InstanceConfigClient {
        InstanceConfigClient { [self] call, options throws(ClientError) in
            try await self.call(call, options: options)
        }
    }
}
