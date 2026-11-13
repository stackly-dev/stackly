import StacklyClient

/// Whether a cancel can still stop an operation safely, as the GUI reads it (spec 0035,
/// *Ketersediaan cancel*). The daemon works it out from the state, the point of no return,
/// and the kind's interrupt policy, and sends it in every summary; the GUI keeps no table of
/// kinds of its own.
nonisolated public enum OperationCancel {
    /// The availability of `operation`: the one the daemon sent, or for an older daemon the
    /// fallback rule (`queued` or `running` is `available`, `cancel_requested` is `requested`,
    /// anything else `none`). The fallback never reads `pointOfNoReturnAt`, since the client
    /// does not know the kind's policy: a cancel it sends is always safe, and the daemon
    /// answers with the disposition the page shows. A value from a newer daemon is `none`.
    /// - Parameter operation: The summary.
    public static func availability(of operation: OperationSummary) -> CancelAvailability {
        switch operation.cancel {
        case .some(.unrecognized):
            return .none
        case .some(let sent):
            return sent
        case nil:
            switch operation.state {
            case .queued, .running: return .available
            case .cancelRequested: return .requested
            case .succeeded, .failed, .cancelled, .needsRecovery: return .none
            }
        }
    }

    /// The badge of `operation` (spec 0012, with spec 0035): "Finishing…" for a cancel that
    /// came after a step that can’t be undone, else the operation status table.
    /// - Parameter operation: The summary.
    public static func appearance(of operation: OperationSummary) -> StatusAppearance {
        if StatusAdapter.upIncomplete(operation) != nil { return StatusAdapter.upPartial }
        return .operation(
            state: StatusAdapter.operation(operation.state),
            partial: StatusAdapter.isPartial(operation),
            continuing: availability(of: operation) == .continuing)
    }
}
