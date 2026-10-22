import Foundation

extension AccessibilityID {
    /// The New Instance sheet (spec 0020).
    public enum CreateInstance {
        /// The category filter.
        public static let category = "createInstance.category"
        /// The runtime picker.
        public static let runtime = "createInstance.runtime"
        /// The name field.
        public static let name = "createInstance.name"
        /// The Fixed or Automatic picker of an endpoint.
        public static func portMode(_ endpoint: String) -> String {
            "createInstance.port.\(endpoint).mode"
        }
        /// The port field of an endpoint.
        public static func port(_ endpoint: String) -> String {
            "createInstance.port.\(endpoint).value"
        }
        /// The "Use {n}" button of an endpoint.
        public static func usePort(_ endpoint: String) -> String {
            "createInstance.port.\(endpoint).use"
        }
        /// The control of a settings field.
        public static func field(_ key: String) -> String {
            "createInstance.field.\(key)"
        }
        /// The Advanced group.
        public static let advanced = "createInstance.advanced"
        /// The "Start after creating" toggle.
        public static let startAfterCreating = "createInstance.start"
        /// The Preflight card.
        public static let preflight = "createInstance.preflight"
        /// The Retry button of a preflight that could not run.
        public static let preflightRetry = "createInstance.preflight.retry"
        /// The equivalent command.
        public static let command = "createInstance.command"
        /// The progress of the create operation.
        public static let progress = "createInstance.progress"
        /// The banner above the form.
        public static let banner = "createInstance.banner"
    }
}
