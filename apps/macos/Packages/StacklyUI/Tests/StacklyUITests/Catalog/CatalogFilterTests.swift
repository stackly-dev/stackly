import StacklyClient
import Testing

@testable import StacklyUI

/// The category picker, the search, and the selection of the Catalog screen (spec 0017
/// AC-10).
struct CatalogFilterTests {
    let providers: [ProviderSummary]

    init() throws {
        providers = try #require(CatalogFixture.list().items)
    }

    func ids(_ filter: CatalogFilter) -> [String] {
        filter.apply(to: providers).map(\.provider)
    }

    @Test func allShowsEveryProviderInTheDaemonsOrder() {
        #expect(ids(CatalogFilter()) == providers.map(\.provider))
    }

    @Test func aCategoryShowsOnlyItsProviders() {
        #expect(
            ids(CatalogFilter(category: .messaging))
                == ["official/blockedhost", "official/intelonly", "official/nats"])
        #expect(
            ids(CatalogFilter(category: .database)) == ["official/mysql", "official/postgresql"])
        #expect(ids(CatalogFilter(category: .cache)) == ["official/broken", "official/valkey"])
        #expect(ids(CatalogFilter(category: .proxy)) == ["official/traefik"])
        #expect(ids(CatalogFilter(category: .analytics)) == ["official/clickhouse"])
    }

    @Test func anEntryWithoutACategoryShowsOnlyUnderAll() {
        #expect(ids(CatalogFilter()).contains("official/zenith"))
        for category in CatalogCategory.allCases where category != .all {
            #expect(ids(CatalogFilter(category: category)).contains("official/zenith") == false)
        }
    }

    @Test func aTestingEntryShowsOnlyUnderAll() {
        let testing = ProviderSummary.fixture(
            provider: "testing/fake", displayName: "Fake", category: .testing)
        for category in CatalogCategory.allCases where category != .all {
            #expect(category.contains(testing) == false)
        }
        #expect(CatalogCategory.all.contains(testing))
    }

    @Test func thereIsNoTestingSegment() {
        #expect(
            CatalogCategory.allCases.map(\.title)
                == ["All", "Databases", "Messaging", "Analytics", "Cache", "Proxy"])
    }

    @Test func theSearchMatchesTheNameOrTheIDWithoutCaseOrDiacritics() {
        #expect(ids(CatalogFilter(query: "nats")).contains("official/nats"))
        #expect(ids(CatalogFilter(query: "OFFICIAL/POSTGRESQL")) == ["official/postgresql"])
        #expect(ids(CatalogFilter(query: "clickhouse")) == ["official/clickhouse"])
        #expect(ids(CatalogFilter(query: "  traefik  ")) == ["official/traefik"])
    }

    @Test func theSearchIgnoresDiacriticsOnBothSides() {
        let accented = [
            ProviderSummary.fixture(provider: "official/cafe", displayName: "Café Server"),
            ProviderSummary.fixture(provider: "official/other", displayName: "Other"),
        ]
        #expect(
            CatalogFilter(query: "cafe").apply(to: accented).map(\.provider) == ["official/cafe"])
        #expect(
            CatalogFilter(query: "CAFÉ").apply(to: accented).map(\.provider) == ["official/cafe"])
    }

    @Test func theCategoryAndTheSearchCombine() {
        #expect(ids(CatalogFilter(category: .messaging, query: "intel")) == ["official/intelonly"])
        #expect(ids(CatalogFilter(category: .proxy, query: "nats")).isEmpty)
    }

    @Test func noResultsQuoteTheQueryElseTheCategory() {
        #expect(CatalogFilter(query: " zzz ").description == "zzz")
        #expect(CatalogFilter(category: .cache).description == "Cache")
        #expect(CatalogFilter().isEmpty)
        #expect(CatalogFilter(query: "  ").isEmpty)
        #expect(CatalogFilter(category: .cache).isEmpty == false)
    }

    @Test func theChosenProviderStaysSelectedWhileItIsVisible() {
        #expect(
            CatalogFilter.selection(selected: "official/nats", in: providers) == "official/nats")
    }

    @Test func aFilteredOutSelectionMovesToTheFirstVisibleProvider() {
        let visible = CatalogFilter(category: .database).apply(to: providers)
        #expect(
            CatalogFilter.selection(selected: "official/nats", in: visible) == "official/mysql")
    }

    @Test func noSelectionPicksTheFirstProviderAndNothingVisibleSelectsNothing() {
        #expect(CatalogFilter.selection(selected: nil, in: providers) == providers.first?.provider)
        #expect(CatalogFilter.selection(selected: "official/nats", in: []) == nil)
    }
}
