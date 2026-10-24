import Foundation

/// A top level section of the main window's sidebar.
/// The raw value is what scene restoration stores, so it never changes.
nonisolated public enum SidebarSection: String, CaseIterable, Hashable, Identifiable, Sendable {
    /// Overview.
    case overview
    /// Instances.
    case instances
    /// Environments.
    case environments
    /// Catalog, in the Library group.
    case catalog
    /// Runtimes, in the Library group.
    case runtimes
    /// Operations, in the System group.
    case operations
    /// Diagnostics, in the System group.
    case diagnostics

    /// The section's identity: itself.
    public var id: Self { self }

    /// The title in the sidebar and the window toolbar.
    public var title: String {
        switch self {
        case .overview: "Overview"
        case .instances: "Instances"
        case .environments: "Environments"
        case .catalog: "Catalog"
        case .runtimes: "Runtimes"
        case .operations: "Operations"
        case .diagnostics: "Diagnostics"
        }
    }

    /// The section's symbol and color.
    public var style: SectionStyle {
        switch self {
        case .overview: .overview
        case .instances: .instances
        case .environments: .environments
        case .catalog: .catalog
        case .runtimes: .runtimes
        case .operations: .operations
        case .diagnostics: .diagnostics
        }
    }

    /// The group the section belongs to.
    public var group: SidebarGroup {
        switch self {
        case .overview, .instances, .environments: .main
        case .catalog, .runtimes: .library
        case .operations, .diagnostics: .system
        }
    }

    /// The badge of the section, or `nil` when it has none.
    /// Operations shows the running operations only while connected. A count of zero, a count that never loaded, and every other section show no badge.
    /// - Parameters:
    ///   - counts: The counts from the stores.
    ///   - isConnected: Whether the daemon is connected and every store refreshed.
    public func badge(counts: SidebarCounts, isConnected: Bool) -> SidebarBadge? {
        switch self {
        case .instances:
            guard let count = counts.instances, count > 0 else { return nil }
            return SidebarBadge(
                count: count,

                isProminent: false
            )
        case .operations:
            guard isConnected, let count = counts.runningOperations, count > 0 else {
                return nil
            }
            return SidebarBadge(count: count, isProminent: true)
        case .overview, .environments, .catalog, .runtimes, .diagnostics:
            return nil
        }
    }

    /// "1 instance" or "<n> instances".
    static func countText(_ count: Int, singular: String, plural: String) -> String {
        "\(count) \(count == 1 ? singular : plural)"
    }
}

/// A group of sidebar sections: the main sections without a title, then Library and
/// System.
nonisolated public enum SidebarGroup: CaseIterable, Hashable, Identifiable, Sendable {
    /// Overview, Instances, and Environments, without a title.
    case main
    /// Catalog and Runtimes.
    case library
    /// Operations and Diagnostics.
    case system

    /// The group's identity: itself.
    public var id: Self { self }

    /// The group title, or `nil` for the main group.
    public var title: String? {
        switch self {
        case .main: nil
        case .library: "Library"
        case .system: "System"
        }
    }

    /// The sections of the group, in sidebar order.
    public var sections: [SidebarSection] {
        SidebarSection.allCases.filter { $0.group == self }
    }
}

/// The counts behind the sidebar badges. `nil` means the store never loaded a snapshot,
/// so there is no number to show.
nonisolated public struct SidebarCounts: Hashable, Sendable {
    /// The instance count of the last loaded snapshot.
    public var instances: Int?
    /// The operations that are running or cancelling.
    public var runningOperations: Int?

    /// Creates the counts.
    public init(instances: Int? = nil, runningOperations: Int? = nil) {
        self.instances = instances
        self.runningOperations = runningOperations
    }
}

/// The native count badge and its prominence.
nonisolated public struct SidebarBadge: Hashable, Sendable {
    /// The number on the badge, never zero.
    public let count: Int
    /// Whether the badge uses the increased prominence (Operations).
    public let isProminent: Bool
}
