import Foundation

/// A provider is known by its ID, such as `official/nats`, so the Catalog store can keep it
/// in a store keyed by that ID (spec 0017, *Layar Catalog (GUI)*).
extension ProviderSummary: Identifiable {
    /// The provider ID.
    public var id: String { provider }
}
