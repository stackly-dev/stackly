import Foundation

/// The audit method of spec 0035 (*Method IPC*) as a typed call: `audit.list`.
///
/// It wraps one `send` closure, the same shape ``InstanceDetailClient`` uses, so a test answers
/// with a fixed reply and the app passes ``DaemonLink`` through ``DaemonLink/audit``. The method
/// only reads: it writes nothing, starts nothing, and a row never carries a secret value,
/// since the registry never stores one (spec 0011).
public struct AuditClient: Sendable {
    /// Sends one call and returns the reply.
    public typealias Send = @Sendable (Call, CallOptions) async throws(ClientError) -> Reply

    /// The most rows one page holds; the daemon refuses a larger `limit` (spec 0035, AC-5).
    public static let maxLimit: UInt32 = 200

    private let send: Send

    /// Creates a client over `send`.
    ///
    /// - Parameter send: Sends one call with its time limit and returns the reply.
    public init(send: @escaping Send) {
        self.send = send
    }

    /// `audit.list`: one page of the local audit, newest first by `(occurredAt, id)`.
    ///
    /// The filters combine: both together keep the rows of that operation on that instance.
    /// An instance that was removed is still found by its UUID.
    ///
    /// - Parameters:
    ///   - instanceId: Only the rows of this instance, or `nil` for every row.
    ///   - operationId: Only the rows of this operation, or `nil` for every row.
    ///   - limit: The most rows to return, 1 to ``maxLimit``; `nil` lets the daemon use 50.
    ///   - before: The `next` of the page before, or `nil` for the first page. The row it
    ///     names is not repeated.
    /// - Returns: The page; its `next` is `nil` on the last page.
    /// - Throws: ``ClientError``, such as `request.invalid_params` for a `limit` out of range
    ///   or a cursor the daemon did not write, or `request.unknown_method` from a daemon
    ///   older than protocol 3.3.
    public func auditList(
        instanceId: UUID? = nil, operationId: UUID? = nil, limit: UInt32? = nil,
        before: String? = nil
    ) async throws(ClientError) -> AuditPage {
        let params = ListAuditParams(
            instanceId: instanceId, operationId: operationId, limit: limit, before: before)
        let reply = try await send(.auditList(params), .standard)
        guard case .auditList(let page) = reply else { throw Self.unexpectedReply() }
        return page
    }

    /// The error for a reply of the wrong kind: the call only reads, but the outcome is not
    /// the one the contract promises.
    private static func unexpectedReply() -> ClientError {
        .rejected(ClientErrorCatalog.protocolViolation())
    }
}

extension DaemonLink {
    /// The audit method over this link, for the app.
    public nonisolated var audit: AuditClient {
        AuditClient { [self] call, options throws(ClientError) in
            try await self.call(call, options: options)
        }
    }
}
