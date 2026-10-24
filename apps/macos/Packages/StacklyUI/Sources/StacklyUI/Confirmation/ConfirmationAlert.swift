import AppKit

/// A button of a confirmation alert.
nonisolated enum ConfirmationButton: Sendable {
    /// The button that runs the action.
    case action
    /// The Cancel button.
    case cancel
}

extension ConfirmationRequest.Role {

    /// The buttons in their native alert order.
    var buttonOrder: [ConfirmationButton] {
        switch self {
        case .destructive: [.cancel, .action]
        case .standard: [.action, .cancel]
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

/// A native alert whose actions are chosen with its buttons.
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
        switch request.role {
        case .destructive:
            cancelButton = alert.addButton(withTitle: "Cancel")
            actionButton = alert.addButton(withTitle: request.actionTitle)
        case .standard:
            actionButton = alert.addButton(withTitle: request.actionTitle)
            cancelButton = alert.addButton(withTitle: "Cancel")
        }
        actionButton.keyEquivalent = ""
        cancelButton.keyEquivalent = ""
        alert.window.defaultButtonCell = nil
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

    /// The request shown now, if any.
    var shownID: UUID? { shown?.id }

    /// Shows `request`, closing any alert still open, and calls `answer` once when the
    /// user presses a button: `true` for the action, `false` for Cancel.
    func show(_ request: ConfirmationRequest, answer: @escaping @MainActor (Bool) -> Void) {
        close()
        let confirmation = ConfirmationAlert(request: request)
        let id = request.id
        shown = (id, confirmation)

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
        answer(shown.alert.isConfirmed(response))
    }

}
