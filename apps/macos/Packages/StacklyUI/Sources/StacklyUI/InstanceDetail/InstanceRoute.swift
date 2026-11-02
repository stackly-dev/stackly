import Foundation

/// A place the Instances page can push onto its navigation stack (spec 0021, *Halaman detail
/// dan navigasi*): the detail page of one instance.
nonisolated public struct InstanceRoute: Hashable, Sendable {
    /// The instance.
    public let id: UUID

    /// Creates the route of an instance.
    /// - Parameter id: The instance.
    public init(_ id: UUID) {
        self.id = id
    }
}

/// A tab of the detail page; the picker shows when there is more than one (spec 0021,
/// *Halaman detail dan navigasi*; Logs from spec 0027, Metrics from spec 0028).
nonisolated public enum InstanceTab: String, CaseIterable, Hashable, Identifiable, Sendable {
    /// Endpoints, connection, dependents, and details.
    case summary
    /// The captured stdout and stderr of the instance.
    case logs
    /// CPU, memory, disk, restarts, and the processes of the running instance (spec 0028).
    case metrics

    /// The tab's identity.
    public var id: String { rawValue }

    /// The title in the picker.
    public var title: String {
        switch self {
        case .summary: "Summary"
        case .logs: "Logs"
        case .metrics: "Metrics"
        }
    }
}
