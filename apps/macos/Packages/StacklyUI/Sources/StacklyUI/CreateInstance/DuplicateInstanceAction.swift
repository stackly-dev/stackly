import SwiftUI

/// The action that opens the New Instance sheet on the settings of an instance (spec 0024,
/// *Titik masuk*). The shell fills it in ``AppShell``; while it is `nil`, no page draws a
/// Duplicate… item.
@MainActor
public struct DuplicateInstanceAction {
    private let action: @MainActor (DuplicateSourceRef) -> Void

    /// Creates the action.
    /// - Parameter action: Opens the sheet on `source`.
    public init(_ action: @escaping @MainActor (DuplicateSourceRef) -> Void) {
        self.action = action
    }

    /// Opens the sheet on the settings of `source`.
    /// - Parameter source: The instance to copy the settings of.
    public func callAsFunction(_ source: DuplicateSourceRef) {
        action(source)
    }

    /// The action as a ``StacklyAction``, for the More menu of the detail page and the
    /// Duplicate… command of the Instance menu. It opens a sheet and asks nothing first;
    /// it is a mutation, so the gate keeps it off while the daemon cannot be reached.
    /// - Parameter source: The instance to copy the settings of.
    public func stacklyAction(for source: DuplicateSourceRef) -> StacklyAction {
        StacklyAction(
            id: CommandID.duplicateInstance.rawValue,
            title: CommandMap.entry(for: .duplicateInstance).title,
            role: .standard(confirmation: nil), mutates: true,
            perform: { action(source) })
    }
}

extension EnvironmentValues {
    /// Opens the New Instance sheet on an instance, or `nil` where the window has no sheet.
    @Entry public var duplicateInstanceAction: DuplicateInstanceAction? = nil
}
