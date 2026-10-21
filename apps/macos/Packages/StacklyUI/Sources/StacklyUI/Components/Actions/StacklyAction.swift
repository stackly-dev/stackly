import SwiftUI

/// The one description of an action that shows as a button, a menu item, or a command
/// (spec 0067, AC-6). A screen builds it once; ``ActionButton``, ``ActionMenu``,
/// ``ActionMenuItems``, and the commands of feature 69 all read it, so the same action is
/// disabled and confirmed the same way everywhere.
public struct StacklyAction: Identifiable {
    /// A stable key for the action: the `<id>` of the identifier `action.button.<id>`.
    public var id: String
    /// The title. It ends with "…" when the role carries a confirmation.
    public var title: String
    /// The SF Symbol, or `nil` for a text only button.
    public var symbol: String?
    /// The kind of action and its confirmation.
    public var role: ActionRole
    /// Whether the action changes data through the daemon, so the mutation gate disables
    /// it while the daemon cannot be reached.
    public var mutates: Bool
    /// Why the action cannot run now for a reason of its own (a capability the provider
    /// lacks, a state, a failed check), or `nil`.
    public var disabledReason: String?
    /// Whether the action is running now.
    public var isBusy: Bool
    /// The work. It runs after the confirmation, when there is one.
    public var perform: @MainActor () -> Void

    /// Creates an action.
    ///
    /// - Parameters:
    ///   - id: A stable key.
    ///   - title: The title; end it with "…" when the role confirms.
    ///   - symbol: The SF Symbol, or `nil`.
    ///   - role: The kind of action; ordinary without a confirmation by default.
    ///   - mutates: Whether the action changes data through the daemon.
    ///   - disabledReason: The action's own reason it cannot run, or `nil`.
    ///   - isBusy: Whether the action is running.
    ///   - perform: The work.
    public init(
        id: String,
        title: String,
        symbol: String? = nil,
        role: ActionRole = .standard(confirmation: nil),
        mutates: Bool,
        disabledReason: String? = nil,
        isBusy: Bool = false,
        perform: @escaping @MainActor () -> Void
    ) {
        self.id = id
        self.title = title
        self.symbol = symbol
        self.role = role
        self.mutates = mutates
        self.disabledReason = disabledReason
        self.isBusy = isBusy
        self.perform = perform
        assert(
            title.hasSuffix("…") || role.confirmation == nil,
            "the title of an action that confirms ends with … (macOS marks a step that follows)"
        )
    }

    /// The confirmation the role carries, or `nil`.
    public var confirmation: ActionConfirmation? {
        role.confirmation
    }

    /// Whether the action can run now, in the order of how specific the reason is: the
    /// action's own reason first (it must be visible), then the mutation gate (the window
    /// banner already writes it), then busy.
    ///
    /// - Parameter gate: The mutation availability of the window.
    public func availability(gate: MutationAvailability) -> ActionAvailability {
        if let disabledReason {
            return .disabled(reason: disabledReason, showsReason: true)
        }
        if mutates, let reason = gate.blockedReason {
            return .disabled(reason: reason, showsReason: false)
        }
        return isBusy ? .busy : .enabled
    }

    /// Runs the action: nothing when it cannot run, ``present`` with the confirmation
    /// request when the role carries one, ``perform`` otherwise.
    ///
    /// - Parameters:
    ///   - gate: The mutation availability of the window.
    ///   - present: Shows a confirmation; it runs ``perform`` only when the user confirms.
    public func invoke(gate: MutationAvailability, present: (ConfirmationRequest) -> Void) {
        guard availability(gate: gate) == .enabled else { return }
        guard let confirmation else {
            perform()
            return
        }
        present(
            ConfirmationRequest(
                title: confirmation.title,
                message: confirmation.message,
                actionTitle: confirmation.actionTitle,
                role: role.isDestructive ? .destructive : .standard,
                perform: perform
            )
        )
    }
}

extension StacklyAction {
    /// Runs the action the way a button or a menu item does: through `host` when there is
    /// one. Without a host an action with no confirmation still runs, with the window's
    /// `gate`; one that confirms asserts and runs nothing, because it would otherwise run
    /// without its dialog.
    func run(host: ActionHost?, gate: MutationAvailability) {
        if let host {
            host.run(self)
        } else if confirmation != nil {
            assertionFailure("an action that confirms needs .actionHost()")
        } else {
            invoke(gate: gate, present: { _ in })
        }
    }
}

extension ActionRole {
    /// The confirmation either case carries.
    var confirmation: ActionConfirmation? {
        switch self {
        case .standard(let confirmation): confirmation
        case .destructive(let confirmation): confirmation
        }
    }

    /// Whether the role is destructive.
    var isDestructive: Bool {
        if case .destructive = self { true } else { false }
    }
}
