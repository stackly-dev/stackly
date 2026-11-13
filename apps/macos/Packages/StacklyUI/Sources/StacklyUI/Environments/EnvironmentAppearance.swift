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
}
