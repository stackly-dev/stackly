import StacklyClient

/// How a runtime state looks (spec 0018, *Status runtime*, and `apps/macos/design.md`): the
/// label, tone, and symbol of every ``RuntimeState``.
///
/// The mapping is one exhaustive `switch` without `default`, so a state added to the
/// protocol fails to compile until the table covers it. A state from a newer daemon
/// (`unrecognized`) is "Unrecognized State", never guessed into another state.
nonisolated public enum RuntimeAppearance {
    /// "3 installed": the status of a Catalog card whose software has runtimes installed.
    public static func installedCount(_ count: Int) -> StatusAppearance {
        StatusAppearance(
            label: "\(count) installed", tone: .positive, symbol: "checkmark.circle.fill")
    }

    /// The appearance of a runtime state.
    public static func appearance(of state: RuntimeState) -> StatusAppearance {
        switch state {
        case .installed:
            StatusAppearance(label: "Installed", tone: .positive, symbol: "checkmark.circle.fill")
        case .resolving:
            StatusAppearance(
                label: "Resolving…", tone: .informative, symbol: "magnifyingglass.circle.fill")
        case .downloading:
            StatusAppearance(
                label: "Downloading…", tone: .informative, symbol: "arrow.down.circle.fill")
        case .verifying:
            StatusAppearance(
                label: "Verifying…", tone: .informative, symbol: "checkmark.seal.fill")
        case .installing:
            StatusAppearance(
                label: "Installing…", tone: .informative,
                symbol: "arrow.triangle.2.circlepath.circle.fill")
        case .failed:
            StatusAppearance(label: "Failed", tone: .critical, symbol: "xmark.circle.fill")
        case .missing:
            StatusAppearance(
                label: "Missing", tone: .critical, symbol: "exclamationmark.octagon.fill")
        case .drifted:
            StatusAppearance(
                label: "Drifted", tone: .caution, symbol: "exclamationmark.triangle.fill")
        case .absent:
            StatusAppearance(label: "Removed", tone: .neutral, symbol: "minus.circle.fill")
        case .unrecognized:
            CatalogAppearance.unrecognized
        }
    }
}
