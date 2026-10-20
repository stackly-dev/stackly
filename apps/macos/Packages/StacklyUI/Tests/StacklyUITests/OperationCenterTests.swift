import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The operation center against the shared operation fixtures: snapshots from
/// `operation.list`, both operation events, `operation.get`, the running count behind the
/// Operations badge and subtitle, recent operations, and `updates(for:)` (spec 0013 AC-13,
/// AC-18).
@Suite(.timeLimit(.minutes(1)))
struct OperationCenterTests {
    static let start = Date(timeIntervalSince1970: 1_792_400_000)

    /// Answers the center's calls from a script and records them.
    final class Daemon {
        var calls: [Call] = []
        /// The answers to `operation.list`, in order; an empty last page after them.
        var pages: [Result<Reply, ClientError>] = []
        /// The answers to `operation.get`; `resource.not_found` for any other ID.
        var operations: [UUID: OperationSummary] = [:]

        func call(_ call: Call) async throws(ClientError) -> Reply {
            calls.append(call)
            switch call {
            case .operationList:
                guard pages.isEmpty == false else {
                    return .operationList(OperationPage(items: [], next: nil))
                }
                return try pages.removeFirst().get()
            case .operationGet(let params):
                guard let operation = operations[params.operationId] else {
                    throw .rejected(Samples.error("resource.not_found"))
                }
                return .operationGet(
                    OperationDetail(operation: operation, steps: nil, created: nil))
            default:
                throw .rejected(Samples.error("request.unknown_method"))
            }
        }

        var listCalls: [ListOperationsParams] {
            calls.compactMap { if case .operationList(let params) = $0 { params } else { nil } }
        }
    }

    /// A center over a scripted daemon and a clock the test moves.
    final class Fixture {
        let daemon = Daemon()
        var clock = OperationCenterTests.start
        var center: OperationCenter!

        init() {
            center = OperationCenter(
                call: { [daemon] call throws(ClientError) in try await daemon.call(call) },
                now: { [unowned self] in clock })
        }

        /// Loads a snapshot from the list fixture: one running and one queued operation.
        func loadListFixture() async throws {
            let page = try ContractFixture.operationPage()
            daemon.pages = [.success(.operationList(OperationPage(items: page.items, next: nil)))]
            await center.store.resnapshot()
        }

        /// Collects every value of a stream until it ends.
        func collect(_ id: UUID) -> Task<[OperationSnapshot], Never> {
            let stream = center.updates(for: id)
            return Task {
                var values: [OperationSnapshot] = []
                for await value in stream { values.append(value) }
                return values
            }
        }
    }

    static func running() throws -> OperationSummary {
        try #require(try ContractFixture.operationPage().items?.first { $0.state == .running })
    }

    static func queued() throws -> OperationSummary {
        try #require(try ContractFixture.operationPage().items?.first { $0.state == .queued })
    }

    // MARK: - Snapshots and counts

    @Test func aSnapshotLoadsEveryPageOfTheOperationsThatHaveNotEnded() async throws {
        let fixture = Fixture()
        let page = try ContractFixture.operationPage()
        let next = try #require(page.next)
        fixture.daemon.pages = [
            .success(.operationList(page)),
            .success(.operationList(OperationPage(items: [], next: nil))),
        ]
        await fixture.center.store.resnapshot()

        #expect(
            fixture.daemon.listCalls == [
                ListOperationsParams(activeOnly: true, resource: nil, limit: nil, before: nil),
                ListOperationsParams(activeOnly: true, resource: nil, limit: nil, before: next),
            ])
        // Newest first, like operation.list: the running one was created after the queued one.
        #expect(fixture.center.active.map(\.id) == [try Self.running().id, try Self.queued().id])
        #expect(fixture.center.runningCount == 1)
        #expect(fixture.center.subtitle == "1 running")
    }

    @Test func beforeTheFirstSnapshotThereIsNoCountAndNoSubtitle() {
        let fixture = Fixture()
        #expect(fixture.center.runningCount == nil)
        #expect(fixture.center.subtitle == nil)
        #expect(fixture.center.active.isEmpty)
        let counts = SidebarCounts(runningOperations: fixture.center.runningCount)
        #expect(SidebarSection.operations.badge(counts: counts, isConnected: true) == nil)
    }

    @Test func theSubtitleCountsRunningAndCancellingOperations() {
        #expect(OperationCenter.subtitle(running: nil) == nil)
        #expect(OperationCenter.subtitle(running: 0) == "No running operations")
        #expect(OperationCenter.subtitle(running: 1) == "1 running")
        #expect(OperationCenter.subtitle(running: 12) == "12 running")
    }

    @Test func needsRecoveryIsActiveButNeverCountedAsRunning() async throws {
        let fixture = Fixture()
        guard
            case .operationGet(let detail) = try ContractFixture.reply(
                "frame-response-operation-get")
        else {
            Issue.record("the operation.get fixture holds another reply")
            return
        }
        let stuck = detail.operation
        #expect(stuck.state == .needsRecovery)
        let cancelling = try Self.running().with(state: .cancelRequested, revision: 9)
        fixture.daemon.pages = [
            .success(.operationList(OperationPage(items: [stuck, cancelling], next: nil)))
        ]
        await fixture.center.store.resnapshot()
        #expect(Set(fixture.center.active.map(\.id)) == [stuck.id, cancelling.id])
        #expect(fixture.center.runningCount == 1)
    }

    @Test func anOlderDaemonWithoutOperationMethodsShowsNoCount() async {
        let fixture = Fixture()
        fixture.daemon.pages = [.failure(.rejected(Samples.error("request.unknown_method")))]
        await fixture.center.store.resnapshot()
        #expect(fixture.center.store.unsupported)
        #expect(fixture.center.runningCount == nil)
        #expect(fixture.center.subtitle == nil)
    }

    // MARK: - Events

    @Test func anEventChangesOneOperationWithoutAnotherSnapshot() async throws {
        let fixture = Fixture()
        try await fixture.loadListFixture()
        let progress = try ContractFixture.event("frame-event-operation-progress")
        fixture.center.store.receive(progress)
        let updated = try #require(fixture.center.store.items[try Self.running().id])
        #expect(updated.revision == 5)
        #expect(
            updated == (try ContractFixture.operation(inEvent: "frame-event-operation-progress")))
        #expect(fixture.daemon.listCalls.count == 1)
        #expect(fixture.center.runningCount == 1)
    }

    @Test func anOlderEventNeverReplacesANewerOperation() async throws {
        let fixture = Fixture()
        try await fixture.loadListFixture()
        let running = try Self.running()
        fixture.center.store.receive(.operation(running.with(revision: 6), progress: true))
        fixture.center.store.receive(
            .operation(running.with(state: .cancelRequested, revision: 5)))
        #expect(fixture.center.store.items[running.id]?.revision == 6)
        #expect(fixture.center.store.items[running.id]?.state == .running)
    }

    @Test func anEndedOperationLeavesActiveAndStaysRecentForFiveMinutes() async throws {
        let fixture = Fixture()
        try await fixture.loadListFixture()
        let ended = try ContractFixture.operation(inEvent: "frame-event-operation-state-changed")
        #expect(ended.state == .succeeded)
        fixture.center.store.receive(
            try ContractFixture.event("frame-event-operation-state-changed"))

        #expect(fixture.center.active.map(\.id) == [try Self.queued().id])
        #expect(fixture.center.runningCount == 0)
        #expect(fixture.center.subtitle == "No running operations")
        #expect(fixture.center.recent == [ended])
        #expect(fixture.center.store.items[ended.id] == nil)

        // A late, older event cannot bring it back.
        fixture.center.store.receive(.operation(try Self.running().with(revision: 6)))
        #expect(fixture.center.store.items[ended.id] == nil)

        fixture.clock = Self.start.addingTimeInterval(299)
        #expect(fixture.center.recent == [ended])
        fixture.clock = Self.start.addingTimeInterval(301)
        #expect(fixture.center.recent.isEmpty)
    }

    // MARK: - Updates for one operation

    @Test func updatesGiveTheCurrentValueThenEachChangeAndEndAfterTheFinalState() async throws {
        let fixture = Fixture()
        try await fixture.loadListFixture()
        let running = try Self.running()
        let values = fixture.collect(running.id)

        fixture.center.store.receive(
            try ContractFixture.event("frame-event-operation-progress"))
        // An older change is never sent.
        fixture.center.store.receive(.operation(running.with(revision: 3), progress: true))
        fixture.center.store.receive(
            try ContractFixture.event("frame-event-operation-state-changed"))

        let received = await values.value
        #expect(received.map(\.revision) == [4, 5, 7])
        #expect(received.last?.state == .succeeded)
    }

    @Test func aLateListenerGetsTheFinalStateFromRecent() async throws {
        let fixture = Fixture()
        try await fixture.loadListFixture()
        fixture.center.store.receive(
            try ContractFixture.event("frame-event-operation-state-changed"))
        let received = await fixture.collect(try Self.running().id).value
        #expect(received.map(\.state) == [.succeeded])
        #expect(fixture.daemon.calls.contains { $0.methodName == "operation.get" } == false)
    }

    @Test func anOperationTheCenterDoesNotHoldIsFetchedWithOperationGet() async throws {
        let fixture = Fixture()
        guard
            case .operationGet(let detail) = try ContractFixture.reply(
                "frame-response-operation-get")
        else {
            Issue.record("the operation.get fixture holds another reply")
            return
        }
        let stuck = detail.operation
        fixture.daemon.operations[stuck.id] = stuck
        let values = fixture.collect(stuck.id)
        try await waitUntil { fixture.center.store.items[stuck.id] != nil }
        #expect(
            fixture.daemon.calls.last == .operationGet(GetOperationParams(operationId: stuck.id)))

        let failed = stuck.with(state: .failed, revision: stuck.revision + 1)
        fixture.center.store.receive(.operation(failed))
        let received = await values.value
        #expect(received == [stuck, failed])
    }

    @Test func anOperationTheDaemonForgotEndsItsUpdatesWithoutAValue() async {
        let fixture = Fixture()
        let received = await fixture.collect(UUID()).value
        #expect(received.isEmpty)
    }

    @Test func aWatchedOperationThatEndedWhileAwayIsLookedUpAfterTheSnapshot() async throws {
        let fixture = Fixture()
        try await fixture.loadListFixture()
        let running = try Self.running()
        let values = fixture.collect(running.id)

        // The shell reconnects: the operation ended meanwhile, so the snapshot lacks it.
        let succeeded = running.with(state: .succeeded, revision: 7)
        fixture.daemon.operations[running.id] = succeeded
        fixture.daemon.pages = [
            .success(.operationList(OperationPage(items: [try Self.queued()], next: nil)))
        ]
        await fixture.center.store.resnapshot()

        let received = await values.value
        #expect(received == [running, succeeded])
        #expect(fixture.center.recent == [succeeded])
        #expect(fixture.center.runningCount == 0)
    }
}
