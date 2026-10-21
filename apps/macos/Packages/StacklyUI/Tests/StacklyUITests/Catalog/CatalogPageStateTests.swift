import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The state of the Catalog page: the fixed order of `ViewState.resolve` with the copy of
/// spec 0017 (AC-8, AC-10).
@MainActor
struct CatalogPageStateTests {
    let providers: [ProviderSummary]
    let info: CatalogInfo

    init() throws {
        let list = try CatalogFixture.list()
        providers = try #require(list.items)
        info = list.catalog
    }

    func state(
        providers: [ProviderSummary]? = nil,
        info: CatalogInfo? = nil,
        filter: CatalogFilter = CatalogFilter(),
        error: ErrorBody? = nil,
        unsupported: Bool = false,
        waiting: FailureContent? = nil
    ) -> ViewState<[ProviderSummary]> {
        CatalogPage.state(
            providers: providers ?? self.providers, info: info ?? self.info, filter: filter,
            error: error, unsupported: unsupported, waiting: waiting,
            loadedAt: Date(timeIntervalSince1970: 100), isRefreshing: false, clearFilters: {})
    }

    func loading(waiting: FailureContent? = nil, error: ErrorBody? = nil) -> ViewState<
        [ProviderSummary]
    > {
        CatalogPage.state(
            providers: nil, info: nil, filter: CatalogFilter(), error: error, unsupported: false,
            waiting: waiting, loadedAt: nil, isRefreshing: false, clearFilters: {})
    }

    @Test func beforeTheFirstSnapshotThePageIsLoading() {
        #expect(loading().shape == .loading)
    }

    @Test func anOlderDaemonShowsTheUnsupportedPage() {
        guard case .unsupported(let content) = state(unsupported: true) else {
            Issue.record("expected unsupported")
            return
        }
        #expect(content.title == "Catalog Unavailable")
        #expect(
            content.reason
                == "This version of stacklyd doesn’t provide the catalog. Update Stackly.")
    }

    @Test func aFailedFirstLoadShowsTheDaemonsMessageAndCode() {
        let error = ErrorBody(
            code: "catalog.unavailable", category: .unavailable,
            message: "the provider catalog is not available", retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
        guard case .failed(let failure) = loading(error: error) else {
            Issue.record("expected failed")
            return
        }
        #expect(failure.title == "Catalog Unavailable")
        #expect(failure.message == "the provider catalog is not available")
        #expect(failure.code == "catalog.unavailable")
    }

    @Test func aDaemonThatCannotBeReachedShowsTheWindowsWaitingFailure() {
        let waiting = FailureContent(title: "Waiting for stacklyd", message: "…")
        guard case .failed(let failure) = loading(waiting: waiting) else {
            Issue.record("expected failed")
            return
        }
        #expect(failure == waiting)
    }

    @Test func aLoadedCatalogShowsItsProvidersWithAPartialNoteForInvalidOnes() {
        guard case .loaded(let value, let notes) = state() else {
            Issue.record("expected loaded")
            return
        }
        #expect(value.count == providers.count)
        #expect(notes.partial?.summary == "2 providers failed validation.")
        #expect(notes.partial?.items.map(\.name) == ["official/broken", "official/zenith"])
        #expect(notes.stale == nil)
    }

    @Test func oneInvalidProviderIsWrittenInTheSingular() {
        let one = CatalogInfo(
            revision: info.revision, publishedAt: info.publishedAt, loadedAt: info.loadedAt,
            contentSha256: info.contentSha256, source: info.source, trust: info.trust,
            host: info.host, outdated: false, invalidCount: 1)
        guard case .loaded(_, let notes) = state(info: one) else {
            Issue.record("expected loaded")
            return
        }
        #expect(notes.partial?.summary == "1 provider failed validation.")
    }

    @Test func aCatalogWithoutInvalidProvidersHasNoPartialNote() {
        let clean = CatalogInfo(
            revision: info.revision, publishedAt: info.publishedAt, loadedAt: info.loadedAt,
            contentSha256: info.contentSha256, source: info.source, trust: info.trust,
            host: info.host, outdated: false, invalidCount: 0)
        guard case .loaded(_, let notes) = state(info: clean) else {
            Issue.record("expected loaded")
            return
        }
        #expect(notes.partial == nil)
    }

    @Test func anOutdatedCatalogAddsTheStaleNoteWithTheDateItWasPublished() {
        let old = CatalogInfo(
            revision: info.revision, publishedAt: info.publishedAt, loadedAt: info.loadedAt,
            contentSha256: info.contentSha256, source: .development, trust: .development,
            host: info.host, outdated: true, invalidCount: 0)
        guard case .loaded(_, let notes) = state(info: old) else {
            Issue.record("expected loaded")
            return
        }
        #expect(notes.stale?.since == info.publishedAt)
        #expect(notes.stale?.reason == .outdated(source: "the development catalog"))
    }

    @Test func aFreshCatalogHasNoStaleNote() {
        guard case .loaded(_, let notes) = state() else {
            Issue.record("expected loaded")
            return
        }
        #expect(notes.stale == nil)
    }

    @Test func anEmptyCatalogSaysSo() {
        guard case .empty(let empty, _) = state(providers: []) else {
            Issue.record("expected empty")
            return
        }
        #expect(empty.kind == .noData)
        #expect(empty.title == "No Providers")
        #expect(empty.message == "This catalog has no providers.")
    }

    @Test func aSearchWithoutResultsQuotesTheQueryAndOffersClearFilters() {
        guard case .empty(let empty, _) = state(filter: CatalogFilter(query: "zzz")) else {
            Issue.record("expected empty")
            return
        }
        #expect(empty.kind == .noResults(query: "zzz"))
        #expect(empty.message == "No providers match “zzz”.")
        #expect(empty.primaryAction?.title == "Clear Filters")
    }

    @Test func aCategoryWithoutProvidersQuotesTheCategory() {
        let onlyDatabases = providers.filter { $0.category == .database }
        guard
            case .empty(let empty, _) = state(
                providers: onlyDatabases, filter: CatalogFilter(category: .proxy))
        else {
            Issue.record("expected empty")
            return
        }
        #expect(empty.kind == .noResults(query: "Proxy"))
        #expect(empty.message == "No providers match “Proxy”.")
    }

    @Test func aFilterThatLeavesProvidersShowsThem() {
        guard case .loaded = state(filter: CatalogFilter(query: "nats")) else {
            Issue.record("expected loaded")
            return
        }
    }
}
