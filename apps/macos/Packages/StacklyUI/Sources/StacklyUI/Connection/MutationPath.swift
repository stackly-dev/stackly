import Foundation
import StacklyClient

/// How a mutation ended, by what the daemon may have done.
public enum MutationOutcome: Equatable, Sendable {
    /// The daemon ran it and replied.
    case completed(Reply)
    /// The daemon refused it; nothing changed.
    case rejected(ErrorBody)
    /// Nothing reached the daemon.
    case notSent(ErrorBody)
    /// The request was written but no answer came; a retry reuses `key`.
    case outcomeUnknown(ErrorBody, key: String)
}

/// The words feature 67 shows after a mutation that did not complete.
public enum MutationFeedback {
    /// After ``MutationOutcome/notSent(_:)``.
    public static let notSent = "stacklyd is unavailable. Nothing was sent."
    /// After ``MutationOutcome/outcomeUnknown(_:key:)``.
    public static let outcomeUnknown =
        "Lost the connection to stacklyd. The action may have run; check Operations before trying again."
}

/// The one way the GUI sends a mutation (spec 0013, *Jalur mutasi*): with an
/// idempotency key per user action, and never through a socket while not
/// connected. The daemon still validates every mutation.
public struct MutationPath {
    private let model: DaemonModel

    /// A path through `model`'s link.
    public init(model: DaemonModel) {
        self.model = model
    }

    /// A new idempotency key: a lowercase UUID.
    public static func newKey() -> String {
        UUID().uuidString.lowercased()
    }

    /// Sends `call` with `idempotencyKey`. Pass the key of an
    /// ``MutationOutcome/outcomeUnknown(_:key:)`` to retry the same action.
    public func perform(
        _ call: Call,
        idempotencyKey: String = MutationPath.newKey()
    ) async -> MutationOutcome {
        guard model.isLinked else {
            return .notSent(model.lastError ?? ClientErrorCatalog.notConnected())
        }
        do {
            return .completed(try await model.send(call, idempotencyKey: idempotencyKey))
        } catch {
            switch error {
            case .rejected(let body): return .rejected(body)
            case .notSent(let body): return .notSent(body)
            case .outcomeUnknown(let body): return .outcomeUnknown(body, key: idempotencyKey)
            }
        }
    }
}
