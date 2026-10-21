import StacklyClient

/// One stage of an operation: the `stage` value the daemon reports and the title the
/// screen shows for it.
///
/// The screen that owns the operation defines its stages: five for an install (spec
/// 0018) and six for create (spec 0020).
nonisolated public struct StageDefinition: Equatable, Sendable {
    /// The value of `OperationSummary.stage` for this stage.
    public var key: String
    /// The title shown for the stage.
    public var title: String

    /// Creates a stage definition.
    ///
    /// - Parameters:
    ///   - key: The value of `OperationSummary.stage`.
    ///   - title: The title shown for the stage.
    public init(key: String, title: String) {
        self.key = key
        self.title = title
    }
}

/// Where one stage is.
nonisolated public enum StageState: Equatable, Sendable {
    /// Not reached yet.
    case pending
    /// Running now.
    case running
    /// Finished.
    case done
    /// The stage the operation failed at.
    case failed
    /// The stage the operation was cancelled at.
    case cancelled
}

/// One stage with its state.
nonisolated public struct ProgressStage: Equatable, Sendable {
    /// The value of `OperationSummary.stage` for this stage.
    public var key: String
    /// The title shown for the stage.
    public var title: String
    /// Where the stage is.
    public var state: StageState

    /// Creates a stage with a state.
    ///
    /// - Parameters:
    ///   - key: The value of `OperationSummary.stage`.
    ///   - title: The title shown for the stage.
    ///   - state: Where the stage is.
    public init(key: String, title: String, state: StageState) {
        self.key = key
        self.title = title
        self.state = state
    }
}

/// How the whole operation stands.
nonisolated public enum ProgressOutcome: Equatable, Sendable {
    /// Queued behind another operation.
    case waiting
    /// Running.
    case running
    /// A cancel was asked for and the operation winds down.
    case cancelling
    /// Finished with success.
    case succeeded
    /// Finished with a failure.
    case failed
    /// Stopped in a way that needs the person's action.
    case needsRecovery
    /// Cancelled.
    case cancelled
}

/// What a staged progress shows, worked out from an operation (spec 0067, AC-14): the state
/// of every stage, the fraction and percent, the title, and the outcome.
///
/// ``from(operation:stages:)`` is a pure function, so every row of the *Pemetaan operasi ke
/// tahap* table is tested without a view.
nonisolated public struct StagedProgressValue: Equatable, Sendable {
    /// The stages in order, each with its state.
    public var stages: [ProgressStage]
    /// The index of the stage that is running, failed, or was cancelled, or `nil`.
    public var runningIndex: Int?
    /// How far the bar is from 0 to 1, or `nil` for an indeterminate bar or no bar.
    public var fraction: Double?
    /// The percent shown, whole and at most 99 until the operation succeeded, or `nil`.
    public var percent: Int?
    /// The title of what the operation is doing or how it ended.
    public var currentTitle: String
    /// How the whole operation stands.
    public var outcome: ProgressOutcome

    /// Creates a value.
    ///
    /// - Parameters:
    ///   - stages: The stages in order, each with its state.
    ///   - runningIndex: The index of the running, failed, or cancelled stage.
    ///   - fraction: How far the bar is, or `nil`.
    ///   - percent: The percent shown, or `nil`.
    ///   - currentTitle: The title of what the operation is doing or how it ended.
    ///   - outcome: How the whole operation stands.
    public init(
        stages: [ProgressStage], runningIndex: Int?, fraction: Double?, percent: Int?,
        currentTitle: String, outcome: ProgressOutcome
    ) {
        self.stages = stages
        self.runningIndex = runningIndex
        self.fraction = fraction
        self.percent = percent
        self.currentTitle = currentTitle
        self.outcome = outcome
    }

    /// Works the value out from an operation.
    ///
    /// - Parameters:
    ///   - operation: The operation, from `OperationCenter`.
    ///   - stages: The stages of the operation, in order, from the screen that owns it.
    public static func from(
        operation: OperationSummary, stages definitions: [StageDefinition]
    ) -> StagedProgressValue {
        let known = operation.stage.flatMap { key in
            definitions.firstIndex { $0.key == key }
        }
        switch operation.state {
        case .queued:
            return value(definitions, index: nil, state: .pending, title: "Queued", .waiting)
        case .running, .cancelRequested:
            return working(operation, definitions, known: known)
        case .succeeded:
            return StagedProgressValue(
                stages: stages(definitions, index: nil, state: .done, others: .done),
                runningIndex: nil, fraction: 1, percent: 100, currentTitle: "Done",
                outcome: .succeeded)
        case .failed:
            let title = known.map { "Failed at \(definitions[$0].title)" } ?? "Failed"
            return value(definitions, index: known, state: .failed, title: title, .failed)
        case .needsRecovery:
            return value(
                definitions, index: known, state: .failed, title: "Needs Recovery",
                .needsRecovery)
        case .cancelled:
            return value(
                definitions, index: known, state: .cancelled, title: "Cancelled", .cancelled)
        }
    }

    /// The row of a running or cancelling operation.
    private static func working(
        _ operation: OperationSummary, _ definitions: [StageDefinition], known: Int?
    ) -> StagedProgressValue {
        let isCancelling = operation.state == .cancelRequested
        let outcome: ProgressOutcome = isCancelling ? .cancelling : .running
        let index = definitions.isEmpty ? nil : (operation.stage == nil ? 0 : known)
        guard let index else {
            return value(
                definitions, index: nil, state: .pending,
                title: isCancelling ? "Cancelling…" : "Working…", outcome)
        }
        let (fraction, percent) =
            operation.stage == nil
            ? (0, 0)
            : measure(index: index, count: definitions.count, progress: operation.progress)
        return StagedProgressValue(
            stages: stages(definitions, index: index, state: .running, others: .pending),
            runningIndex: index, fraction: fraction, percent: percent,
            currentTitle: isCancelling ? "Cancelling…" : definitions[index].title,
            outcome: outcome)
    }

    /// The bar fraction and the percent of stage `index` of `count`: the stages before it
    /// are done and the stage itself is `progress` done. The percent is whole, rounded
    /// down, and at most 99 so "100%" never shows for work that is not finished.
    static func measure(index: Int, count: Int, progress: OperationProgress) -> (Double, Int) {
        let numerator: UInt128
        let denominator: UInt128
        if case .counted(let counted) = progress, counted.total > 0 {
            let total = UInt128(counted.total)
            numerator = UInt128(index) * total + UInt128(min(counted.done, counted.total))
            denominator = UInt128(count) * total
        } else {
            numerator = UInt128(index)
            denominator = UInt128(count)
        }
        let percent = min(99, Int(numerator * 100 / denominator))
        return (Double(numerator) / Double(denominator), percent)
    }

    /// A value with no bar: every stage `pending`, or the stages around `index`.
    private static func value(
        _ definitions: [StageDefinition], index: Int?, state: StageState, title: String,
        _ outcome: ProgressOutcome
    ) -> StagedProgressValue {
        StagedProgressValue(
            stages: stages(definitions, index: index, state: state, others: .pending),
            runningIndex: index, fraction: nil, percent: nil, currentTitle: title,
            outcome: outcome)
    }

    /// The stages with `state` at `index`, `done` before it, and `others` after it. With no
    /// index every stage takes `others`.
    private static func stages(
        _ definitions: [StageDefinition], index: Int?, state: StageState, others: StageState
    ) -> [ProgressStage] {
        definitions.enumerated().map { position, definition in
            let stageState: StageState
            if let index {
                stageState = position < index ? .done : (position == index ? state : others)
            } else {
                stageState = others
            }
            return ProgressStage(key: definition.key, title: definition.title, state: stageState)
        }
    }
}

extension StagedProgressValue {
    /// Whether the bar shows: while the operation runs or winds down.
    public var showsBar: Bool {
        outcome == .running || outcome == .cancelling
    }

    /// The line of the compact layout, per the *Teks progress* table.
    public var compactText: String {
        switch outcome {
        case .running where percent != nil, .cancelling where percent != nil:
            "\(currentTitle) · \(percent ?? 0)%"
        default:
            currentTitle
        }
    }

    /// The value VoiceOver reads, per the *Teks progress* table.
    public var accessibilityValue: String {
        switch outcome {
        case .waiting, .succeeded, .failed, .needsRecovery, .cancelled:
            return currentTitle
        case .running:
            guard let percent, let runningIndex else { return "Working" }
            return
                "Stage \(runningIndex + 1) of \(stages.count), \(currentTitle), \(percent) percent"
        case .cancelling:
            guard let percent, let runningIndex else { return "Cancelling" }
            return "Cancelling, stage \(runningIndex + 1) of \(stages.count), \(percent) percent"
        }
    }

    /// What to announce when the running stage may have changed: "{title}." for a running
    /// stage, `nil` otherwise.
    public var stageAnnouncement: String? {
        guard outcome == .running, let runningIndex, stages.indices.contains(runningIndex) else {
            return nil
        }
        return "\(stages[runningIndex].title)."
    }
}
