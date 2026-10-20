import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The instance store: snapshots, the `statusSeq` recipe of spec 0010 (AC-16), the retained
/// rule (K16), and the count behind the badge and subtitle (spec 0013 AC-13, AC-17).
@Suite(.timeLimit(.minutes(1)))
struct InstanceStoreTests {
    /// What the fetchers answer, and what they were asked.
    final class Script {
        var pages: [[InstanceSummary]] = []
        var failure: ClientError?
        var one: [UUID: InstanceSummary] = [:]
        var fetchedOne: [UUID] = []
        var snapshots = 0
        var gate: CheckedContinuation<Void, Never>?
        var holdNextFetch = false

        func open() {
            gate?.resume()
            gate = nil
        }

        func page(_ token: PageToken?) async throws(ClientError) -> Page<InstanceSummary> {
            snapshots += token == nil ? 1 : 0
            if holdNextFetch {
                holdNextFetch = false
                await withCheckedContinuation { gate = $0 }
            }
            if let failure { throw failure }
            let index = Int(token?.value ?? "0") ?? 0
            let items = index < pages.count ? pages[index] : []
            return Page(
                items: items, next: index + 1 < pages.count ? PageToken("\(index + 1)") : nil)
        }

        func fetchOne(_ id: UUID) -> InstanceSummary? {
            fetchedOne.append(id)
            return one[id]
        }
    }

    let script = Script()

    /// orders-db: running, ready, healthy, revision 7, `statusSeq` 4.
    let orders: InstanceSummary
    /// cache: failed, revision 9, `statusSeq` 12.
    let cache: InstanceSummary

    init() throws {
        orders = try ContractFixture.instance()
        cache = try ContractFixture.instance("frame-response-instance-get-failed")
    }

    func makeStore() -> InstanceStore {
        let script = script
        return InstanceStore(
            fetchPage: { token throws(ClientError) in try await script.page(token) },
            fetchOne: { id in script.fetchOne(id) },
            now: { Date(timeIntervalSince1970: 100) },
            sleep: { _ in })
    }

    func loaded(_ instances: [InstanceSummary]) async -> InstanceStore {
        script.pages = [instances]
        let store = makeStore()
        await store.store.resnapshot()
        return store
    }

    // MARK: - Snapshot, count, and order

    @Test func theCountAndSubtitleWaitForTheFirstSnapshot() async {
        script.pages = [[orders]]
        let store = makeStore()
        #expect(store.loadedCount == nil)
        #expect(store.subtitle == nil)
        await store.store.resnapshot()
        #expect(store.loadedCount == 1)
        #expect(store.subtitle == "1 instance")
    }

    @Test func theSubtitleCountsInstancesInThePlural() async {
        let store = await loaded([orders, cache])
        #expect(store.count == 2)
        #expect(store.subtitle == "2 instances")
        #expect(InstanceStore.subtitle(count: nil) == nil)
        #expect(InstanceStore.subtitle(count: 0) == "0 instances")
    }

    @Test func instancesAreSortedByNameLikeTheFinderDoes() async {
        let tenth = orders.testing(id: UUID(), name: "db10")
        let second = orders.testing(id: UUID(), name: "db2")
        let store = await loaded([tenth, cache, second, orders])
        #expect(store.instances.map(\.name) == ["cache", "db2", "db10", "orders-db"])
    }

    @Test func aSnapshotLoadsEveryPageWithTheListLimit() async throws {
        script.pages = [[orders], [cache]]
        let store = makeStore()
        await store.store.resnapshot()
        #expect(store.count == 2)
        #expect(InstanceStore.pageLimit == 200)
    }

    // MARK: - Events

    @Test func aStateChangeUpdatesOneRowWithoutLoadingTheListAgain() async {
        let store = await loaded([orders, cache])
        #expect(script.snapshots == 1)
        let stopped = orders.testing(revision: 8, lifecycle: .stopped)
        store.store.receive(.instanceState(stopped))
        #expect(store.store.items[orders.id]?.lifecycle == .stopped)
        #expect(store.store.items[cache.id]?.lifecycle == .failed)
        #expect(script.snapshots == 1)
    }

    @Test func aStateChangeForAnUnknownInstanceIsAppliedAsItIs() async {
        let store = await loaded([cache])
        store.store.receive(.instanceState(orders))
        #expect(store.count == 2)
        #expect(store.store.items[orders.id] == orders)
    }

    @Test func aHealthChangeForAnUnknownInstanceFetchesIt() async throws {
        let store = await loaded([cache])
        script.one[orders.id] = orders
        let change = orders.healthChange(statusSeq: 5, health: .degraded)
        store.store.receive(.instanceHealth(change, revision: 7))
        try await waitUntil { store.store.items[orders.id] != nil }
        #expect(script.fetchedOne == [orders.id])
        #expect(store.store.items[orders.id]?.health.state == .healthy)
    }

    @Test func aHealthChangeDoesNotBringBackAnInstanceThatWasRemoved() async throws {
        let store = await loaded([orders])
        store.store.receive(.instanceRemoved(orders.id, revision: 8))
        #expect(store.count == 0)
        script.one[orders.id] = orders
        store.store.receive(
            .instanceHealth(orders.healthChange(statusSeq: 9, health: .degraded), revision: 8))
        for _ in 0..<50 { await Task.yield() }
        #expect(script.fetchedOne.isEmpty)
        #expect(store.count == 0)
    }

    // MARK: - The statusSeq recipe (spec 0010 AC-16)

    @Test func theStatusSeqFixtureRecipeAppliesEachObservationInOrder() async throws {
        let store = await loaded([orders])
        #expect(store.store.items[orders.id]?.statusSeq == 4)

        // statusSeq 5 at revision 7: the snapshot already holds revision 7.
        let degraded = try ContractFixture.healthChange("frame-event-instance-health-changed")
        store.store.receive(
            .instanceHealth(degraded.change, revision: try #require(degraded.event.revision)))
        #expect(store.store.items[orders.id]?.health.state == .degraded)
        #expect(store.store.items[orders.id]?.statusSeq == 5)

        // statusSeq 6 at revision 6, lower than the held 7: it still applies.
        let lower = try ContractFixture.healthChange(
            "frame-event-instance-health-changed-lower-revision")
        #expect(try #require(lower.event.revision) < orders.revision)
        store.store.receive(
            .instanceHealth(lower.change, revision: try #require(lower.event.revision)))
        let held = try #require(store.store.items[orders.id])
        #expect(held.health.state == .unhealthy)
        #expect(held.statusSeq == 6)
        #expect(held.blocker == lower.change.blocker)
        #expect(held.blocker != nil)
    }

    @Test func anObservationWithAnOlderStatusSeqIsIgnored() async throws {
        let store = await loaded([orders])
        let newer = orders.healthChange(statusSeq: 9, health: .unhealthy)
        let older = orders.healthChange(statusSeq: 8, health: .degraded)
        let sameSeq = orders.healthChange(statusSeq: 9, health: .degraded)
        store.store.receive(.instanceHealth(newer, revision: 7, sequence: 1))
        store.store.receive(.instanceHealth(older, revision: 99, sequence: 2))
        store.store.receive(.instanceHealth(sameSeq, revision: 99, sequence: 3))
        #expect(store.store.items[orders.id]?.health.state == .unhealthy)
        #expect(store.store.items[orders.id]?.statusSeq == 9)
    }

    @Test func aHealthChangeDoesNotRaiseTheRevisionSoAStateChangeAtThatRevisionApplies()
        async throws
    {
        let before = orders.testing(revision: 6, statusSeq: 3)
        let store = await loaded([before])
        // The health change carries revision 7, the revision of the state change below.
        let change = before.healthChange(statusSeq: 5, health: .degraded)
        store.store.receive(.instanceHealth(change, revision: 7, sequence: 1))
        #expect(store.store.revisions[before.id] == 6)

        let stopped = before.testing(
            revision: 7, lifecycle: .stopping, health: before.health, statusSeq: 4)
        store.store.receive(.instanceState(stopped, sequence: 2))
        let held = try #require(store.store.items[before.id])
        #expect(held.lifecycle == .stopping)
        #expect(store.store.revisions[before.id] == 7)
        // The lifecycle came from the state change; the status fields keep the newer
        // observation (statusSeq 5 beats 4).
        #expect(held.statusSeq == 5)
        #expect(held.health.state == .degraded)
    }

    @Test func aStateChangeTakesTheStatusFieldsOnlyWhenItsStatusSeqIsGreater() async throws {
        let store = await loaded([orders])
        let incoming = orders.testing(
            revision: 8, lifecycle: .stopping,
            health: InstanceHealth(
                state: .unhealthy, reason: "x", detail: nil, observedAt: orders.health.observedAt),
            statusSeq: 6)
        store.store.receive(.instanceState(incoming, sequence: 1))
        #expect(store.store.items[orders.id]?.health.state == .unhealthy)
        #expect(store.store.items[orders.id]?.statusSeq == 6)

        // A tie keeps the held status fields.
        let tie = orders.testing(revision: 9, lifecycle: .stopped, statusSeq: 6)
        store.store.receive(.instanceState(tie, sequence: 2))
        let held = try #require(store.store.items[orders.id])
        #expect(held.lifecycle == .stopped)
        #expect(held.health.state == .unhealthy)

        // A lower statusSeq keeps them too.
        let older = orders.testing(revision: 10, lifecycle: .running, statusSeq: 2)
        store.store.receive(.instanceState(older, sequence: 3))
        #expect(store.store.items[orders.id]?.lifecycle == .running)
        #expect(store.store.items[orders.id]?.health.state == .unhealthy)
        #expect(store.store.items[orders.id]?.statusSeq == 6)
    }

    @Test func anObservationDuringASnapshotWaitsAndAppliesAfterIt() async throws {
        script.pages = [[orders]]
        script.holdNextFetch = true
        let store = makeStore()
        let running = Task { await store.store.resnapshot() }
        while script.gate == nil { await Task.yield() }
        store.store.receive(
            .instanceHealth(orders.healthChange(statusSeq: 7, health: .degraded), revision: 7))
        #expect(store.store.items[orders.id] == nil)
        script.open()
        _ = await running.value
        let held = try #require(store.store.items[orders.id])
        #expect(held.health.state == .degraded)
        #expect(held.statusSeq == 7)
        #expect(store.store.revisions[orders.id] == 7)
    }

    @Test func aRestartResetsTheStatusSeqBoundEvenWhenTheSnapshotFails() async throws {
        let store = await loaded([cache])
        #expect(store.store.items[cache.id]?.statusSeq == 12)
        let sync = ShellSync()
        sync.register(store.store)

        // The new run starts its counter again at 1; its snapshot fails.
        script.failure = .rejected(ResourceStoreTests.error("resource.busy"))
        sync.handle(.resnapshot(.gap(.serverRestarted)))
        try await waitUntil { sync.isSettled }
        #expect(store.store.error?.code == "resource.busy")
        #expect(store.store.items[cache.id] != nil)

        let change = cache.healthChange(statusSeq: 2, health: .degraded)
        sync.handle(.event(.instanceHealth(change, revision: 9)))
        #expect(store.store.items[cache.id]?.health.state == .degraded)
        #expect(store.store.items[cache.id]?.statusSeq == 2)
    }

    @Test func anOtherGapKeepsTheStatusSeqBound() async throws {
        let store = await loaded([cache])
        let sync = ShellSync()
        sync.register(store.store)
        script.failure = .rejected(ResourceStoreTests.error("resource.busy"))
        sync.handle(.resnapshot(.gap(.overflow)))
        try await waitUntil { sync.isSettled }
        let change = cache.healthChange(statusSeq: 2, health: .degraded)
        sync.handle(.event(.instanceHealth(change, revision: 9)))
        #expect(store.store.items[cache.id]?.health.state == .unhealthy)
        #expect(store.store.items[cache.id]?.statusSeq == 12)
    }

    // MARK: - Retained instances (K16)

    @Test func aRetainedStateChangeRemovesTheRowAndLeavesATombstone() async {
        let store = await loaded([orders, cache])
        let retained = orders.testing(revision: 8, lifecycle: .retained)
        store.store.receive(.instanceState(retained, sequence: 1))
        #expect(store.count == 1)
        #expect(store.store.items[orders.id] == nil)
        #expect(store.store.tombstones[orders.id] == 8)
        // An older state change cannot bring it back.
        store.store.receive(.instanceState(orders.testing(revision: 7), sequence: 2))
        #expect(store.count == 1)
    }

    @Test func removedRemovesTheRowAtTheEventRevision() async {
        let store = await loaded([orders, cache])
        store.store.receive(.instanceRemoved(cache.id, revision: 10))
        #expect(store.store.items[cache.id] == nil)
        #expect(store.store.tombstones[cache.id] == 10)
        #expect(store.subtitle == "1 instance")
    }

    @Test func aRetainedInstanceNeverEntersThroughOfferOrASnapshot() async {
        let retained = orders.testing(revision: 8, lifecycle: .retained)
        let store = await loaded([retained, cache])
        #expect(store.count == 1)
        #expect(store.store.items[orders.id] == nil)

        store.offer(retained.testing(id: UUID(), revision: 3))
        #expect(store.count == 1)

        // A retained summary offered for a held instance removes it.
        store.offer(cache.testing(revision: 10, lifecycle: .retained))
        #expect(store.count == 0)
        #expect(store.store.tombstones[cache.id] == 10)
    }

    @Test func aHealthChangeForAnInstanceThatIsRetainedOrGoneBecomesADelete() async throws {
        let store = await loaded([orders])
        // fetchOne answers nil for a retained or missing instance.
        let other = orders.testing(id: UUID(), name: "gone")
        store.store.receive(
            .instanceHealth(other.healthChange(statusSeq: 1, health: .degraded), revision: 4))
        try await waitUntil { script.fetchedOne.contains(other.id) }
        try await waitUntil { store.store.tombstones[other.id] == 4 }
        #expect(store.store.items[other.id] == nil)
        #expect(store.count == 1)
    }

    // MARK: - Calls

    /// Collects the calls and answers with `reply`.
    final class Calls {
        var made: [Call] = []
        var reply: Result<Reply, ClientError> = .success(.eventsUnsubscribe)

        func send(_ call: Call) throws(ClientError) -> Reply {
            made.append(call)
            return try reply.get()
        }
    }

    @Test func theListCallAsksForTwoHundredAndDropsRetainedSummaries() async throws {
        let calls = Calls()
        let retained = orders.testing(id: UUID(), lifecycle: .retained)
        calls.reply = .success(
            .instanceList(InstancePage(items: [orders, retained], next: "token-2")))
        let page = try await InstanceStore.page(after: PageToken("token-1")) {
            call throws(ClientError) in
            try calls.send(call)
        }
        #expect(page.items == [orders])
        #expect(page.next == PageToken("token-2"))
        guard case .instanceList(let params) = try #require(calls.made.first) else {
            Issue.record("not an instance.list call")
            return
        }
        #expect(params.limit == 200)
        #expect(params.before == "token-1")
    }

    @Test func aReplyOfAnotherKindIsAProtocolViolation() async {
        let calls = Calls()
        do {
            _ = try await InstanceStore.page(after: nil) { call throws(ClientError) in
                try calls.send(call)
            }
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error.body.code == "ipc.protocol_violation")
        }
    }

    @Test func getAnswersNilForAMissingOrRetainedInstance() async throws {
        let calls = Calls()
        calls.reply = .failure(.rejected(ResourceStoreTests.error("resource.not_found")))
        let missing = try await InstanceStore.instance(orders.id) { call throws(ClientError) in
            try calls.send(call)
        }
        #expect(missing == nil)

        calls.reply = .success(.instanceGet(orders.testing(lifecycle: .retained)))
        let retained = try await InstanceStore.instance(orders.id) { call throws(ClientError) in
            try calls.send(call)
        }
        #expect(retained == nil)

        calls.reply = .success(.instanceGet(orders))
        let found = try await InstanceStore.instance(orders.id) { call throws(ClientError) in
            try calls.send(call)
        }
        #expect(found == orders)
    }

    @Test func getKeepsAnotherErrorForTheCaller() async {
        let calls = Calls()
        calls.reply = .failure(.rejected(ResourceStoreTests.error("resource.busy")))
        do {
            _ = try await InstanceStore.instance(orders.id) { call throws(ClientError) in
                try calls.send(call)
            }
            Issue.record("expected the error")
        } catch {
            #expect(error.body.code == "resource.busy")
        }
    }
}
