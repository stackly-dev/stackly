import StacklyClient

/// How a catalog status looks (spec 0017, *Tone dan label*, and `apps/macos/design.md`):
/// the label, tone, symbol, and detail of every ``SupportStatus`` and
/// ``CatalogEntryState``.
///
/// Each mapping is one exhaustive `switch` without `default`, so a value added to the
/// protocol fails to compile until the table covers it. A value from a newer daemon
/// (`unrecognized`) is "Unrecognized State", never guessed into another status. Unsupported
/// is neutral because it is a fact about the build, not a failure; blocked is `caution`
/// because it waits for a maintainer, not for the user.
nonisolated public enum CatalogAppearance {
    /// The detail that goes with an unrecognized state.
    static let unrecognizedDetail = StatusDetail(
        text: "Update Stackly to see this state", tone: .caution)

    /// The appearance of a version's status on one OS.
    public static func appearance(of status: SupportStatus) -> StatusAppearance {
        switch status {
        case .supported:
            StatusAppearance(label: "Tested", tone: .positive, symbol: "checkmark.circle.fill")
        case .experimental:
            StatusAppearance(
                label: "Experimental", tone: .caution, symbol: "exclamationmark.triangle.fill")
        case .unverified:
            StatusAppearance(
                label: "Unverified", tone: .neutral, symbol: "questionmark.circle.fill")
        case .planned:
            StatusAppearance(label: "Planned", tone: .neutral, symbol: "clock.fill")
        case .unsupported:
            StatusAppearance(label: "Unsupported", tone: .neutral, symbol: "minus.circle.fill")
        case .blocked:
            StatusAppearance(label: "Blocked", tone: .caution, symbol: "lock.circle.fill")
        case .unrecognized:
            unrecognized
        }
    }

    /// The appearance of a provider's state on a card.
    public static func appearance(of state: CatalogEntryState) -> StatusAppearance {
        switch state {
        case .available:
            StatusAppearance(
                label: "Available", tone: .neutral, symbol: "arrow.down.circle.fill")
        case .blocked:
            StatusAppearance(label: "Blocked", tone: .caution, symbol: "lock.circle.fill")
        case .unsupported:
            StatusAppearance(label: "Unsupported", tone: .neutral, symbol: "minus.circle.fill")
        case .planned:
            StatusAppearance(label: "Planned", tone: .neutral, symbol: "clock.fill")
        case .invalid:
            StatusAppearance(label: "Invalid", tone: .critical, symbol: "xmark.circle.fill")
        case .unrecognized:
            unrecognized
        }
    }

    /// "Unrecognized State": the state a newer daemon sent that this build does not know.
    static var unrecognized: StatusAppearance {
        StatusAppearance(
            label: "Unrecognized State", tone: .caution, symbol: "questionmark.diamond.fill",
            detail: unrecognizedDetail)
    }
}
