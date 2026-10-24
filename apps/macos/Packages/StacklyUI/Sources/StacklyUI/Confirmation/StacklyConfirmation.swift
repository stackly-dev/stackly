import AppKit
import SwiftUI

extension View {
    /// Presents the confirmation dialog for `item` while it is not `nil`: the only way
    /// Stackly asks the user to confirm an action.
    /// The dialog is the native alert, as a sheet on this view's window (app modal when
    /// the view has no window). A destructive request gets a red action button, the
    /// critical style, and Cancel as the default button, so Return and Escape both
    /// cancel. A standard request makes its action the default button, and Escape
    /// cancels. When the user answers, the dialog sets `item` back to `nil`, then runs
    /// ``ConfirmationRequest/perform`` only if the user pressed the action button. A new
    /// request replaces the shown one; there is only one confirmation at a time. Setting
    /// `item` to `nil` closes the dialog without running the action.
    /// - Parameter item: The request to confirm, or `nil` when there is none.
    public func stacklyConfirmation(item: Binding<ConfirmationRequest?>) -> some View {
        modifier(StacklyConfirmationModifier(request: item))
    }
}

/// The modifier behind ``SwiftUI/View/stacklyConfirmation(item:)``.
struct StacklyConfirmationModifier: ViewModifier {
    @Binding var request: ConfirmationRequest?
    @State private var presenter = ConfirmationPresenter()

    func body(content: Content) -> some View {
        content
            .background { ConfirmationAnchor(presenter: presenter) }
            .onChange(of: request?.id, initial: true) {
                sync()
            }
            .onDisappear {
                presenter.close()
                request = nil
            }
    }

    /// Shows the current request, or closes the alert when there is none. It runs after
    /// the view update, once the anchor is in its window.
    private func sync() {
        let binding = $request
        let presenter = presenter
        Task {
            guard let current = binding.wrappedValue else {
                presenter.close()
                return
            }
            guard presenter.shownID != current.id else { return }
            presenter.show(current) { confirmed in
                if binding.wrappedValue?.id == current.id {
                    binding.wrappedValue = nil
                }
                if confirmed {
                    current.perform()
                }
            }
        }
    }
}

/// An empty view that tells ``ConfirmationPresenter`` which window to put the sheet on.
struct ConfirmationAnchor: NSViewRepresentable {
    let presenter: ConfirmationPresenter

    func makeNSView(context: Context) -> AnchorView {
        let view = AnchorView()
        presenter.anchor = view
        return view
    }

    func updateNSView(_ view: AnchorView, context: Context) {
        presenter.anchor = view
    }

    final class AnchorView: NSView {
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func isAccessibilityElement() -> Bool { false }
    }
}
