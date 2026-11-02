import Foundation
import SwiftUI

/// The instance a Rename or Ports sheet works on.
nonisolated public struct InstanceEditSource: Hashable, Sendable {
    /// The instance.
    public let id: UUID
    /// Its name now.
    public let name: String

    /// Creates a source.
    public init(id: UUID, name: String) {
        self.id = id
        self.name = name
    }
}

/// A sheet that changes an instance that already exists (spec 0029, *GUI*).
nonisolated public enum InstanceEdit: Identifiable, Hashable, Sendable {
    /// Rename… (AC-13, AC-19).
    case renaming(InstanceEditSource)
    /// Edit Ports… (AC-14, AC-19).
    case editPorts(InstanceEditSource)

    /// The identity of the sheet: one per kind and instance.
    public var id: String {
        switch self {
        case .renaming(let source): "rename:\(source.id.uuidString)"
        case .editPorts(let source): "ports:\(source.id.uuidString)"
        }
    }
}

/// The action that opens a Rename or Ports sheet on an instance, the way
/// ``DuplicateInstanceAction`` opens the New Instance sheet. The Instances page fills it in
/// and hosts the sheets; while it is `nil`, no page draws the items.
@MainActor
public struct InstanceEditAction {
    private let present: @MainActor (InstanceEdit) -> Void

    /// Creates the action.
    /// - Parameter present: Opens the sheet.
    public init(_ present: @escaping @MainActor (InstanceEdit) -> Void) {
        self.present = present
    }

    /// Rename… as a ``StacklyAction``, for the More menu, the Instance menu, and the context
    /// menu of a row. It opens a sheet and asks nothing first; the gate keeps it off while the
    /// daemon cannot be reached.
    /// - Parameter source: The instance.
    public func renameAction(for source: InstanceEditSource) -> StacklyAction {
        StacklyAction(
            id: CommandID.renameInstance.rawValue,
            title: CommandMap.entry(for: .renameInstance).title, mutates: true,
            perform: { present(.renaming(source)) })
    }

    /// Edit Ports… as a ``StacklyAction``.
    /// - Parameter source: The instance.
    public func editPortsAction(for source: InstanceEditSource) -> StacklyAction {
        StacklyAction(
            id: CommandID.editPorts.rawValue,
            title: CommandMap.entry(for: .editPorts).title, mutates: true,
            perform: { present(.editPorts(source)) })
    }
}

extension EnvironmentValues {
    /// Opens a Rename or Ports sheet on an instance, or `nil` where the window has none.
    @Entry public var instanceEditAction: InstanceEditAction? = nil
}
