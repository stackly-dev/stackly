import Foundation

/// The detail methods of an instance (spec 0021, *Method IPC*) as typed calls:
/// `instance.get_detail` and `instance.get_data_usage`.
///
/// It wraps one `send` closure, the same shape ``SecretsClient`` uses, so a test answers
/// with a fixed reply and the app passes ``DaemonLink`` through ``DaemonLink/instanceDetails``.
/// Both methods only read: they write nothing, start nothing, and never carry a secret value.
public struct InstanceDetailClient: Sendable {
    /// Sends one call and returns the reply.
    public typealias Send = @Sendable (Call, CallOptions) async throws(ClientError) -> Reply

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its time limit and returns the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `instance.get_detail`: the facts of one instance the summary lacks.
    ///
    /// - Parameter instanceId: The instance.
    /// - Returns: The detail, built from the instance at the revision it carries.
    /// - Throws: ``ClientError``, such as `resource.not_found`.
    public func getDetail(instanceId: UUID) async throws(ClientError) -> InstanceDetail {
        let reply = try await send(
            .instanceGetDetail(GetInstanceDetailParams(instanceId: instanceId)), .standard)
        guard case .instanceGetDetail(let detail) = reply, detail.instanceId == instanceId else {
            throw Self.unexpectedReply()
        }
        return detail
    }

    /// `instance.get_data_usage`: the size of the data folder of one instance.
    ///
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - refresh: Skips the daemon's cache and measures again.
    ///   - cachedOnly: Never starts a measurement; the result is `nil` when the daemon holds
    ///     none. The daemon refuses `refresh` and `cachedOnly` together.
    /// - Returns: The measured size, or `nil` when `cachedOnly` found no fresh result.
    /// - Throws: ``ClientError``, such as `resource.not_found` or
    ///   `instance.data_usage_unavailable`.
    public func getDataUsage(
        instanceId: UUID, refresh: Bool = false, cachedOnly: Bool = false
    ) async throws(ClientError) -> DataUsage? {
        let params = GetDataUsageParams(
            instanceId: instanceId, refresh: refresh, cachedOnly: cachedOnly)
        let reply = try await send(.instanceGetDataUsage(params), .standard)
        guard case .instanceGetDataUsage(let usage) = reply else { throw Self.unexpectedReply() }
        return usage.usage
    }

    /// The error for a reply of the wrong kind: the call only reads, but the outcome is not
    /// the one the contract promises.
    private static func unexpectedReply() -> ClientError {
        .rejected(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The detail methods over this link, for the app.
    public nonisolated var instanceDetails: InstanceDetailClient {
        InstanceDetailClient { [self] call, options throws(ClientError) in
            try await self.call(call, options: options)
        }
    }
}
