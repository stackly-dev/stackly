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

    // MARK: - Local providers (spec 0037)

    @Test(
        .evidence(
            covers: ["REQ-033"],
            expected: "catalog.changed makes the catalog store load a snapshot again"
        ))
    func catalogChangedLoadsTheCatalogAgain() async throws {
        let store = script.store()
        await store.store.resnapshot()
        #expect(script.listCalls == 1)
        store.store.receive(
            Event(
                subscriptionId: 1, sequence: 1, at: Date(timeIntervalSince1970: 100),
                resource: nil, revision: nil,
                kind: .catalogChanged(CatalogChanged(provider: "acme/demo", change: .imported))))
        try await waitUntil { script.listCalls == 2 }
    }

    @Test func aSnapshotKeepsTheRevocationsAndTheImportedProviders() async throws {
        let local = try ProviderFixture.detail("pending").summary
        let revocation = CatalogRevocation(
            provider: "official/nats", providerRevision: "1.0.0", manifestSha256: nil,
            reason: "Starts with an insecure default.", since: "2026-11-05")
        let list = try #require(script.list)
        script.list = ListProvidersReply(
            catalog: list.catalog, items: (list.items ?? []) + [local], revocations: [revocation])
        let store = script.store()
        await store.store.resnapshot()
        #expect(store.revocations == [revocation])
        #expect(store.localProviders?.map(\.provider) == ["acme/demo"])
    }

    // MARK: - Snapshots and the detail cache (spec 0037 review, Major 3)

    // covers: spec 0037 review, Major 3
    @Test func aLoadAfterInvalidateStartsAtOnceAndKeepsItsAnswer() async throws {
        let nats = try CatalogFixture.detail("nats")
        let gate = ReplyGate()
        let model = ProviderDetailModel(call: { _ throws(ClientError) in await gate.wait() })
        let first = Task { await model.load("official/nats", revision: 1) }
        await settle("the first call") { gate.waiting.count == 1 }

        model.invalidate()
        let second = Task { await model.load("official/nats", revision: 1) }
        await settle("a second call for the new epoch") { gate.waiting.count == 2 }

        gate.answer(0, .catalogGetProvider(nats))
        await first.value
        #expect(model.phase(for: "official/nats", revision: 1) == .loading)
        gate.answer(1, .catalogGetProvider(nats))
        await second.value
        #expect(model.phase(for: "official/nats", revision: 1) == .loaded(nats))
    }

    // covers: spec 0037 review, Major 3
    @Test func catalogChangedAndAReloadShareSnapshotsAndTheDetailLoadsOnce() async throws {
        let gate = ReplyGate()
        let script = script
        let release = Reply.catalogListProviders(try #require(script.list))
        let store = CatalogStore(
            call: { call throws(ClientError) in
                if case .catalogListProviders = call { _ = await gate.wait() }
                return try await script.call(call)
            },
            now: { Date(timeIntervalSince1970: 100) }, sleep: { _ in })
        let event = Event(
            subscriptionId: 1, sequence: 1, at: Date(timeIntervalSince1970: 100),
            resource: nil, revision: nil,
            kind: .catalogChanged(CatalogChanged(provider: "acme/demo", change: .approved)))

        store.store.receive(event)
        await settle("the snapshot of the event") { gate.waiting.count == 1 }
        let reloads = (0..<3).map { _ in Task { await store.reload() } }
        store.store.receive(event)
        for _ in 0..<5 { await Task.yield() }
        #expect(gate.waiting.count == 1)

        gate.answer(0, release)
        await settle("one more snapshot for the requests") { gate.waiting.count == 2 }
        gate.answer(1, release)
        for _ in 0..<5 { await Task.yield() }
        #expect(gate.waiting.count == 2)
        gate.answerAll(release)
        for reload in reloads { await reload.value }
        #expect(script.listCalls == 2)
        #expect(store.providers?.isEmpty == false)

        let epoch = store.detail.epoch
        await store.detail.load("official/nats", revision: 1)
        #expect(store.detail.epoch == epoch)
        #expect(
            store.detail.phase(for: "official/nats", revision: 1)
                == .loaded(try CatalogFixture.detail("nats")))
    }

    // covers: spec 0037 review, Major 3
    @Test func aSnapshotThatANewerOneReplacedChangesNothing() async throws {
        let gate = ReplyGate()
        let script = script
        let release = Reply.catalogListProviders(try #require(script.list))
        let store = CatalogStore(
            call: { call throws(ClientError) in
                if case .catalogListProviders = call { _ = await gate.wait() }
                return try await script.call(call)
            },
            now: { Date(timeIntervalSince1970: 100) }, sleep: { _ in })
        let older = Task { await store.store.resnapshot() }
        await settle("the older snapshot") { gate.waiting.count == 1 }
        let newer = Task { await store.store.resnapshot() }
        await settle("the newer snapshot") { gate.waiting.count == 2 }
        let epoch = store.detail.epoch

        gate.answer(0, release)
        _ = await older.value
        #expect(store.info == nil)
        #expect(store.detail.epoch == epoch)

        gate.answer(1, release)
        _ = await newer.value
        #expect(store.info == script.list?.catalog)
        #expect(store.detail.epoch == epoch + 1)
    }
}

/// Holds each call until the test answers it, by index.
@MainActor
final class ReplyGate {
    private(set) var waiting: [CheckedContinuation<Reply, Never>] = []
    private var answered: Set<Int> = []

    /// Waits for the test to answer this call.
    func wait() async -> Reply {
        await withCheckedContinuation { waiting.append($0) }
    }

    /// Answers every call not answered yet with `reply`, so a test that expected fewer
    /// calls fails instead of waiting forever.
    func answerAll(_ reply: Reply) {
        for index in waiting.indices { answer(index, reply) }
    }

    /// Answers call `index` with `reply`.
    func answer(_ index: Int, _ reply: Reply) {
        guard answered.insert(index).inserted else { return }
        waiting[index].resume(returning: reply)
    }
}
