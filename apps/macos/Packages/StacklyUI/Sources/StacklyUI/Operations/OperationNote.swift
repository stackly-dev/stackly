import StacklyClient

/// The note line of an operation on the Operations page and in Needs Attention (spec 0035,
/// *Baris catatan*). A pure function of the summary, so the page, the Overview, and the
/// fixtures say the same thing.
nonisolated public enum OperationNote {
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
}
