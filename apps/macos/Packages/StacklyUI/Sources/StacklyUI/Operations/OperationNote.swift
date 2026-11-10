import StacklyClient

/// The note line of an operation on the Operations page and in Needs Attention (spec 0035,
/// *Baris catatan*). A pure function of the summary, so the page, the Overview, and the
/// fixtures say the same thing.
nonisolated public enum OperationNote {
    /// The note of a row: its text and whether it is a failure, which the row writes in the
    /// critical text color (spec 0012).
    public struct Line: Equatable, Sendable {
        /// The text.
        public let text: String
        /// Whether the text says why the operation failed.
        public let isCritical: Bool
    }

    /// What a row adds after the stage while Cancel is off at a step that can’t be undone.
    static let notCancellable = " · can’t be cancelled"

    /// The note line of `operation`.
    /// - Parameter operation: The summary.
    public static func line(for operation: OperationSummary) -> Line {
        let cancel = OperationCancel.availability(of: operation)
        let stage = OperationStages.label(for: operation)
        switch operation.state {
        case .queued:
            return plain(operation.waitingOn == nil ? "Queued" : "Waiting for another operation")
        case .running:
            let suffix = cancel == .pastPointOfNoReturn ? notCancellable : ""
            if let percent = percent(of: operation) {
                return plain("\(percent)% · \(stage)\(suffix)")
            }
            return plain("\(stage)\(suffix)")
        case .cancelRequested:
            return plain(
                cancel == .continuing ? "Finishing safely · \(stage)" : "Cancelling · \(stage)")
        case .needsRecovery:
            return plain(recovery(operation.recovery?.hint))
        case .failed:
            guard let error = operation.failure?.error else {
                return Line(text: "Failed", isCritical: true)
            }
            return Line(
                text: "\(DaemonText.sentence(error.message)) · \(error.code)", isCritical: true)
        case .cancelled:
            let failedPart = operation.result?.partial?.contains { $0.outcome == .failed } == true
            return plain(failedPart ? "Cancelled" : "Cancelled · undone")
        case .succeeded:
            if let partial = operation.result?.partial, partial.isEmpty == false {
                let done = partial.count { $0.outcome == .done }
                return plain("\(done) of \(partial.count) done")
            }
            return plain(operation.cancelRequestedAt == nil ? "Done" : "Done after cancel request")
        }
    }

    /// What an operation that needs recovery can do next, by its hint: the note of a
    /// `needs_recovery` row and the body of item O1 (spec 0035, *Navigasi*).
    /// - Parameter hint: The hint of its recovery, or `nil` when the summary carries none.
    public static func recovery(_ hint: RecoveryHint?) -> String {
        switch hint {
        case .resume: "It can continue. Resume or abandon it."
        case .abandon: "It can’t continue. Abandon it to clean up."
        case .manual: "It needs manual cleanup. Open it for details."
        case .autoCompensate: "Stackly is undoing it."
        case .unrecognized, nil: "Open it for details."
        }
    }

    /// The percent of a running operation with counted progress: of the whole operation for
    /// a kind with stages (the percent its bar shows), else of the counted work. At most 99,
    /// so "100%" never shows for work that is not finished. `nil` without counted progress.
    static func percent(of operation: OperationSummary) -> Int? {
        guard case .counted(let counted) = operation.progress, counted.total > 0 else {
            return nil
        }
        if let stages = OperationStages.definitions(for: operation.kind),
            let percent = StagedProgressValue.from(operation: operation, stages: stages).percent
        {
            return percent
        }
        return min(99, Int(min(counted.done, counted.total) * 100 / counted.total))
    }

    private static func plain(_ text: String) -> Line {
        Line(text: text, isCritical: false)
    }
}
