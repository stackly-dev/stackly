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
    /// The Catalog with a provider selected and its inspector open, such as an entry
    /// Settings › Sources shows (spec 0037).
    case provider(String)
    /// The Runtimes page.
    case runtimes
    /// The Operations page.
    case operations
    /// The Diagnostics page.
    case diagnostics
    /// The Environments list (spec 0044).
    case environments
    /// The detail page of one environment, a child row of Environments in the sidebar
    /// (spec 0044).
    case environment(UUID)

    /// The sidebar section the destination shows.
    public var section: SidebarSection {
        switch self {
        case .overview: .overview
        case .instances, .instance: .instances
        case .catalog, .provider: .catalog
        case .runtimes: .runtimes
        case .operations: .operations
        case .diagnostics: .diagnostics
        case .environments, .environment: .environments
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
    /// The tab the detail page starts on.
    public let tab: InstanceTab
    /// A sheet to open on the detail page, such as the endpoints sheet with a port filled in
    /// (the `change_port` action of the doctor, spec 0034 AC-18), or `nil`.
    public let edit: InstanceEdit?

    init(instance: UUID, tab: InstanceTab = .summary, edit: InstanceEdit? = nil) {
        self.instance = instance
        self.tab = tab
        self.edit = edit
    }
}

/// What the Operations page is asked to open: an operation, selected with its inspector open
/// (spec 0035, *Navigasi*). Every request is new even for the same operation, so the page
/// opens it again.
public struct OperationRequest: Equatable, Sendable {
    /// Tells one request from another.
    public let id = UUID()
    /// The operation to select.
    public let operation: UUID
}

/// What the Catalog page is asked to select (spec 0037). Each request has its own ID, so
/// asking again selects the provider again.
public struct CatalogRequest: Equatable, Sendable {
    /// The request's identity.
    public let id = UUID()
    /// The provider to select.
    public let provider: String
}

/// A request to open Diagnostics with a focus (spec 0034, AC-20). Each request has its own
/// ID, so asking again applies it again.
public struct DiagnosticsRequest: Equatable, Sendable {
    /// The request's identity.
    public let id = UUID()
    /// What the Doctor section shows first, or `nil` for everything.
    public let focus: DiagnosticsFocus?
}

/// The one way a page sends the person to another page of the window (spec 0022, *Navigasi*).
/// ``AppShell`` creates one per window, puts it in the environment, and mirrors
/// ``section`` with its `@SceneStorage` in both directions, since `@SceneStorage` only
/// lives in a view. A destination that is the Instances page also leaves an
/// ``InstancesRequest`` or an ``InstanceRequest`` the page applies, and
/// ``openOperation(_:)`` leaves an ``OperationRequest`` for the Operations page (spec 0035).
/// A request stays until the section changes: SwiftUI can build a page a second time right
/// after the first one applied it (the first visit of a window), and the second page applies
/// it too.
@MainActor
@Observable
public final class ShellNavigator {
    /// The section that shows. Showing another section drops the requests for the
    /// Instances page and the Operations page.
    public var section: SidebarSection {
        didSet {
            if section != .instances {
                instancesRequest = nil
                instanceRequest = nil
            }
            if section != .operations {
                operationRequest = nil
            }
            if section != .diagnostics {
                diagnosticsRequest = nil
            }
            if section != .catalog {
                catalogRequest = nil
            }
            if section != .environments {
                environment = nil
            }
        }
    }
    /// The environment whose detail page shows, while the Environments section shows; `nil`
    /// for the Environments list (spec 0044, *Sidebar*).
    public var environment: UUID?

    /// The sidebar row that shows: a section, or the child row of one environment.
    public var selection: SidebarSelection {
        get {
            if section == .environments, let environment {
                return .environment(environment)
            }
            return .section(section)
        }
        set {
            switch newValue {
            case .section(let newSection):
                section = newSection
                environment = nil
            case .environment(let id):
                section = .environments
                environment = id
            }
        }
    }
    /// The last request for the Instances page, while the Instances section shows.
    public private(set) var instancesRequest: InstancesRequest?
    /// The last detail page asked of the Instances page, while the Instances section shows.
    public private(set) var instanceRequest: InstanceRequest?
    /// The last operation asked of the Operations page, while the Operations section shows.
    public private(set) var operationRequest: OperationRequest?
    /// The last request to open Diagnostics, kept while Diagnostics shows.
    public private(set) var diagnosticsRequest: DiagnosticsRequest?
    /// The last provider asked of the Catalog page, kept while the Catalog shows.
    public private(set) var catalogRequest: CatalogRequest?
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
        case .provider(let id):
            catalogRequest = CatalogRequest(provider: id)
        default:
            break
        }
        section = destination.section
        switch destination {
        case .environment(let id): environment = id
        case .environments: environment = nil
        default: break
        }
    }

    /// Opens the detail page of an environment (spec 0044).
    /// - Parameter id: The environment.
    public func openEnvironment(_ id: UUID) {
        show(.environment(id))
    }

    /// Opens the detail page of an instance: the Instances page applies the request to its
    /// navigation stack, also when it was not showing yet.
    /// - Parameter id: The instance.
    public func openInstance(_ id: UUID) {
        show(.instance(id))
    }

    /// Opens the detail page of an instance on a tab, such as Configuration from the drift
    /// item of Needs Attention (spec 0029, AC-19).
    /// - Parameters:
    ///   - id: The instance.
    ///   - tab: The tab the page starts on.
    public func openInstance(_ id: UUID, tab: InstanceTab) {
        instanceRequest = InstanceRequest(instance: id, tab: tab)
        section = .instances
    }

    /// Opens the Operations page with an operation selected and its inspector open (spec 0035,
    /// AC-13), also an operation that is not on the history page the window loaded.
    /// - Parameter id: The operation.
    public func openOperation(_ id: UUID) {
        operationRequest = OperationRequest(operation: id)
        section = .operations
    }

    /// Opens the detail page of an instance at `tab` with the sheet `edit` open, such as the
    /// endpoints sheet with a suggested port filled in (spec 0034, AC-18). Nothing changes
    /// until the user applies the sheet.
    public func openInstance(_ id: UUID, tab: InstanceTab, edit: InstanceEdit) {
        instanceRequest = InstanceRequest(instance: id, tab: tab, edit: edit)
        section = .instances
    }

    /// Opens Diagnostics, its Doctor section filtered by `focus`, and asks the doctor for a
    /// run (spec 0034, AC-20).
    public func openDiagnostics(focus: DiagnosticsFocus?) {
        diagnosticsRequest = DiagnosticsRequest(focus: focus)
        section = .diagnostics
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
