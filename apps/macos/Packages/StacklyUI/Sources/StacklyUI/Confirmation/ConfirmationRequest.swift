import Foundation

/// A request to confirm one action, shown by ``SwiftUI/View/stacklyConfirmation(item:)``.
/// Confirm an action that deletes or overwrites data, deletes a record or a runtime,
/// kills a process without a graceful stop, or disturbs another owner. Start, stop, and
/// restart without dependents, reload, copy, cancelling an operation, and cleaning up a
/// cache that can be downloaded again run without one. `apps/macos/design.md`
/// (Confirmation) lists every action and its role.
/// The copy follows fixed rules: the title is a question that names the object, in
/// curly quotes when it is an identifier ("Remove “pg17-dev”?"); the message says what
/// happens and then what stays safe ("Data is kept."); the action title is the verb from
/// the title ("Remove"), never "OK" or "Yes".
public struct ConfirmationRequest {
    /// What the confirmed action does to the user's data or to other owners.
    nonisolated public enum Role: CaseIterable, Sendable {
        /// Loses data, deletes something, or disturbs another owner. The action button is
        /// red and Cancel is the default button, so Return and Escape both cancel.
        case destructive
        /// Interrupts work but keeps data and leaves other owners alone. The action
        /// button is the default button, so Return runs it, and Escape cancels.
        case standard
    }

    /// The question the dialog asks, naming the object: "Remove “pg17-dev”?".
    public var title: String
    /// One or two sentences: what happens, then what stays safe.
    public var message: String
    /// The label of the action button: the verb from the title, such as "Remove".
    public var actionTitle: String
    /// Whether the action is destructive.
    public var role: Role
    /// Runs the action after the user confirms it. The dialog never calls it on Cancel.
    public var perform: @MainActor () -> Void

    /// Tells requests apart: a new request replaces the one shown, even with equal copy.
    let id = UUID()

    /// Creates a confirmation request.
    /// - Parameters:
    ///   - title: The question, naming the object.
    ///   - message: What happens, then what stays safe.
    ///   - actionTitle: The verb from the title.
    ///   - role: Whether the action is destructive.
    ///   - perform: The action, run only when the user confirms.
    public init(
        title: String,
        message: String,
        actionTitle: String,
        role: Role,
        perform: @escaping @MainActor () -> Void
    ) {
        self.title = title
        self.message = message
        self.actionTitle = actionTitle
        self.role = role
        self.perform = perform
    }
}

/// A break of the confirmation copy rules in `apps/macos/design.md` (Confirmation).
nonisolated enum ConfirmationCopyProblem: Equatable, Sendable {
    /// The title is empty.
    case emptyTitle
    /// The title is not a question.
    case titleIsNotAQuestion
    /// The message is empty.
    case emptyMessage
    /// The action title is empty.
    case emptyActionTitle
    /// The action title is a generic answer ("OK", "Yes") instead of a verb.
    case genericActionTitle
    /// The action title is not a word from the title.
    case actionTitleNotInTitle
}

extension ConfirmationRequest {
    /// The action titles that never label a confirmation button.
    nonisolated static let genericActionTitles: Set<String> = ["ok", "okay", "yes"]

    /// Every way this request breaks the copy rules; empty when it follows them.
    /// Tests of a screen call it on each request the screen builds.
    var copyProblems: [ConfirmationCopyProblem] {
        let title = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let action = actionTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        var problems: [ConfirmationCopyProblem] = []
        if title.isEmpty {
            problems.append(.emptyTitle)
        } else if title.hasSuffix("?") == false {
            problems.append(.titleIsNotAQuestion)
        }
        if message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            problems.append(.emptyMessage)
        }
        if action.isEmpty {
            problems.append(.emptyActionTitle)
        } else if Self.genericActionTitles.contains(action.lowercased()) {
            problems.append(.genericActionTitle)
        } else if title.localizedCaseInsensitiveContains(action) == false {
            problems.append(.actionTitleNotInTitle)
        }
        return problems
    }
}
