import Foundation

/// A place the Instances page can push onto its navigation stack (spec 0021, *Halaman detail
/// dan navigasi*): the detail page of one instance.
nonisolated public struct InstanceRoute: Hashable, Sendable {
    /// The instance.
    public let id: UUID

    /// Creates the route of an instance.
    ///
    /// - Parameter id: The instance.
    public init(_ id: UUID) {
        self.id = id
    }
}

/// A tab of the detail page. Summary is the only one built; the picker shows when there is
/// more than one (spec 0021, *Halaman detail dan navigasi*).
nonisolated public enum InstanceTab: String, CaseIterable, Hashable, Identifiable, Sendable {
    /// Endpoints, connection, dependents, and details.
    case summary

    /// The tab's identity.
    public var id: String { rawValue }

    /// The title in the picker.
    public var title: String {
        switch self {
        case .summary: "Summary"
        }
    }
}
