import AppKit

/// A button of a confirmation alert.
nonisolated enum ConfirmationButton: Sendable {
    /// The button that runs the action.
    case action
    /// The Cancel button.
    case cancel
}

/// The key equivalents a confirmation alert uses.
nonisolated enum ConfirmationKey {
    /// Return: presses the default button.
    static let defaultButton = "\r"
    /// Escape: presses Cancel.
    static let cancel = "\u{1b}"
    /// No key equivalent.
    static let none = ""

    /// Whether a key press is a plain Escape, which always presses Cancel.
    ///
    /// - Parameters:
    ///   - characters: The event's `charactersIgnoringModifiers`.
    ///   - modifiers: The event's modifier flags.
    static func isCancel(characters: String?, modifiers: NSEvent.ModifierFlags) -> Bool {
        let held = modifiers.intersection([.command, .option, .control, .shift])
        return characters == cancel && held.isEmpty
    }
}

extension ConfirmationRequest.Role {
    /// The button Return presses. Escape always presses Cancel.
    var defaultButton: ConfirmationButton {
        switch self {
        case .destructive: .cancel
        case .standard: .action
        }
    }

    /// The buttons in the order the alert adds them: the default button first, in the
    /// default button's place at the trailing edge.
    var buttonOrder: [ConfirmationButton] {
        switch defaultButton {
        case .cancel: [.cancel, .action]
        case .action: [.action, .cancel]
        }
    }

    /// The alert style: `.critical` adds the caution badge to a destructive alert.
    var alertStyle: NSAlert.Style {
        switch self {
        case .destructive: .critical
        case .standard: .informational
        }
    }
}

/// The native alert for one confirmation request, built but not shown.
///
/// On macOS 27 a SwiftUI `confirmationDialog` cannot make Cancel both the default button
/// and the Escape button of a destructive dialog: one button has one key equivalent. So
/// Cancel takes Return as its key equivalent, and ``ConfirmationPresenter`` maps Escape
/// to Cancel while the alert is open. See `apps/macos/design.md` (Confirmation).
struct ConfirmationAlert {
    /// The alert.
    let alert: NSAlert
    /// The button that runs the action.
    let actionButton: NSButton
    /// The Cancel button.
    let cancelButton: NSButton
    /// The role the alert was built for.
    let role: ConfirmationRequest.Role

    /// Builds the alert for `request`.
    init(request: ConfirmationRequest) {
        let alert = NSAlert()
        alert.messageText = request.title
        alert.informativeText = request.message
        alert.alertStyle = request.role.alertStyle
        let actionButton: NSButton
        let cancelButton: NSButton
        switch request.role.defaultButton {
        case .cancel:
            cancelButton = alert.addButton(withTitle: "Cancel")
            actionButton = alert.addButton(withTitle: request.actionTitle)
            cancelButton.keyEquivalent = ConfirmationKey.defaultButton
            actionButton.keyEquivalent = ConfirmationKey.none
        case .action:
            actionButton = alert.addButton(withTitle: request.actionTitle)
            cancelButton = alert.addButton(withTitle: "Cancel")
            actionButton.keyEquivalent = ConfirmationKey.defaultButton
            cancelButton.keyEquivalent = ConfirmationKey.cancel
        }
        actionButton.hasDestructiveAction = request.role == .destructive
        self.alert = alert
        self.actionButton = actionButton
        self.cancelButton = cancelButton
        self.role = request.role
    }

    /// Whether `response` came from the action button.
    func isConfirmed(_ response: NSApplication.ModalResponse) -> Bool {
        guard let index = role.buttonOrder.firstIndex(of: .action) else { return false }
        let first = NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
        return response.rawValue == first + index
    }
}

/// Shows confirmation alerts for one ``SwiftUI/View/stacklyConfirmation(item:)``: as a
/// sheet on the anchor's window, or app modal with `runModal()` when there is no window.
final class ConfirmationPresenter {
    /// A view in the window that owns the confirmation. Set by ``ConfirmationAnchor``.
    weak var anchor: NSView?

    private var shown: (id: UUID, alert: ConfirmationAlert)?
    private var escapeMonitor: Any?

    /// The request shown now, if any.
    var shownID: UUID? { shown?.id }

    /// Shows `request`, closing any alert still open, and calls `answer` once when the
    /// user presses a button: `true` for the action, `false` for Cancel.
    func show(_ request: ConfirmationRequest, answer: @escaping @MainActor (Bool) -> Void) {
        close()
        let confirmation = ConfirmationAlert(request: request)
        let id = request.id
        shown = (id, confirmation)
        if confirmation.role.defaultButton == .cancel {
            watchEscape(for: confirmation)
        }
        if let window = anchor?.window {
            confirmation.alert.beginSheetModal(for: window) { [weak self] response in
                MainActor.assumeIsolated {
                    self?.finish(id: id, response: response, answer: answer)
                }
            }
        } else {
            let response = confirmation.alert.runModal()
            finish(id: id, response: response, answer: answer)
        }
    }

    /// Closes the alert shown now without running its action or calling its `answer`.
    func close() {
        guard let shown else { return }
        self.shown = nil
        stopWatchingEscape()
        let window = shown.alert.alert.window
        if let parent = window.sheetParent {
            parent.endSheet(window, returnCode: .cancel)
        } else if NSApp.modalWindow === window {
            NSApp.stopModal(withCode: .cancel)
        }
    }

    private func finish(
        id: UUID,
        response: NSApplication.ModalResponse,
        answer: @MainActor (Bool) -> Void
    ) {
        guard let shown, shown.id == id else { return }
        self.shown = nil
        stopWatchingEscape()
        answer(shown.alert.isConfirmed(response))
    }

    /// Makes Escape press Cancel while Cancel holds Return as its key equivalent.
    private func watchEscape(for confirmation: ConfirmationAlert) {
        let window = confirmation.alert.window
        let cancel = confirmation.cancelButton
        escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) {
            [weak window, weak cancel] event in
            guard let window, event.window === window,
                ConfirmationKey.isCancel(
                    characters: event.charactersIgnoringModifiers,
                    modifiers: event.modifierFlags
                )
            else { return event }
            cancel?.performClick(nil)
            return nil
        }
    }

    private func stopWatchingEscape() {
        if let escapeMonitor {
            NSEvent.removeMonitor(escapeMonitor)
        }
        escapeMonitor = nil
    }
}
