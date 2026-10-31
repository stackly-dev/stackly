import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The catalog store and the detail model of spec 0017 (AC-9, AC-10): one snapshot, the
/// daemon's order, an older daemon, and the detail cache.
@Suite(.timeLimit(.minutes(1)))
@MainActor
struct CatalogStoreTests {
    let script: CatalogScript

    init() throws {
        script = try CatalogScript.fixture()
    }

    @Test func nothingShowsBeforeTheFirstSnapshot() {
        let store = script.store()
        #expect(store.providers == nil)
        #expect(store.info == nil)
        #expect(store.subtitle() == nil)
    }

    @Test func aSnapshotKeepsTheDaemonsOrderAndTheCatalogInfo() async throws {
        let store = script.store()
        await store.store.resnapshot()
        let expected = try #require(CatalogFixture.list().items).map(\.provider)
        #expect(store.providers?.map(\.provider) == expected)
        #expect(store.info == script.list?.catalog)
        #expect(script.listCalls == 1)
    }

    @Test func theSubtitleFollowsTheLoadedCatalog() async throws {
        let store = script.store()
        await store.store.resnapshot()
        let utc = try #require(TimeZone(identifier: "UTC"))
        #expect(
            store.subtitle(
                locale: Locale(identifier: "en_US"), calendar: Calendar(identifier: .gregorian),
                timeZone: utc) == "Official · updated Oct 20, 2026")
    }

    @Test func aDaemonWithoutTheMethodMarksTheStoreUnsupported() async {
        script.failure = .rejected(
            ErrorBody(
                code: "request.unknown_method", category: .unsupported, message: "unknown method",
                retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
                operationId: nil, rule: nil, items: nil))
        let store = script.store()
        await store.store.resnapshot()
        #expect(store.store.unsupported)
        #expect(store.providers == nil)
    }

    @Test func aFailedSnapshotKeepsTheErrorAndNoData() async {
        script.failure = .rejected(
            ErrorBody(
                code: "catalog.unavailable", category: .unavailable,
                message: "the provider catalog is not available", retryable: false,
                resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                rule: nil, items: nil))
        let store = script.store()
        await store.store.resnapshot()
        #expect(store.store.error?.code == "catalog.unavailable")
        #expect(store.providers == nil)
    }

    @Test func aProviderIsKnownByItsID() throws {
        let nats = try #require(
            CatalogFixture.list().items?.first { $0.provider == "official/nats" })
        #expect(nats.id == "official/nats")
    }

    @Test func fetchingOneProviderReadsItsSummary() async throws {
        let summary = try await CatalogStore.provider(
            "official/nats", call: { call throws(ClientError) in try await script.call(call) })
        #expect(summary?.displayName == "NATS Server")
    }

    @Test func aMissingProviderFetchesAsNil() async throws {
        let missing = ErrorBody(
            code: "catalog.provider_not_found", category: .notFound,
            message: "no provider official/x in the catalog", retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
        let summary = try await CatalogStore.provider(
            "official/x", call: { _ throws(ClientError) in throw .rejected(missing) })
        #expect(summary == nil)
    }

    // MARK: - Detail

    @Test func theDetailLoadsOnceAndIsKept() async throws {
        let store = script.store()
        await store.store.resnapshot()
        let nats = try CatalogFixture.detail("nats")
        #expect(store.detail.phase(for: "official/nats", revision: 1) == .loading)
        await store.detail.load("official/nats", revision: 1)
        await store.detail.load("official/nats", revision: 1)
        #expect(store.detail.phase(for: "official/nats", revision: 1) == .loaded(nats))
        #expect(script.getCalls == ["official/nats"])
    }

    @Test func aNewSnapshotDropsTheCachedDetail() async {
        let store = script.store()
        await store.store.resnapshot()
        await store.detail.load("official/nats", revision: 1)
        await store.store.resnapshot()
        #expect(store.detail.phase(for: "official/nats", revision: 1) == .loading)
    }

    @Test func aNewSnapshotMovesTheEpochSoTheSelectedDetailLoadsAgain() async throws {
        let store = script.store()
        await store.store.resnapshot()
        await store.detail.load("official/nats", revision: 1)
        let before = store.detail.epoch
        await store.store.resnapshot()
        #expect(store.detail.epoch != before)
        await store.detail.load("official/nats", revision: 1)
        #expect(
            store.detail.phase(for: "official/nats", revision: 1)
                == .loaded(try CatalogFixture.detail("nats")))
        #expect(script.getCalls == ["official/nats", "official/nats"])
    }

    @Test func aDetailIsKeptPerDaemonInstance() async {
        var server = UUID()
        let store = script.store(serverInstance: { server })
        await store.store.resnapshot()
        await store.detail.load("official/nats", revision: 1)
        server = UUID()
        #expect(store.detail.phase(for: "official/nats", revision: 1) == .loading)
        await store.detail.load("official/nats", revision: 1)
        #expect(script.getCalls.count == 2)
    }

    @Test func aDetailIsKeptPerCatalogRevision() async {
        let store = script.store()
        await store.detail.load("official/nats", revision: 1)
        #expect(store.detail.phase(for: "official/nats", revision: 2) == .loading)
    }

    @Test func aFailedDetailShowsTheDaemonsMessageAndRetryLoadsItAgain() async throws {
        let store = script.store()
        script.failure = .rejected(
            ErrorBody(
                code: "catalog.provider_not_found", category: .notFound,
                message: "no provider official/nats in the catalog", retryable: false,
                resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                rule: nil, items: nil))
        await store.detail.load("official/nats", revision: 1)
        #expect(
            store.detail.phase(for: "official/nats", revision: 1)
                == .failed(
                    FailureContent(
                        title: "Couldn’t Load Provider",
                        message: "no provider official/nats in the catalog",
                        code: "catalog.provider_not_found")))
        script.failure = nil
        await store.detail.retry("official/nats", revision: 1)
        #expect(
            store.detail.phase(for: "official/nats", revision: 1)
                == .loaded(try CatalogFixture.detail("nats")))
    }
}
