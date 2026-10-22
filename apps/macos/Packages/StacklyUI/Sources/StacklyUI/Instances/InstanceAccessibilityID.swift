import Foundation

extension AccessibilityID {
    /// The Instances page (spec 0022).
    public enum Instances {
        /// The Status filter.
        public static let filter = "instances.filter"
        /// The table.
        public static let table = "instances.table"
        /// The inspector content.
        public static let inspector = "instances.inspector"
        /// The row of the instance `id`.
        public static func row(_ id: UUID) -> String {
            "instances.row.\(id.uuidString.lowercased())"
        }
    }

    /// The Overview page (spec 0022).
    public enum Overview {
        /// A summary card: `running`, `stopped`, `failed`, or `runtimes`.
        public static func card(_ key: String) -> String {
            "overview.card.\(key)"
        }
        /// The item of Needs Attention with the ID `id`.
        public static func attention(_ id: String) -> String {
            "overview.attention.\(id)"
        }
        /// The Show More or Show Less button of Needs Attention.
        public static let attentionToggle = "overview.attention.toggle"
        /// The row of the instance `id` in the Instances section.
        public static func instance(_ id: UUID) -> String {
            "overview.instance.\(id.uuidString.lowercased())"
        }
        /// The row of the operation `id`.
        public static func operation(_ id: UUID) -> String {
            "overview.operation.\(id.uuidString.lowercased())"
        }
    }
}
