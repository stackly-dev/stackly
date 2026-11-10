import Foundation
import Observation
import StacklyClient

/// The detail of the operation the inspector shows (spec 0035, AC-12): `operation.get` read
/// through the history, so the operation also joins the list in its place. A new revision of
/// the same operation reads it again and keeps the last detail on screen meanwhile.
@Observable
public final class OperationDetailModel {
    /// Where the detail stands.
    public enum Phase: Equatable {
        /// The first read of this operation has not answered.
        case loading
        /// The detail.
        case loaded(OperationDetail)
        /// The daemon no longer knows the operation (`resource.not_found`, pruned).
        case missing
        /// The read failed.
        case failed(ErrorBody)
    }

    /// The operation the phase belongs to.
    public private(set) var operation: UUID?
    /// Where the detail of ``operation`` stands.
    public private(set) var phase: Phase = .loading

    @ObservationIgnored private let read:
        @MainActor (UUID) async -> Result<OperationDetail, ClientError>
    @ObservationIgnored private var generation = 0

    /// Creates the model.
    /// - Parameter read: Reads one operation; `resource.not_found` means it is gone.
    public init(read: @escaping @MainActor (UUID) async -> Result<OperationDetail, ClientError>) {
        self.read = read
    }

    /// The model of a page: it reads through the history, which puts the operation in the
    /// list and marks one the daemon no longer knows.
    /// - Parameter history: The page's history.
    public convenience init(history: OperationHistory) {
        self.init(read: { id in await history.read(id) })
    }

    /// The phase of `id`: ``Phase/loading`` while another operation's detail is held.
    /// - Parameter id: The operation the inspector shows.
    public func phase(for id: UUID) -> Phase {
        operation == id ? phase : .loading
    }

    /// Reads `id`, at `revision`; the page calls it whenever the selection or its revision
    /// changes. The detail of the same operation stays while the new one is read.
    /// - Parameters:
    ///   - id: The operation.
    ///   - revision: The revision the page shows, or `nil` for one it does not hold yet.
    public func load(_ id: UUID, revision: UInt64?) async {
        generation += 1
        let generation = generation
        if operation != id {
            operation = id
            phase = .loading
        }
        let result = await read(id)
        guard generation == self.generation, operation == id else { return }
        switch result {
        case .success(let detail):
            phase = .loaded(detail)
        case .failure(.rejected(let body)) where body.code == "resource.not_found":
            phase = .missing
        case .failure(let error):
            if case .loaded = phase { return }
            phase = .failed(error.body)
        }
    }
}
