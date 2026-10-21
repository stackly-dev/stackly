/// Whether a diff line was added, removed, or left as it was.
nonisolated public enum DiffKind: Equatable, Sendable {
    /// The line is new: a "+" mark in positive text on a positive tint.
    case added
    /// The line is gone: a "−" mark in critical text on a critical tint.
    case removed
    /// The line did not change: secondary text, no mark, no tint.
    case unchanged

    /// The mark in the sign column, or `nil` for an unchanged line. The mark always
    /// shows, so the kind never depends on color.
    public var mark: String? {
        switch self {
        case .added: "+"
        case .removed: "−"
        case .unchanged: nil
        }
    }

    /// The tone of the mark and the tint, or `nil` for an unchanged line.
    public var tone: Tone? {
        switch self {
        case .added: .positive
        case .removed: .critical
        case .unchanged: nil
        }
    }

    /// The word VoiceOver reads before the text.
    public var word: String {
        switch self {
        case .added: "Added"
        case .removed: "Removed"
        case .unchanged: "Unchanged"
        }
    }
}

/// One line of a ``DiffView``.
///
/// Feature 29 fills it from a config diff or a drift.
nonisolated public struct DiffLine: Identifiable, Equatable, Sendable {
    /// The position of the line in the diff; it is the stable key.
    public var id: Int
    /// Whether the line was added, removed, or left as it was.
    public var kind: DiffKind
    /// The text of the line.
    public var text: String

    /// Creates a line.
    ///
    /// - Parameters:
    ///   - id: The position of the line in the diff.
    ///   - kind: Added, removed, or unchanged.
    ///   - text: The text.
    public init(id: Int, kind: DiffKind, text: String) {
        self.id = id
        self.kind = kind
        self.text = text
    }

    /// What VoiceOver reads for the line: "Added: {text}", "Removed: {text}", or
    /// "Unchanged: {text}".
    public var accessibilityLabel: String {
        "\(kind.word): \(text)"
    }
}
