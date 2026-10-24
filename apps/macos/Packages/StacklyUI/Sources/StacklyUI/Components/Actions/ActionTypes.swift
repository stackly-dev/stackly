/// The copy of the confirmation an action asks for before it runs: the same three parts
/// as a ``ConfirmationRequest``, without the role and the action, which
/// ``StacklyAction`` adds.
/// The copy follows the confirmation rules of `apps/macos/design.md`: the title is a
/// question naming the object, the message says what happens and what stays safe, and the
/// action title is the verb from the title.
nonisolated public struct ActionConfirmation: Equatable, Sendable {
    /// The question: "Remove “pg17-dev”?".
    public var title: String
    /// What happens, then what stays safe.
    public var message: String
    /// The verb from the title: "Remove".
    public var actionTitle: String

    /// Creates the confirmation copy.
    /// - Parameters:
    ///   - title: The question, naming the object.
    ///   - message: What happens, then what stays safe.
    ///   - actionTitle: The verb from the title.
    public init(title: String, message: String, actionTitle: String) {
        self.title = title
        self.message = message
        self.actionTitle = actionTitle
    }
}

/// What kind of action it is, and the confirmation it carries.
/// A destructive action always carries a confirmation, so one without a dialog cannot be
/// written.
nonisolated public enum ActionRole: Equatable, Sendable {
    /// An ordinary action, with a confirmation only when it interrupts work.
    case standard(confirmation: ActionConfirmation?)
    /// An action that loses data or disturbs another owner: a red button and a
    /// confirmation with Cancel as the default.
    case destructive(ActionConfirmation)
}

/// Whether an action can run now, and why not.
nonisolated public enum ActionAvailability: Equatable, Sendable {
    /// It can run.
    case enabled
    /// It is running; it cannot start again.
    case busy
    /// It cannot run, for `reason`. With `showsReason` the reason is the action's own
    /// (a capability the provider lacks, a state, a failed check) and is written next to
    /// the control; without it the reason is the mutation gate's, which the window banner
    /// already writes.
    case disabled(reason: String, showsReason: Bool)

    /// The words a menu shows under a disabled item: the reason of an action's own, or
    /// "In progress" while it runs. `nil` for an enabled action and for a gate reason,
    /// which the window banner writes.
    var menuReason: String? {
        switch self {
        case .enabled: nil
        case .busy: "In progress"
        case .disabled(let reason, let showsReason): showsReason ? reason : nil
        }
    }
}
