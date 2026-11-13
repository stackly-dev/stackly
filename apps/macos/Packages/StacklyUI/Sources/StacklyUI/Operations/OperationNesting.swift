import Foundation
import StacklyClient

/// The order of the Operations list with child operations (spec 0046, *Activity dan
/// Overview*): each child follows its coordinator, indented, newest first among its siblings.
/// A child whose coordinator is not in the list shows on its own.
nonisolated enum OperationNesting {
    /// One row of the list.
    struct Row: Identifiable, Equatable {
        /// The operation.
        let operation: OperationSummary
        /// Whether it shows under its coordinator.
        let isChild: Bool

        /// The operation ID.
        var id: UUID { operation.id }
    }

    /// The rows of `operations`, which come newest first.
    static func rows(_ operations: [OperationSummary]) -> [Row] {
        let ids = Set(operations.map(\.id))
        var children: [UUID: [OperationSummary]] = [:]
        for operation in operations {
            if let parent = operation.parent, ids.contains(parent), parent != operation.id {
                children[parent, default: []].append(operation)
            }
        }
        var rows: [Row] = []
        for operation in operations {
            if let parent = operation.parent, ids.contains(parent), parent != operation.id {
                continue
            }
            rows.append(Row(operation: operation, isChild: false))
            for child in children[operation.id] ?? [] {
                rows.append(Row(operation: child, isChild: true))
            }
        }
        return rows
    }
}
