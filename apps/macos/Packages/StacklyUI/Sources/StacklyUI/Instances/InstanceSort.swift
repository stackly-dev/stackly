import Foundation

/// A column the Instances table can be sorted by (spec 0022, *Halaman Instances*). The
/// action column has no key.
nonisolated public enum InstanceSortKey: String, CaseIterable, Hashable, Sendable {
    /// By name.
    case name
    /// By the provider's display name.
    case provider
    /// By the exact version.
    case version
    /// By the first port.
    case endpoints
    /// By the group: failed, unknown, not ready, ready, stopped.
    case status
}

/// The comparator of one table column. The direction only reverses the column's own
/// comparison: a tie always falls back to the name and then the ID, both ascending, and a
/// missing value (no version, a socket or automatic port) is always last in both directions.
nonisolated public struct InstanceSortComparator: SortComparator, Hashable, Sendable {
    /// The column.
    public var key: InstanceSortKey
    /// The direction.
    public var order: SortOrder

    /// Creates a comparator.
    /// - Parameters:
    ///   - key: The column.
    ///   - order: The direction; ascending by default.
    public init(_ key: InstanceSortKey, order: SortOrder = .forward) {
        self.key = key
        self.order = order
    }

    /// The default order of the page: by name, ascending.
    public static var `default`: InstanceSortComparator {
        InstanceSortComparator(.name)
    }

    /// Compares two rows.
    public func compare(_ lhs: InstanceRowModel, _ rhs: InstanceRowModel) -> ComparisonResult {
        let primary = primaryResult(lhs, rhs)
        if primary != .orderedSame { return primary }
        return Self.tieBreak(lhs, rhs)
    }

    private func primaryResult(_ lhs: InstanceRowModel, _ rhs: InstanceRowModel) -> ComparisonResult
    {
        switch key {
        case .name:
            return directed(lhs.name.localizedStandardCompare(rhs.name))
        case .provider:
            return directed(lhs.providerTitle.localizedStandardCompare(rhs.providerTitle))
        case .version:
            return optional(lhs.version, rhs.version) { $0.localizedStandardCompare($1) }
        case .endpoints:
            return optional(
                InstanceText.firstPort(lhs.summary.endpoints),
                InstanceText.firstPort(rhs.summary.endpoints)
            ) { $0 == $1 ? .orderedSame : ($0 < $1 ? .orderedAscending : .orderedDescending) }
        case .status:
            let left = lhs.group?.statusRank ?? Int.max
            let right = rhs.group?.statusRank ?? Int.max
            return directed(
                left == right
                    ? .orderedSame : (left < right ? .orderedAscending : .orderedDescending))
        }
    }

    /// `result` reversed when the order is reverse.
    private func directed(_ result: ComparisonResult) -> ComparisonResult {
        guard order == .reverse else { return result }
        switch result {
        case .orderedAscending: return .orderedDescending
        case .orderedDescending: return .orderedAscending
        case .orderedSame: return .orderedSame
        }
    }

    /// Compares two values that may be missing; a missing one is last in both directions.
    private func optional<Value>(
        _ lhs: Value?, _ rhs: Value?, _ compare: (Value, Value) -> ComparisonResult
    ) -> ComparisonResult {
        switch (lhs, rhs) {
        case (nil, nil): return .orderedSame
        case (nil, _?): return .orderedDescending
        case (_?, nil): return .orderedAscending
        case (let left?, let right?): return directed(compare(left, right))
        }
    }

    /// The name, then the ID, both ascending.
    static func tieBreak(_ lhs: InstanceRowModel, _ rhs: InstanceRowModel) -> ComparisonResult {
        switch lhs.name.localizedStandardCompare(rhs.name) {
        case .orderedSame:
            if lhs.id.uuidString == rhs.id.uuidString { return .orderedSame }
            return lhs.id.uuidString < rhs.id.uuidString ? .orderedAscending : .orderedDescending
        case let other:
            return other
        }
    }
}

nonisolated extension InstanceRowModel {
    /// The rows in the order of `comparator`.
    public static func sorted(_ rows: [InstanceRowModel], by comparator: InstanceSortComparator)
        -> [InstanceRowModel]
    {
        rows.sorted(using: comparator)
    }
}
