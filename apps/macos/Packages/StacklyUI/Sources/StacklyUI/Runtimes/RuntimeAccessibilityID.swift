import Foundation

extension AccessibilityID {
    /// The Runtimes page and the Get action of the Catalog (spec 0018).
    public enum Runtimes {
        /// The table.
        public static let table = "runtimes.table"
        /// The Installing section.
        public static let installing = "runtimes.installing"
        /// The inspector content.
        public static let inspector = "runtimes.inspector"
        /// The row of the runtime `id`.
        public static func row(_ id: UUID) -> String {
            "runtimes.row.\(id.uuidString.lowercased())"
        }
        /// The Installing row of the operation `id`.
        public static func installingRow(_ id: UUID) -> String {
            "runtimes.installing.\(id.uuidString.lowercased())"
        }
        /// The Get column of the version `version`, such as `runtimes.get.2.11.9`.
        public static func get(_ version: String) -> String {
            "runtimes.get.\(version)"
        }
    }
}
