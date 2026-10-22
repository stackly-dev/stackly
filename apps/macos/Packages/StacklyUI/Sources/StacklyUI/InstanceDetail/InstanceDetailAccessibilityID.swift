import Foundation

extension AccessibilityID {
    /// The instance detail page (spec 0021).
    public enum InstanceDetail {
        /// The page.
        public static let page = "instanceDetail.page"
        /// A card of the Summary tab: `endpoints`, `connection`, `dependents`, or `details`.
        public static func card(_ key: String) -> String {
            "instanceDetail.card.\(key)"
        }
        /// The row that shows the process.
        public static let process = "instanceDetail.process"
        /// The row that shows the readiness.
        public static let readiness = "instanceDetail.readiness"
        /// The entry of the connection string named `name`.
        public static func connection(_ name: String) -> String {
            "instanceDetail.connection.\(name)"
        }
        /// The Recalculate data size button.
        public static let recalculate = "instanceDetail.recalculate"
        /// The picker of tabs.
        public static let tabs = "instanceDetail.tabs"
        /// The placeholder of an instance that was not found.
        public static let notFound = "instanceDetail.notFound"
    }
}
