import SwiftUI

/// What an action needs to run: the mutation gate and a way to show one confirmation
/// (spec 0067, AC-6). One host serves a window or a sheet, so a button and a menu item of
/// the same action are disabled and confirmed the same way, and two dialogs never stack.
///
/// ``SwiftUI/View/actionHost()`` creates it and fills ``SwiftUI/EnvironmentValues/actionHost``.
public struct ActionHost {
    /// The mutation availability of the place the host sits in.
    public let gate: MutationAvailability
    /// Shows a confirmation request.
    public let present: @MainActor (ConfirmationRequest) -> Void

    /// Creates a host.
    ///
    /// - Parameters:
    ///   - gate: The mutation availability.
    ///   - present: Shows a confirmation request.
    init(gate: MutationAvailability, present: @escaping @MainActor (ConfirmationRequest) -> Void) {
        self.gate = gate
        self.present = present
    }

    /// Runs `action` with this host's gate and confirmation.
    public func run(_ action: StacklyAction) {
        action.invoke(gate: gate, present: present)
    }
}

extension EnvironmentValues {
    /// The host that runs actions here, or `nil` where no ``SwiftUI/View/actionHost()``
    /// is above.
    @Entry public var actionHost: ActionHost? = nil
}

extension View {
    /// Holds one confirmation for this window or sheet and fills the `actionHost`
    /// environment value for everything inside.
    ///
    /// It reads ``MutationAvailability`` from the environment where it sits, so put it
    /// inside `.environment(\.mutationAvailability, …)`. The app shell sets it at its root
    /// and every ``StacklySheet`` sets its own, so a confirmation from inside a sheet shows
    /// on top of that sheet.
    public func actionHost() -> some View {
        modifier(ActionHostModifier())
    }
}

private struct ActionHostModifier: ViewModifier {
    @Environment(\.mutationAvailability) private var gate
    @State private var request: ConfirmationRequest?

    func body(content: Content) -> some View {
        let binding = $request
        content
            .environment(
                \.actionHost,
                ActionHost(gate: gate, present: { binding.wrappedValue = $0 })
            )
            .stacklyConfirmation(item: $request)
    }
}
