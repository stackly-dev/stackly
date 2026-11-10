import Foundation
import StacklyClient

/// Which of Cancel, Retry, Resume, and Abandon an operation offers on the Operations page
/// (spec 0035, AC-8 and AC-10). Pure functions of the summaries and the instance, so the rows,
/// the inspector, and the tests agree.
nonisolated public enum OperationRules {
    /// What the Cancel button of an operation is.
    public enum CancelButton: Equatable, Sendable {
        /// No Cancel: the operation ended, a cancel was already asked for, or it needs
        /// recovery.
        case hidden
        /// Cancel works.
        case enabled
        /// Cancel is off: the current step can’t be undone safely.
        case disabled
    }

    /// Why Cancel is off from the step that can’t be undone on.
    public static let notCancellableReason = "The current step can’t be undone safely."

    /// The Cancel button of `operation`: on for `available`, off for
    /// `past_point_of_no_return`, and none for every other availability (spec 0035, *Key
    /// invariants*).
    /// - Parameter operation: The summary.
    public static func cancel(for operation: OperationSummary) -> CancelButton {
        switch OperationCancel.availability(of: operation) {
        case .available: .enabled
        case .pastPointOfNoReturn: .disabled
        case .requested, .continuing, .none, .unrecognized: .hidden
        }
    }

    /// Whether Resume shows: the operation needs recovery and its actions name `resume`.
    /// - Parameter operation: The summary.
    public static func canResume(_ operation: OperationSummary) -> Bool {
        operation.state == .needsRecovery
            && operation.recovery?.actions?.contains(.resume) == true
    }

    /// Whether Abandon shows: the operation needs recovery and its actions name `abandon`.
    /// - Parameter operation: The summary.
    public static func canAbandon(_ operation: OperationSummary) -> Bool {
        operation.state == .needsRecovery
            && operation.recovery?.actions?.contains(.abandon) == true
    }

    /// The instance verb Retry sends for an operation of `kind`, or `nil` for a kind that has
    /// no Retry on this page.
    /// - Parameter kind: The operation kind.
    public static func retryVerb(of kind: OperationKind) -> InstanceVerb? {
        switch kind {
        case .startInstance: .start
        case .stopInstance: .stop
        case .restartInstance: .restart
        case .reloadInstance: .reload
        default: nil
        }
    }

    /// The verb Retry sends for `operation`, or `nil` when Retry does not show (AC-10): the
    /// operation failed, it is a start, stop, restart, or reload, its instance is still in the
    /// instance store, it is the newest of those four kinds for that instance among `loaded`,
    /// and the instance’s actions allow the verb now.
    /// - Parameters:
    ///   - operation: The summary.
    ///   - loaded: The operations the page holds.
    ///   - instance: The instance the operation works on, from the instance store, or `nil`.
    ///   - local: What this window knows about its own click for that instance.
    public static func retry(
        for operation: OperationSummary, among loaded: [OperationSummary],
        instance: InstanceSummary?, local: InstanceLocalStatus
    ) -> InstanceVerb? {
        guard operation.state == .failed, let verb = retryVerb(of: operation.kind),
            let instance, instanceID(of: operation) == instance.id
        else { return nil }
        let newest =
            loaded
            .filter { retryVerb(of: $0.kind) != nil && instanceID(of: $0) == instance.id }
            .reduce(operation) { newest, other in
                OperationHistory.isNewer(other, than: newest) ? other : newest
            }
        guard newest.id == operation.id, allows(verb, summary: instance, local: local) else {
            return nil
        }
        return verb
    }

    /// Whether the actions of the instance allow `verb` now, as its toolbar buttons do
    /// (spec 0022 for start and stop, spec 0026 for restart and reload).
    static func allows(
        _ verb: InstanceVerb, summary: InstanceSummary, local: InstanceLocalStatus
    ) -> Bool {
        switch verb {
        case .start, .stop:
            return InstanceAction.resolve(summary: summary, local: local).verb == verb
        case .restart, .reload:
            let actions = InstanceLifecycleActions.resolve(
                summary: summary, local: local, providerName: "")
            let item = verb == .restart ? actions.restart : actions.reload
            return item.disabledReason == nil && item.isBusy == false
        case .forceStop:
            return false
        }
    }

    /// The instance an operation works on, when its resource is an instance.
    /// - Parameter operation: The summary.
    public static func instanceID(of operation: OperationSummary) -> UUID? {
        guard let resource = operation.resource, resource.kind == .instance else { return nil }
        return UUID(uuidString: resource.id)
    }
}
