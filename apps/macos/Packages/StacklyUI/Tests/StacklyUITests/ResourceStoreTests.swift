import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The store's revision rules, snapshot buffer, generations, tombstones, and
/// retries, with a fake item and fake fetchers (spec 0013 AC-8).
@Suite(.timeLimit(.minutes(1)))
struct ResourceStoreTests {
    nonisolated struct Thing: Identifiable, Sendable, Equatable {
        let id: String
        let revision: UInt64
        var name: String
    }

    /// Fetchers the test controls: each snapshot waits until the test opens
    /// its gate, unless the result is given up front.
    final class Script {
        var pages: [[Thing]] = []
        var failures: [ClientError] = []
        var one: [String: Thing] = [:]
        var oneFailures: [ClientError] = []
        var fetchedOne: [String] = []
        var effects: [UInt64: StoreEffect<Thing>] = [:]
        var gate: CheckedContinuation<Void, Never>?
        var holdNextFetch = false
        var fetchCount = 0
        var sleeps: [Duration] = []

        func fetch(_ token: PageToken?) async throws(ClientError) -> Page<Thing> {
            fetchCount += 1
            if holdNextFetch {
                holdNextFetch = false
                await withCheckedContinuation { gate = $0 }
            }
            if failures.isEmpty == false { throw failures.removeFirst() }
            let index = Int(token?.value ?? "0") ?? 0
            let page = index < pages.count ? pages[index] : []
            let next = index + 1 < pages.count ? PageToken(String(index + 1)) : nil
            return Page(items: page, next: next)
        }

        func open() {
            gate?.resume()
            gate = nil
        }
    }

    let script = Script()

    func makeStore() -> ResourceStore<Thing> {
        let script = script
        return ResourceStore<Thing>(
            kinds: ["events."],
            fetchPage: { token throws(ClientError) in try await script.fetch(token) },
            fetchOne: { id throws(ClientError) in
                script.fetchedOne.append(id)
                if script.oneFailures.isEmpty == false { throw script.oneFailures.removeFirst() }
                return script.one[id]
            },
            revision: \.revision,
            reduce: { event in script.effects[event.sequence] ?? .ignore },
            now: { Date(timeIntervalSince1970: 100) },
            sleep: { script.sleeps.append($0) }
        )
    }

    static func event(_ sequence: UInt64) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: Date(timeIntervalSince1970: 0),
            resource: nil, revision: nil, kind: .eventsGap(Gap(reason: .overflow)))
    }

    @Test func aSnapshotLoadsEveryPage() async {
        script.pages = [
            [Thing(id: "a", revision: 1, name: "a")], [Thing(id: "b", revision: 1, name: "b")],
        ]
        let store = makeStore()
        #expect(await store.resnapshot())
        #expect(store.count == 2)
        #expect(store.loadedAt == Date(timeIntervalSince1970: 100))
        #expect(store.error == nil)
        #expect(store.isRefreshing == false)
    }

    @Test func onlyANewerRevisionApplies() async {
        script.pages = [[Thing(id: "a", revision: 3, name: "old")]]
        let store = makeStore()
        await store.resnapshot()
        script.effects[1] = .upsert(Thing(id: "a", revision: 2, name: "older"))
        script.effects[2] = .upsert(Thing(id: "a", revision: 4, name: "newer"))
        script.effects[3] = .patch("a", revision: 4) { $0.name = "same revision" }
        store.receive(Self.event(1))
        store.receive(Self.event(2))
        store.receive(Self.event(3))
        #expect(store.items["a"]?.name == "newer")
    }

    @Test func eventsDuringASnapshotWaitAndApplyAfterIt() async {
        script.pages = [[Thing(id: "a", revision: 1, name: "snapshot")]]
        script.holdNextFetch = true
        let store = makeStore()
        let running = Task { await store.resnapshot() }
        while script.gate == nil { await Task.yield() }
        script.effects[1] = .upsert(Thing(id: "a", revision: 2, name: "event"))
        store.receive(Self.event(1))
        #expect(store.items["a"] == nil)
        script.open()
        _ = await running.value
        #expect(store.items["a"]?.name == "event")
    }

    @Test func aSnapshotOfAnOlderGenerationIsDropped() async {
        script.pages = [[Thing(id: "a", revision: 1, name: "first")]]
        script.holdNextFetch = true
        let store = makeStore()
        let first = Task { await store.resnapshot() }
        while script.gate == nil { await Task.yield() }
        let held = script.gate
        script.gate = nil
        script.pages = [[Thing(id: "b", revision: 1, name: "second")]]
        #expect(await store.resnapshot())
        script.pages = [[Thing(id: "a", revision: 1, name: "first")]]
        held?.resume()
        #expect(await first.value == false)
        #expect(Set(store.items.keys) == ["b"])
    }

    @Test func aDeleteIsNotUndoneByAnOlderSnapshot() async {
        script.pages = [[Thing(id: "a", revision: 4, name: "a")]]
        script.holdNextFetch = true
        let store = makeStore()
        let running = Task { await store.resnapshot() }
        while script.gate == nil { await Task.yield() }
        // The delete arrived first; the snapshot was taken before it.
        script.effects[1] = .delete("a", revision: 5)
        store.receive(Self.event(1))
        script.open()
        _ = await running.value
        #expect(store.items["a"] == nil)
        #expect(store.tombstones.isEmpty)
    }

    @Test func aPatchForAnUnknownItemFetchesIt() async throws {
        let store = makeStore()
        await store.resnapshot()
        script.one["x"] = Thing(id: "x", revision: 2, name: "fetched")
        script.effects[1] = .patch("x", revision: 2) { $0.name = "never" }
        store.receive(Self.event(1))
        try await waitUntil { store.items["x"] != nil }
        #expect(store.items["x"]?.name == "fetched")
    }

    @Test func aRefetchOfAGoneItemDeletesIt() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.effects[1] = .refetch("a", revision: 2)
        store.receive(Self.event(1))
        try await waitUntil { store.items["a"] == nil }
        #expect(store.tombstones["a"] == 2)
    }

    // MARK: - Item failures (spec 0022, AC-9)

    @Test func aFailedRefetchIsRecordedAndKeepsTheHeldItem() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.oneFailures = [.rejected(Self.error("resource.busy"))]
        script.effects[1] = .refetch("a", revision: 2)
        store.receive(Self.event(1))
        try await waitUntil { store.itemFailures["a"] != nil }
        #expect(store.itemFailures["a"]?.error.code == "resource.busy")
        #expect(store.itemFailures["a"]?.revision == 2)
        #expect(store.items["a"]?.name == "a")
    }

    @Test func retryAsksAgainAtTheRevisionOfTheFailureAndClearsIt() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.oneFailures = [.notSent(Self.error("ipc.not_connected"))]
        script.effects[1] = .refetch("a", revision: 2)
        store.receive(Self.event(1))
        try await waitUntil { store.itemFailures["a"] != nil }
        script.one["a"] = Thing(id: "a", revision: 2, name: "fetched")
        await store.retry("a")
        #expect(store.itemFailures.isEmpty)
        #expect(store.items["a"]?.name == "fetched")
        #expect(script.fetchedOne == ["a", "a"])
    }

    @Test func aRetryThatFailsAgainKeepsTheFailure() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.oneFailures = [
            .rejected(Self.error("resource.busy")), .rejected(Self.error("resource.locked")),
        ]
        script.effects[1] = .refetch("a", revision: 2)
        store.receive(Self.event(1))
        try await waitUntil { store.itemFailures["a"] != nil }
        await store.retry("a")
        #expect(store.itemFailures["a"]?.error.code == "resource.locked")
        #expect(store.items["a"]?.name == "a")
    }

    @Test func aRetryOfAnItemThatIsGoneRemovesItAndItsFailure() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.oneFailures = [.rejected(Self.error("resource.busy"))]
        script.effects[1] = .refetch("a", revision: 2)
        store.receive(Self.event(1))
        try await waitUntil { store.itemFailures["a"] != nil }
        // The fetcher now answers nil: a retained instance, or one the daemon deleted.
        await store.retry("a")
        #expect(store.items["a"] == nil)
        #expect(store.itemFailures.isEmpty)
        #expect(store.tombstones["a"] == 2)
    }

    @Test func aRetryWithoutAFailureAsksNothing() async {
        let store = makeStore()
        await store.resnapshot()
        await store.retry("a")
        #expect(script.fetchedOne.isEmpty)
    }

    @Test func aNewerEventOrASnapshotClearsTheFailure() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.oneFailures = [.rejected(Self.error("resource.busy"))]
        script.effects[1] = .refetch("a", revision: 2)
        store.receive(Self.event(1))
        try await waitUntil { store.itemFailures["a"] != nil }
        script.effects[2] = .upsert(Thing(id: "a", revision: 3, name: "event"))
        store.receive(Self.event(2))
        #expect(store.itemFailures.isEmpty)

        script.oneFailures = [.rejected(Self.error("resource.busy"))]
        script.effects[3] = .refetch("a", revision: 4)
        store.receive(Self.event(3))
        try await waitUntil { store.itemFailures["a"] != nil }
        await store.resnapshot()
        #expect(store.itemFailures.isEmpty)
    }

    @Test func aFullBufferStartsOverThreeTimesThenKeepsAnError() async {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        let limit = ResourceStore<Thing>.bufferLimit
        script.holdNextFetch = true
        let running = Task { await store.resnapshot() }
        for round in 0..<4 {
            while script.gate == nil { await Task.yield() }
            for sequence in 0...limit {
                store.receive(Self.event(UInt64(sequence)))
            }
            if round < 3 { script.holdNextFetch = true }
            script.open()
        }
        #expect(await running.value)
        #expect(script.sleeps == [.seconds(1), .seconds(2), .seconds(4)])
        #expect(store.error?.code == "internal.unexpected")
        #expect(store.items["a"] != nil)
    }

    @Test func aStartingDaemonIsAskedAgainEverySecond() async {
        script.failures = [.rejected(Self.error("daemon.starting"))]
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        #expect(await store.resnapshot())
        #expect(script.sleeps == [.seconds(1)])
        #expect(store.count == 1)
    }

    @Test func anOlderDaemonMarksTheStoreUnsupported() async {
        script.failures = [.rejected(Self.error("request.unknown_method"))]
        let store = makeStore()
        #expect(await store.resnapshot())
        #expect(store.unsupported)
        #expect(store.error == nil)
    }

    @Test func anotherErrorKeepsTheOldData() async {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        await store.resnapshot()
        script.failures = [.rejected(Self.error("resource.busy"))]
        await store.refresh()
        #expect(store.error?.code == "resource.busy")
        #expect(store.items["a"] != nil)
        #expect(store.loadedAt != nil)
    }

    // MARK: - Shell sync

    @Test func shellSyncRoutesEventsByKindAndCountsRounds() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        let sync = ShellSync()
        sync.register(store)
        #expect(sync.completedRounds == 0)
        sync.handle(.resnapshot(.noCursor))
        #expect(sync.isSettled == false)
        try await waitUntil { sync.isSettled }
        #expect(sync.completedRounds == 1)
        script.effects[9] = .upsert(Thing(id: "a", revision: 2, name: "routed"))
        sync.handle(.event(Self.event(9)))
        #expect(store.items["a"]?.name == "routed")
        sync.handle(.caughtUp)
        #expect(sync.completedRounds == 2)
        sync.handle(.lastFrameAt(Date(timeIntervalSince1970: 5)))
        #expect(sync.lastFrameAt == Date(timeIntervalSince1970: 5))
    }

    @Test func aCaughtUpReloadsAStoreWhoseSnapshotFailedAndCountsTheRoundAfterIt() async throws {
        script.failures = [.rejected(Self.error("resource.busy"))]
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        let sync = ShellSync()
        sync.register(store)
        sync.handle(.resnapshot(.noCursor))
        try await waitUntil { sync.isSettled }
        #expect(store.error?.code == "resource.busy")
        #expect(sync.completedRounds == 1)
        script.holdNextFetch = true
        sync.handle(.caughtUp)
        // The round ends after the reload, not when the replay caught up.
        #expect(sync.isSettled == false)
        try await waitUntil { script.gate != nil }
        #expect(sync.completedRounds == 1)
        script.open()
        try await waitUntil { sync.isSettled }
        #expect(script.fetchCount == 2)
        #expect(store.error == nil)
        #expect(store.items["a"] != nil)
        #expect(sync.completedRounds == 2)
    }

    @Test func aCaughtUpLeavesAHealthyStoreAlone() async throws {
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        let sync = ShellSync()
        sync.register(store)
        sync.handle(.resnapshot(.noCursor))
        try await waitUntil { sync.isSettled }
        sync.handle(.caughtUp)
        #expect(sync.isSettled)
        #expect(sync.completedRounds == 2)
        #expect(script.fetchCount == 1)
    }

    @Test func aCaughtUpLeavesAnUnsupportedStoreAndAResnapshotThatWorksClearsIt() async throws {
        script.failures = [.rejected(Self.error("request.unknown_method"))]
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        let sync = ShellSync()
        sync.register(store)
        sync.handle(.resnapshot(.noCursor))
        try await waitUntil { sync.isSettled }
        #expect(store.unsupported)
        // Only a new daemon run knows the method, and it always starts a resnapshot.
        sync.handle(.caughtUp)
        #expect(sync.isSettled)
        #expect(sync.completedRounds == 2)
        #expect(script.fetchCount == 1)
        sync.handle(.resnapshot(.gap(.serverRestarted)))
        try await waitUntil { sync.isSettled }
        #expect(store.unsupported == false)
        #expect(store.items["a"] != nil)
        #expect(sync.completedRounds == 3)
    }

    @Test func aReloadThatFailsAgainEndsTheRoundAndWaitsForTheNextCaughtUp() async throws {
        script.failures = [
            .rejected(Self.error("resource.busy")), .rejected(Self.error("resource.busy")),
        ]
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        let sync = ShellSync()
        sync.register(store)
        sync.handle(.resnapshot(.noCursor))
        try await waitUntil { sync.isSettled }
        sync.handle(.caughtUp)
        try await waitUntil { sync.isSettled }
        #expect(sync.completedRounds == 2)
        #expect(script.fetchCount == 2)
        #expect(store.error?.code == "resource.busy")
        sync.handle(.caughtUp)
        try await waitUntil { sync.isSettled }
        #expect(sync.completedRounds == 3)
        #expect(script.fetchCount == 3)
        #expect(store.error == nil)
    }

    @Test func aCaughtUpDoesNotStartASecondSnapshotOfARefreshingStore() async throws {
        script.failures = [.rejected(Self.error("resource.busy"))]
        script.pages = [[Thing(id: "a", revision: 1, name: "a")]]
        let store = makeStore()
        let sync = ShellSync()
        sync.register(store)
        sync.handle(.resnapshot(.noCursor))
        try await waitUntil { sync.isSettled }
        script.holdNextFetch = true
        let retry = Task { await store.refresh() }
        try await waitUntil { script.gate != nil }
        #expect(store.isRefreshing)
        #expect(store.error != nil)
        sync.handle(.caughtUp)
        #expect(sync.completedRounds == 2)
        script.open()
        await retry.value
        #expect(script.fetchCount == 2)
        #expect(store.error == nil)
    }

    @Test func shellSyncWithoutStoresSettlesAtOnce() {
        let sync = ShellSync()
        sync.handle(.resnapshot(.gap(.overflow)))
        #expect(sync.isSettled)
        #expect(sync.completedRounds == 1)
    }

    static func error(_ code: String) -> ErrorBody {
        ErrorBody(
            code: code, category: .unavailable, message: "m", retryable: true, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }
}

/// Waits until `condition` holds, checking every few milliseconds.
func waitUntil(_ condition: () -> Bool) async throws {
    for _ in 0..<2000 {
        if condition() { return }
        try await Task.sleep(for: .milliseconds(5))
    }
    Issue.record("the condition never held")
}
