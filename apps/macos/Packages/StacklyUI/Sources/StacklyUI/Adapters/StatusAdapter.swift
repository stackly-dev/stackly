import StacklyClient

/// Maps the daemon's status DTOs to the UI status enums of spec 0012 (spec 0013,
/// *Adapter DTO ke enum UI*).
///
/// Every mapping is one exhaustive `switch` over a closed protocol enum, without
/// `default`, so a new protocol case fails to compile until the UI knows how to show it.
/// The source rule test rejects `default:` anywhere in `Adapters/`.
nonisolated public enum StatusAdapter {
    /// The UI status of an operation state.
    public static func operation(_ state: OperationState) -> OperationStatus {
        switch state {
        case .queued: .queued
        case .running: .running
        case .cancelRequested: .cancelRequested
        case .succeeded: .succeeded
        case .failed: .failed
        case .cancelled: .cancelled
        case .needsRecovery: .needsRecovery
        }
    }

    /// Whether an operation finished only in part: its result lists items that did not
    /// all go to plan.
    public static func isPartial(_ operation: OperationSummary) -> Bool {
        operation.result?.partial?.isEmpty == false
    }

    /// How an operation's status looks, from the operation status table of spec 0012.
    public static func appearance(of operation: OperationSummary) -> StatusAppearance {
        .operation(state: self.operation(operation.state), partial: isPartial(operation))
    }
}
