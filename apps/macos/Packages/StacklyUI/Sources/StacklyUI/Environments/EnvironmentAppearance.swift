import StacklyClient

extension StatusAppearance {
    /// The appearance of an environment status (spec 0044, *Layar*, and `apps/macos/design.md`,
    /// *Environment status*). A status from a newer daemon (`unrecognized`) is "Unknown".
    /// - Parameter status: The status the daemon computed from the ready count.
    public static func environment(status: EnvironmentStatus) -> StatusAppearance {
        switch status {
        case .ready:
            StatusAppearance(label: "Ready", tone: .positive, symbol: "checkmark.circle.fill")
        case .partiallyReady:
            StatusAppearance(
                label: "Partially Ready", tone: .caution, symbol: "circle.lefthalf.filled")
        case .down:
            StatusAppearance(label: "Down", tone: .neutral, symbol: "circle")
        case .unrecognized:
            StatusAppearance(label: "Unknown", tone: .neutral, symbol: "questionmark.circle")
        }
    }

    /// A service of an environment whose instance does not exist yet: "Not created".
    public static let serviceNotCreated = StatusAppearance(
        label: "Not created", tone: .neutral, symbol: "circle.dashed")

    /// The phase of a service while an Up works on it (spec 0046, *Copy phase*, and
    /// `apps/macos/design.md`, *Up phase*). An ended phase (`ready`, `failed`, `needs_secret`,
    /// `skipped`, `cancelled`) is never drawn with it: the Services table shows the instance
    /// status for those, so they read like `unrecognized`, "Working…".
    /// - Parameters:
    ///   - phase: The phase from `notReady.phase`.
    ///   - waitingOn: The aliases a `waiting` service waits for.
    nonisolated public static func upPhase(phase: UpPhase, waitingOn: [String] = [])
        -> StatusAppearance
    {
        switch phase {
        case .pending:
            StatusAppearance(label: "Queued", tone: .neutral, symbol: "clock")
        case .installing:
            StatusAppearance(
                label: "Installing…", tone: .informative, symbol: "arrow.down.circle")
        case .creating:
            StatusAppearance(label: "Creating…", tone: .informative, symbol: "plus.circle")
        case .waiting:
            StatusAppearance(
                label: waitingLabel(waitingOn), tone: .neutral, symbol: "hourglass")
        case .starting:
            StatusAppearance(label: "Starting…", tone: .informative, symbol: "play.circle")
        case .ready, .failed, .needsSecret, .skipped, .cancelled, .unrecognized:
            StatusAppearance(label: "Working…", tone: .neutral, symbol: "ellipsis.circle")
        }
    }

    /// "Waiting for {alias}", with "+{n}" for more than one, or "Waiting" without one.
    nonisolated static func waitingLabel(_ aliases: [String]) -> String {
        guard let first = aliases.first else { return "Waiting" }
        return aliases.count > 1
            ? "Waiting for \(first) +\(aliases.count - 1)" : "Waiting for \(first)"
    }

    /// An environment whose Up runs: "Bringing up…" (spec 0046, *Sidebar, daftar, Overview*).
    public static let environmentBringingUp = StatusAppearance(
        label: "Bringing up…", tone: .informative, symbol: "arrow.triangle.2.circlepath")

    /// An environment whose Down runs: "Bringing down…" (spec 0047 fills the banner).
    public static let environmentBringingDown = StatusAppearance(
        label: "Bringing down…", tone: .informative, symbol: "arrow.triangle.2.circlepath")

    /// The status of an environment in the sidebar, the list, and the Overview: "Bringing up…"
    /// or "Bringing down…" while `activeOperation` is set, else its status.
    public static func environment(_ summary: EnvironmentSummary) -> StatusAppearance {
        switch summary.activeOperation?.kind {
        case .environmentUp: environmentBringingUp
        case .environmentDown: environmentBringingDown
        case .some, nil: environment(status: summary.status)
        }
    }
}
