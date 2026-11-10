import Foundation
import StacklyClient

/// Why an instance failed, as one row of *Pemetaan sebab* (spec 0033): read from the catalog
/// code of the related operation's error, or from the `lastFailure.code` of the instance. The
/// two columns map to the same row, so a banner that first shows the summary and then gets the
/// operation keeps its title.
nonisolated public enum FailureCause: Equatable, Sendable {
    /// The port problem of a start.
    public enum PortProblem: Equatable, Sendable {
        /// Another process holds the port (`endpoint_in_use`).
        case inUse
        /// Another Stackly instance owns the port (`endpoint_conflict`).
        case conflict
        /// macOS refused the port (`endpoint_unavailable`).
        case unavailable
    }

    /// The restart budget ran out.
    case crashLoop
    /// A port could not be used.
    case port(PortProblem)
    /// Another process holds the data lock.
    case dataInUse
    /// Any other code, kept as sent.
    case other(String)

    /// The cause of a catalog code of an operation error, such as
    /// `supervisor.endpoint_in_use`.
    /// - Parameter errorCode: The `code` of the `ErrorBody`.
    public init(errorCode: String) {
        switch errorCode {
        case "supervisor.retry_budget_exhausted": self = .crashLoop
        case "supervisor.endpoint_in_use": self = .port(.inUse)
        case "supervisor.endpoint_conflict": self = .port(.conflict)
        case "supervisor.endpoint_unavailable": self = .port(.unavailable)
        case "supervisor.data_in_use": self = .dataInUse
        default: self = .other(errorCode)
        }
    }

    /// The cause of a failure code of an instance, such as `crash_loop`.
    /// - Parameter failureCode: The `lastFailure.code` of the summary.
    public init(failureCode: String) {
        switch failureCode {
        case "crash_loop": self = .crashLoop
        case "endpoint_in_use": self = .port(.inUse)
        case "data_in_use": self = .dataInUse
        default: self = .other(failureCode)
        }
    }

    /// Whether two causes stand on the same row of the table. Every code the table does not
    /// list shares the last row, whatever its text.
    func isSameRow(as other: FailureCause) -> Bool {
        switch (self, other) {
        case (.other, .other): true
        default: self == other
        }
    }
}

/// The operation of an instance a failure banner and `stackly instance inspect` read (spec 0033,
/// *Operasi terkait*): the newest start, restart, stop, or port change, in the order of
/// `operation.list`. Rename, apply config, reload, and create are passed over.
nonisolated public enum RelatedOperation {
    /// The kinds that count.
    static let kinds: Set<OperationKind> = [
        .startInstance, .restartInstance, .stopInstance, .changeEndpoints,
    ]

    /// What the banner takes from the operations of one instance.
    public struct Reading: Equatable, Sendable {
        /// The failed start or restart whose error the banner uses, or `nil`.
        public let operation: OperationSummary?
        /// Whether a port change succeeded after the failure (AC-10).
        public let portsChanged: Bool

        /// The error of ``operation``.
        public var error: ErrorBody? { operation?.failure?.error }
    }

    /// The operations newest first: by creation time, then by ID, like `operation.list`.
    /// - Parameter operations: The operations, in any order.
    public static func newestFirst(_ operations: [OperationSummary]) -> [OperationSummary] {
        operations.sorted { a, b in
            if a.createdAt != b.createdAt { return a.createdAt > b.createdAt }
            return a.id.uuidString > b.id.uuidString
        }
    }

    /// The newest operation of the four related kinds, or `nil`.
    /// - Parameter operations: The operations of the instance, in any order.
    public static func latest(in operations: [OperationSummary]) -> OperationSummary? {
        newestFirst(operations).first { kinds.contains($0.kind) }
    }

    /// What the banner reads from the operations of `instance`: the error of the latest
    /// related operation when it is a start or restart that failed (and, for a `failed`
    /// instance, the failure the instance recorded), and whether the ports changed since.
    /// - Parameters:
    ///   - instance: The instance.
    ///   - operations: Its operations, in any order; those of other resources are ignored.
    public static func reading(of instance: InstanceSummary, operations: [OperationSummary])
        -> Reading
    {
        let id = instance.id.uuidString.lowercased()
        var related = newestFirst(
            operations.filter {
                $0.resource?.kind == .instance && $0.resource?.id.lowercased() == id
                    && kinds.contains($0.kind)
            })[...]
        var portsChanged = false
        // On a failed instance a port change that succeeded after the failure adds a sentence,
        // and the start before it still gives the error. On a stopped one it is a newer
        // operation, so the banner goes away (AC-11).
        if instance.lifecycle == .failed {
            while let first = related.first, first.kind == .changeEndpoints,
                first.state == .succeeded,
                isNewer(first, than: instance.lastFailure)
            {
                portsChanged = true
                related = related.dropFirst()
            }
        }
        guard let latest = related.first,
            latest.kind == .startInstance || latest.kind == .restartInstance,
            latest.state == .failed, let error = latest.failure?.error
        else { return Reading(operation: nil, portsChanged: portsChanged) }
        switch instance.lifecycle {
        case .failed:
            guard matches(latest, error, instance.lastFailure) else {
                return Reading(operation: nil, portsChanged: portsChanged)
            }
        case .stopped:
            break
        default:
            return Reading(operation: nil, portsChanged: portsChanged)
        }
        return Reading(operation: latest, portsChanged: portsChanged)
    }

    /// Whether an operation ended after the failure, or the failure has no time.
    private static func isNewer(_ operation: OperationSummary, than failure: InstanceLastFailure?)
        -> Bool
    {
        guard let at = failure?.at else { return true }
        return operation.updatedAt > at
    }

    /// Whether the failed operation is the failure the instance recorded: it ended at or after
    /// `lastFailure.at`, or, without a time (after a daemon restart), its code stands on the
    /// same row of *Pemetaan sebab*.
    private static func matches(
        _ operation: OperationSummary, _ error: ErrorBody, _ failure: InstanceLastFailure?
    ) -> Bool {
        guard let failure else { return false }
        if let at = failure.at { return operation.updatedAt >= at }
        return FailureCause(errorCode: error.code).isSameRow(
            as: FailureCause(failureCode: failure.code))
    }
}
