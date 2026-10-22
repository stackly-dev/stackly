extension AccessibilityID {
    /// The Catalog screen (spec 0017).
    public enum Catalog {
        /// The category picker.
        public static let category = "catalog.category"
        /// The card of the provider `id`, such as `catalog.card.official/nats`.
        public static func card(_ id: String) -> String {
            "catalog.card.\(id)"
        }
        /// The version matrix of the detail panel.
        public static let matrix = "catalog.matrix"
        /// The capability list of the detail panel.
        public static let capabilities = "catalog.capabilities"
    }
}
