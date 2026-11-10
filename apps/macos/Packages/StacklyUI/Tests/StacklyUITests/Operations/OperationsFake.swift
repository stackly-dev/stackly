import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// A fake daemon for the Operations page models (spec 0035): it answers `operation.list`
/// pages by cursor, `operation.get` from a table, and records every mutation with its key,
/// answering from a queue.
@MainActor
final class OperationsFake {
    /// The pages by cursor; `nil` is the first page.
    var pages: [String?: OperationPage] = [:]
    /// The error the next `operation.list` throws, once.
    var listError: ClientError?
    /// The operations `operation.get` knows.
    var details: [UUID: OperationDetail] = [:]
    /// The cursors `operation.list` was called with, in order.
    private(set) var listed: [String?] = []
    /// The mutations sent, with their idempotency keys.
    private(set) var sent: [(call: Call, key: String)] = []
    /// The outcomes the next mutations get, in order.
    var outcomes: [MutationOutcome] = []

    /// The service over this fake.
    var service: OperationsService {
        OperationsService(
            list: { [self] before throws(ClientError) in try await list(before).get() },
            get: { [self] id throws(ClientError) in try await get(id).get() },
            send: { [self] call, key in
                sent.append((call, key))
                return outcomes.isEmpty
                    ? .notSent(ClientErrorCatalog.notConnected()) : outcomes.removeFirst()
            })
    }

    private func list(_ before: String?) -> Result<OperationPage, ClientError> {
        listed.append(before)
        if let error = listError {
            listError = nil
            return .failure(error)
        }
        return .success(pages[before] ?? OperationPage(items: [], next: nil))
    }

    private func get(_ id: UUID) -> Result<OperationDetail, ClientError> {
        guard let detail = details[id] else {
            return .failure(.rejected(Self.error("resource.not_found")))
        }
        return .success(detail)
    }

    /// An error body with `code`.
    static func error(_ code: String, _ message: String = "refused") -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: message, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }

    /// A detail with no steps and no created items.
    static func detail(_ operation: OperationSummary) -> OperationDetail {
        OperationDetail(operation: operation, steps: [], created: [])
    }

    /// An operation center that holds `active` once its snapshot loaded.
    static func center(active: [OperationSummary] = []) -> OperationCenter {
        OperationCenter(
            fetchPage: { _ throws(ClientError) in Page(items: active, next: nil) },
            fetchOne: { _ throws(ClientError) in nil },
            sleep: { _ in })
    }
}
