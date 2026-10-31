import Foundation
import Observation
import SwiftUI

/// Where a page can send the person (spec 0022, *Navigasi*).
public enum ShellDestination: Hashable, Sendable {
    /// The Overview.
    case overview
    /// The Instances page, with a filter and optionally an instance to select.
    case instances(filter: InstanceFilter = .all, select: UUID? = nil)
    /// The detail page of an instance, inside the Instances page (spec 0021).
    case instance(UUID)
    /// The Catalog.
    case catalog
    /// The Runtimes page.
    case runtimes
    /// The Operations page.
    case operations
    /// The Diagnostics page.
    case diagnostics

    /// The sidebar section the destination shows.
    public var section: SidebarSection {
        switch self {
        case .overview: .overview
        case .instances, .instance: .instances
        case .catalog: .catalog
        case .runtimes: .runtimes
        case .operations: .operations
        case .diagnostics: .diagnostics
        }
    }
}

/// What the Instances page is asked to show: the filter, and an instance to select.
/// Every request is new even when its filter and selection equal the last one, so the page
/// applies it again.
public struct InstancesRequest: Equatable, Sendable {
    /// Tells one request from another.
    public let id = UUID()
    /// The filter to set. The search text is cleared.
    public let filter: InstanceFilter
    /// The instance to select and show in the inspector, or `nil`.
    public let select: UUID?
}

/// What the Instances page is asked to open: the detail page of an instance (spec 0021).
/// Every request is new even for the same instance, so the page opens it again.
public struct InstanceRequest: Equatable, Sendable {
    /// Tells one request from another.
    public let id = UUID()
    /// The instance to open.
    public let instance: UUID
}

/// The one way a page sends the person to another page of the window (spec 0022, *Navigasi*).
/// ``AppShell`` creates one per window, puts it in the environment, and mirrors
/// ``section`` with its `@SceneStorage` in both directions, since `@SceneStorage` only
/// lives in a view. A destination that is the Instances page also leaves an
/// ``InstancesRequest`` or an ``InstanceRequest`` the page applies. The request stays until
/// the section changes: SwiftUI can build the Instances page a second time right after the
/// first one applied it (the first visit of a window), and the second page applies it too.
@MainActor
@Observable
public final class ShellNavigator {
    /// The section that shows. Showing another section drops the requests for the
    /// Instances page.
    public var section: SidebarSection {
        didSet {
            if section != .instances {
                instancesRequest = nil
                instanceRequest = nil
            }
        }
    }
    /// The last request for the Instances page, while the Instances section shows.
    public private(set) var instancesRequest: InstancesRequest?
    /// The last detail page asked of the Instances page, while the Instances section shows.
    public private(set) var instanceRequest: InstanceRequest?
    /// Counts the times the person asked for the table of Instances again, such as a second
    /// click on its sidebar row: the page closes the detail page when it changes.
    public private(set) var instancesResets = 0

    /// Creates a navigator.
    /// - Parameter section: The section that shows first.
    public init(section: SidebarSection = .overview) {
        self.section = section
    }

    /// Shows a destination.
    public func show(_ destination: ShellDestination) {
        switch destination {
        case .instances(let filter, let select):
            instancesRequest = InstancesRequest(filter: filter, select: select)
        case .instance(let id):
            instanceRequest = InstanceRequest(instance: id)
        default:
            break
        }
        section = destination.section
    }

    /// Opens the detail page of an instance: the Instances page applies the request to its
    /// navigation stack, also when it was not showing yet.
    /// - Parameter id: The instance.
    public func openInstance(_ id: UUID) {
        show(.instance(id))
    }

    /// Asks the Instances page to go back to its table.
    public func showInstancesTable() {
        instancesResets += 1
    }
}

extension EnvironmentValues {
    /// The navigator of this window, or `nil` where no shell sets one (a preview or a
    /// snapshot of a page alone): then a link does nothing.
    @Entry public var shellNavigator: ShellNavigator? = nil
}

/// The action that opens the New Instance sheet (spec 0022, *Titik sambung New Instance*).
/// Feature 20 fills it in ``AppShell``; while it is `nil`, no page draws a New Instance
/// button.
@MainActor
public struct NewInstanceAction {
    private let action: @MainActor () -> Void

    /// Creates the action.
    /// - Parameter action: Opens the sheet.
    public init(_ action: @escaping @MainActor () -> Void) {
        self.action = action
    }

    /// Opens the sheet.
    public func callAsFunction() {
        action()
    }

    /// The action as a ``StacklyAction``, for a toolbar button and an empty state.
    /// - Parameter title: "New Instance" in the toolbar, "New Instance…" in an empty state.
    public func stacklyAction(title: String) -> StacklyAction {
        StacklyAction(
            id: CommandID.newInstance.rawValue, title: title, mutates: true,
            perform: { action() })
    }
}

extension EnvironmentValues {
    /// Opens the New Instance sheet, or `nil` until feature 20 builds it.
    @Entry public var newInstanceAction: NewInstanceAction? = nil
}
