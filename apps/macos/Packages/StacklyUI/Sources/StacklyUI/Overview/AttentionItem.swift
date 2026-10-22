import Foundation

/// Where an item of Needs Attention leads.
nonisolated public enum AttentionDestination: Hashable, Sendable {
    /// The instance, through ``ShellNavigator/openInstance(_:)``.
    case instance(UUID)
    /// The Operations page.
    case operations
    /// The Diagnostics page.
    case diagnostics

    /// The name VoiceOver reads in the hint "Opens {name}.".
    var spokenName: String {
        switch self {
        case .instance: "the instance"
        case .operations: "Operations"
        case .diagnostics: "Diagnostics"
        }
    }
}

/// What kind of thing an item is about; it orders the items after their tone.
nonisolated public enum AttentionCategory: Int, Comparable, Hashable, Sendable {
    /// The disk.
    case disk = 0
    /// An operation.
    case operation = 1
    /// An instance.
    case instance = 2

    /// Orders disk before operation before instance.
    public static func < (lhs: Self, rhs: Self) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

/// One line of Needs Attention (spec 0022, *Aturan Needs Attention*).
nonisolated public struct AttentionItem: Identifiable, Hashable, Sendable {
    /// A stable ID: `instance:<uuid>`, `operation:<uuid>`, `operations` (rule O2), or `disk`.
    public let id: String
    /// The rule that made the item, such as `I3` or `S2`.
    public let rule: String
    /// How serious it is.
    public let tone: Tone
    /// What it is about.
    public let category: AttentionCategory
    /// The title.
    public let title: String
    /// The body: what happened and the way out.
    public let message: String
    /// Where the item leads.
    public let destination: AttentionDestination
    /// The key that orders items of the same tone, category, and rule: the instance name,
    /// or the title of the operation.
    let sortKey: String

    /// The word VoiceOver reads before the title: "Critical", "Warning", or "Info".
    public var spokenLevel: String {
        switch tone {
        case .critical: "Critical"
        case .caution: "Warning"
        case .positive, .neutral, .informative: "Info"
        }
    }

    /// What VoiceOver reads: "{level}: {title}. {message}".
    public var spokenLabel: String {
        "\(spokenLevel): \(title). \(message)"
    }

    /// The hint VoiceOver reads.
    public var spokenHint: String {
        "Opens \(destination.spokenName)."
    }

    /// The number in the rule, for ordering: `I10` is 10.
    var ruleNumber: Int {
        Int(rule.dropFirst()) ?? 0
    }
}
