import Foundation
import StacklyClient

/// What the Operations page asks of the daemon (spec 0035, *Halaman Operations*): the history
/// pages of `operation.list`, the detail of `operation.get`, and the mutation path with its
/// idempotency key for Cancel, Resume, and Abandon. The app builds one from its
/// ``DaemonModel``; a preview, a snapshot, or a test builds one that answers from data.
nonisolated public struct OperationsService: Sendable {
    /// One page of `operation.list` without `activeOnly`, newest first, before an opaque
    /// cursor (`nil` for the first page).
    public var list: @Sendable (String?) async throws(ClientError) -> OperationPage
    /// `operation.get`: the summary with its steps and created items.
    public var get: @Sendable (UUID) async throws(ClientError) -> OperationDetail
    /// Sends one mutation with its idempotency key.
    public var send: @MainActor @Sendable (Call, String) async -> MutationOutcome

    /// Creates a service.
    /// - Parameters:
    ///   - list: Reads one page of the operation history.
    ///   - get: Reads one operation with its steps.
    ///   - send: Sends a mutation with its idempotency key.
    public init(
        list: @escaping @Sendable (String?) async throws(ClientError) -> OperationPage,
        get: @escaping @Sendable (UUID) async throws(ClientError) -> OperationDetail,
        send: @escaping @MainActor @Sendable (Call, String) async -> MutationOutcome
    ) {
        self.list = list
        self.get = get
        self.send = send
    }

    /// A service for a window that has no daemon: every call fails as not connected.
    public static var disconnected: OperationsService {
        OperationsService(
            list: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) },
            get: { _ throws(ClientError) in throw .notSent(ClientErrorCatalog.notConnected()) },
            send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) })
    }
}

extension OperationsService {
    /// The page size of the history (spec 0035, AC-11).
    nonisolated static let pageSize: UInt32 = 50

    /// The service of the running app: the reads go through the model's link and the
    /// mutations through its ``MutationPath``.
    /// - Parameter model: The daemon model of the window.
    static func live(_ model: DaemonModel) -> OperationsService {
        let path = MutationPath(model: model)
        return OperationsService(
            list: { before throws(ClientError) in
                let params = ListOperationsParams(
                    activeOnly: false, resource: nil, limit: pageSize, before: before)
                let reply = try await model.read(.operationList(params)).get()
                guard case .operationList(let page) = reply else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return page
            },
            get: { id throws(ClientError) in
                let reply = try await model.read(
                    .operationGet(GetOperationParams(operationId: id))
                )
                .get()
                guard case .operationGet(let detail) = reply, detail.operation.id == id else {
                    throw .rejected(ClientErrorCatalog.protocolViolation())
                }
                return detail
            },
            send: { call, key in await path.perform(call, idempotencyKey: key) })
    }
}
