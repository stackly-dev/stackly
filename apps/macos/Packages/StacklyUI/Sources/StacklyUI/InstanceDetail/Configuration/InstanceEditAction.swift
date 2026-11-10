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
    /// Edit Ports… (AC-14, AC-19), with the ports to fill in by endpoint name: empty for Edit
    /// Ports… and Change Port…, the suggested port for Use Port of a failure banner (spec 0033,
    /// AC-10).
    case editPorts(InstanceEditSource, prefill: [String: UInt16])

    /// The identity of the sheet: one per kind and instance, whatever it fills in.
    public var id: String {
        switch self {
        case .renaming(let source): "rename:\(source.id.uuidString)"
        case .editPorts(let source, _): "ports:\(source.id.uuidString)"
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
            perform: { present(.editPorts(source, prefill: [:])) })
    }

    /// The port button of a failure banner (spec 0033, AC-10): "Use Port {n}" opens the Ports
    /// sheet with the suggested port filled in, "Change Port…" without a fix opens it empty. The
    /// port changes only when the person confirms the sheet, never from the banner.
    /// - Parameters:
    ///   - source: The instance.
    ///   - fix: The suggested port, or `nil` for Change Port….
    public func portAction(for source: InstanceEditSource, fix: PortFix?) -> StacklyAction {
        let action = fix.map(FailureBannerContent.Action.usePort) ?? .changePort
        let prefill = fix.map { [$0.endpoint: $0.port] } ?? [:]
        return StacklyAction(
            id: fix == nil ? "failure.changePort" : "failure.usePort", title: action.title,
            mutates: true, perform: { present(.editPorts(source, prefill: prefill)) })
    }
}

extension EnvironmentValues {
    /// Opens a Rename or Ports sheet on an instance, or `nil` where the window has none.
    @Entry public var instanceEditAction: InstanceEditAction? = nil
}
