/// The lifecycle state of an instance, as the UI shows it: the ten states of spec 0004.
///
/// The app shell (spec 0013) maps the daemon's DTO to this enum with an exhaustive
/// switch, so a new protocol state fails to compile until the status table covers it.
nonisolated public enum LifecycleStatus: CaseIterable, Hashable, Sendable {
    /// Created, its data not initialized yet.
    case uninitialized
    /// Initializing its data.
    case initializing
    /// Not running.
    case stopped
    /// Being started.
    case starting
    /// Running. Readiness and health say whether it serves.
    case running
    /// Being restarted by the supervisor after a crash.
    case recovering
    /// Being stopped.
    case stopping
    /// Stopped by a failure.
    case failed
    /// The daemon cannot confirm the process identity.
    case unknown
    /// Removed, with its folder kept under `retained/`.
    case retained
}

/// The readiness of a running instance, after the daemon applied
/// `effective(now, max_age)` (spec 0004). The UI never computes freshness itself.
nonisolated public enum ReadinessStatus: Hashable, Sendable {
    /// The readiness probe passed.
    case ready
    /// Not ready yet because the first probe has not run (`not_ready` with reason
    /// `awaiting_probe`).
    case awaitingProbe
    /// Not ready for another reason, with the daemon's detail when it sent one (at most
    /// 256 characters, already free of secrets).
    case notReady(detail: String?)
    /// Readiness is unknown, including a stale result.
    case unknown
}

/// The health of a ready instance, after the daemon applied `effective(now, max_age)`.
nonisolated public enum HealthStatus: CaseIterable, Hashable, Sendable {
    /// No health result.
    case unknown
    /// Healthy.
    case healthy
    /// Working, but degraded.
    case degraded
    /// Unhealthy.
    case unhealthy
}

/// The state of an operation, as the UI shows it: the operation states of spec 0004.
nonisolated public enum OperationStatus: CaseIterable, Hashable, Sendable {
    /// Waiting to run.
    case queued
    /// Running.
    case running
    /// The user asked to cancel it, and it has not stopped yet.
    case cancelRequested
    /// Finished. A partial result shows as Partial.
    case succeeded
    /// Failed.
    case failed
    /// Cancelled.
    case cancelled
    /// Stopped halfway, and it needs recovery.
    case needsRecovery
}

/// The one extra fact a status label shows after its label, with its own tone.
nonisolated public struct StatusDetail: Hashable, Sendable {
    /// The detail text, for example "Waiting for probe".
    public let text: String
    /// The tone of the detail, which may differ from the status tone.
    public let tone: Tone

    /// Creates a status detail.
    public init(text: String, tone: Tone) {
        self.text = text
        self.tone = tone
    }
}

/// How a status looks: its label, tone, symbol, and at most one detail.
///
/// Build it from the status tables of spec 0012 with ``instance(lifecycle:readiness:health:)``
/// or ``operation(state:partial:)``, and show it with ``StatusLabel`` or ``ToneBadge``. A new
/// status set (for example runtime states) maps its states to the five tones by meaning,
/// creates its appearances with ``init(label:tone:symbol:detail:)``, and adds its table to
/// `apps/macos/design.md`.
nonisolated public struct StatusAppearance: Hashable, Sendable {
    /// The status text, always shown, for example "Running".
    public let label: String
    /// The tone of the status.
    public let tone: Tone
    /// The SF Symbol that replaces the status dot while Differentiate Without Color is on.
    /// Each state has its own shape, so states of one tone stay apart without color.
    public let symbol: String
    /// The optional detail shown after the label.
    public let detail: StatusDetail?

    /// Creates a status appearance.
    public init(label: String, tone: Tone, symbol: String, detail: StatusDetail? = nil) {
        self.label = label
        self.tone = tone
        self.symbol = symbol
        self.detail = detail
    }

    /// The appearance of an instance status, from the instance status table of spec 0012.
    ///
    /// Readiness is read only while `lifecycle` is `running`, and health only while
    /// readiness is `ready`. "Running" is `positive` only when the instance is running
    /// and ready; running while awaiting the first probe shows "Starting…".
    ///
    /// - Parameters:
    ///   - lifecycle: The lifecycle state.
    ///   - readiness: The effective readiness from the daemon.
    ///   - health: The effective health from the daemon.
    public static func instance(
        lifecycle: LifecycleStatus,
        readiness: ReadinessStatus,
        health: HealthStatus
    ) -> StatusAppearance {
        switch lifecycle {
        case .uninitialized:
            StatusAppearance(label: "Not Initialized", tone: .neutral, symbol: "circle.dashed")
        case .initializing:
            StatusAppearance(label: "Initializing…", tone: .caution, symbol: Symbol.hourglass)
        case .stopped:
            StatusAppearance(label: "Stopped", tone: .neutral, symbol: "stop.circle.fill")
        case .starting:
            StatusAppearance(label: "Starting…", tone: .caution, symbol: Symbol.hourglass)
        case .running:
            running(readiness: readiness, health: health)
        case .recovering:
            StatusAppearance(
                label: "Recovering…",
                tone: .caution,
                symbol: "arrow.clockwise.circle.fill"
            )
        case .stopping:
            StatusAppearance(label: "Stopping…", tone: .caution, symbol: Symbol.hourglass)
        case .failed:
            StatusAppearance(label: "Failed", tone: .critical, symbol: "xmark.circle.fill")
        case .unknown:
            StatusAppearance(
                label: "Unknown",
                tone: .caution,
                symbol: Symbol.questionMark,
                detail: StatusDetail(text: "Process identity not confirmed", tone: .caution)
            )
        case .retained:
            StatusAppearance(label: "Retained", tone: .neutral, symbol: "archivebox.circle.fill")
        }
    }

    /// The appearance of an operation status, from the operation status table of spec 0012.
    ///
    /// Operations show as a ``ToneBadge``, so the symbol is the tone symbol and there is no
    /// detail.
    ///
    /// - Parameters:
    ///   - state: The operation state.
    ///   - partial: Whether a succeeded operation finished only in part. Ignored for every
    ///     other state.
    public static func operation(state: OperationStatus, partial: Bool) -> StatusAppearance {
        let (label, tone): (String, Tone) =
            switch state {
            case .queued: ("Queued", .neutral)
            case .running: ("Running", .informative)
            case .cancelRequested: ("Cancelling…", .caution)
            case .succeeded: partial ? ("Partial", .caution) : ("Done", .neutral)
            case .failed: ("Failed", .critical)
            case .cancelled: ("Cancelled", .neutral)
            case .needsRecovery: ("Needs Recovery", .critical)
            }
        return StatusAppearance(label: label, tone: tone, symbol: tone.symbol)
    }

    /// The running rows of the instance status table.
    private static func running(
        readiness: ReadinessStatus,
        health: HealthStatus
    ) -> StatusAppearance {
        switch readiness {
        case .awaitingProbe:
            StatusAppearance(
                label: "Starting…",
                tone: .caution,
                symbol: Symbol.hourglass,
                detail: StatusDetail(text: "Waiting for probe", tone: .caution)
            )
        case .notReady(let detail):
            StatusAppearance(
                label: "Not Ready",
                tone: .caution,
                symbol: "exclamationmark.circle.fill",
                detail: StatusDetail(text: notReadyText(detail), tone: .caution)
            )
        case .unknown:
            StatusAppearance(
                label: "Running",
                tone: .caution,
                symbol: Symbol.questionMark,
                detail: StatusDetail(text: "Readiness unknown", tone: .caution)
            )
        case .ready:
            StatusAppearance(
                label: "Running",
                tone: .positive,
                symbol: "checkmark.circle.fill",
                detail: healthDetail(health)
            )
        }
    }

    /// The detail of a ready instance: nothing while healthy or unknown.
    private static func healthDetail(_ health: HealthStatus) -> StatusDetail? {
        switch health {
        case .unknown, .healthy: nil
        case .degraded: StatusDetail(text: "Degraded", tone: .caution)
        case .unhealthy: StatusDetail(text: "Unhealthy", tone: .critical)
        }
    }

    /// The daemon's not ready detail, or the fallback when it sent none or only white space.
    static func notReadyText(_ detail: String?) -> String {
        guard let detail, detail.allSatisfy(\.isWhitespace) == false else {
            return "Readiness check failed"
        }
        return detail
    }

    /// Status symbols that more than one state shares.
    private enum Symbol {
        static let hourglass = "hourglass.circle.fill"
        static let questionMark = "questionmark.circle.fill"
    }
}
