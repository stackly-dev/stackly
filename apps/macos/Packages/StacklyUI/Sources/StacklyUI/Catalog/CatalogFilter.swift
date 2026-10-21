import Foundation
import StacklyClient

/// A segment of the category picker: All, then one segment per category a user can browse.
/// There is no Testing segment.
nonisolated public enum CatalogCategory: String, CaseIterable, Identifiable, Hashable, Sendable {
    /// Every provider, also those with a category this build does not know.
    case all
    /// Databases.
    case database
    /// Messaging.
    case messaging
    /// Analytics.
    case analytics
    /// Cache.
    case cache
    /// Proxy.
    case proxy

    /// The segment's identity: itself.
    public var id: Self { self }

    /// The segment's title: "All", "Databases", "Messaging", and so on.
    public var title: String {
        switch self {
        case .all: "All"
        case .database: "Databases"
        case .messaging: "Messaging"
        case .analytics: "Analytics"
        case .cache: "Cache"
        case .proxy: "Proxy"
        }
    }

    /// Whether `provider` belongs to the segment. An entry of the `testing` category, one
    /// with no category, and one with a category this build does not know show only in All.
    func contains(_ provider: ProviderSummary) -> Bool {
        switch self {
        case .all: true
        case .database: provider.category == .database
        case .messaging: provider.category == .messaging
        case .analytics: provider.category == .analytics
        case .cache: provider.category == .cache
        case .proxy: provider.category == .proxy
        }
    }
}

/// What the Catalog screen filters by: the category segment and the search text.
///
/// Neither is saved: a new window starts with All and an empty field.
nonisolated public struct CatalogFilter: Equatable, Sendable {
    /// The chosen segment.
    public var category: CatalogCategory
    /// The search text as typed.
    public var query: String

    /// Creates a filter.
    ///
    /// - Parameters:
    ///   - category: The chosen segment, All by default.
    ///   - query: The search text, empty by default.
    public init(category: CatalogCategory = .all, query: String = "") {
        self.category = category
        self.query = query
    }

    /// Whether the filter lets every provider through.
    public var isEmpty: Bool {
        category == .all && trimmedQuery.isEmpty
    }

    /// What the "no results" state quotes: the search text, else the segment's title.
    public var description: String {
        trimmedQuery.isEmpty ? category.title : trimmedQuery
    }

    private var trimmedQuery: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// The providers that pass, in the order given (the daemon's display order).
    ///
    /// The search matches the display name or the ID, ignoring case and diacritics.
    ///
    /// - Parameter providers: The providers to filter.
    public func apply(to providers: [ProviderSummary]) -> [ProviderSummary] {
        let needle = trimmedQuery
        return providers.filter { provider in
            category.contains(provider)
                && (needle.isEmpty
                    || provider.displayName.range(
                        of: needle, options: [.caseInsensitive, .diacriticInsensitive]) != nil
                    || provider.provider.range(
                        of: needle, options: [.caseInsensitive, .diacriticInsensitive]) != nil)
        }
    }

    /// The provider the page selects: `selected` while it is visible, else the first
    /// visible one, else `nil`.
    ///
    /// - Parameters:
    ///   - selected: The ID the user chose last, or `nil`.
    ///   - visible: The providers that pass the filter.
    public static func selection(selected: String?, in visible: [ProviderSummary]) -> String? {
        if let selected, visible.contains(where: { $0.provider == selected }) {
            return selected
        }
        return visible.first?.provider
    }
}
