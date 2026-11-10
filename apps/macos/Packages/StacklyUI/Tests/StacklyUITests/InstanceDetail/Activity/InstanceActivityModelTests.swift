import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The Activity tab model (spec 0035, AC-15): the first page and Show More of
/// `audit.list { instanceId }`, the loads after a state change of the instance's operations
/// (gathered over 500 ms) and after a snapshot round, the placeholders of an older daemon and
/// of a failed load, and the operation a row opens (AC-13).
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceActivityModelTests {
    /// A scripted `audit.list`: the pages by cursor, the calls it saw, and a failure for the
    /// next call.
    nonisolated final class Daemon: Sendable {
        private struct State {
            var pages: [String?: AuditPage] = [:]
            var calls: [(UUID, String?)] = []
            var failNext: ClientError?
        }

        private let state = Mutex(State())

        /// The cursors of the calls, in order.
        var cursors: [String?] { state.withLock { $0.calls.map(\.1) } }
        /// The instances of the calls, in order.
        var instances: [UUID] { state.withLock { $0.calls.map(\.0) } }

        func set(_ page: AuditPage, for cursor: String? = nil) {
            state.withLock { $0.pages[cursor] = page }
        }

        func fail(next error: ClientError) { state.withLock { $0.failNext = error } }

        var read: @Sendable (UUID, String?) async throws(ClientError) -> AuditPage {
            { [self] id, cursor throws(ClientError) in
                let answer = state.withLock { state -> Result<AuditPage, ClientError> in
                    state.calls.append((id, cursor))
                    if let failure = state.failNext {
                        state.failNext = nil
                        return .failure(failure)
                    }
                    return .success(state.pages[cursor] ?? AuditPage(items: [], next: nil))
                }
                return try answer.get()
            }
        }
    }

    /// The waits the model asked for.
    nonisolated final class Sleeps: Sendable {
        private let durations = Mutex<[Duration]>([])

        var all: [Duration] { durations.withLock { $0 } }

        var sleep: @Sendable (Duration) async throws -> Void {
            { [self] duration in durations.withLock { $0.append(duration) } }
        }
    }

    private let daemon = Daemon()
    private let sleeps = Sleeps()

    private func model() -> InstanceActivityModel {
        InstanceActivityModel(
            instanceId: ActivitySample.instance, read: daemon.read,
            now: { ActivitySample.base }, sleep: sleeps.sleep)
    }

    private func rows(_ numbers: ClosedRange<UInt8>) -> [AuditEventInfo] {
        numbers.reversed().map { ActivitySample.row($0) }
    }

    private func operation(
        _ number: UInt8, state: OperationState, instance: UUID = ActivitySample.instance,
        revision: UInt64 = 1
    ) -> OperationSummary {
        OperationSummary(
            id: ActivitySample.operation(number), kind: .startInstance,
            resource: ResourceRef(kind: .instance, id: instance.uuidString.lowercased()),
            state: state, stage: nil, progress: .indeterminate, waitingOn: nil,
            cancelRequestedAt: nil, pointOfNoReturnAt: nil, failure: nil, recovery: nil,
            result: nil, trigger: OperationTriggerInfo(origin: .user, client: .gui),
            createdAt: ActivitySample.base, startedAt: ActivitySample.base,
            updatedAt: ActivitySample.base, revision: revision, cancel: nil)
    }

    private func opened(changes: AsyncStream<OperationCenterChange>? = nil) async
        -> InstanceActivityModel
    {
        let model = model()
        model.open(changes: changes)
        await settle("the first page") {
            model.loadedAt != nil || model.error != nil || model.unsupported
        }
        return model
    }

    // MARK: - Pages

    @Test func openingLoadsTheFirstPageOfTheInstance() async {
        daemon.set(ActivitySample.page(rows(1...3)))
        let model = await opened()
        #expect(daemon.instances == [ActivitySample.instance])
        #expect(daemon.cursors == [nil])
        #expect(model.items.map(\.id) == [3, 2, 1].map(ActivitySample.id))
        #expect(model.next == nil)
        #expect(model.loadedAt == ActivitySample.base)
        #expect(InstanceActivityTab.display(of: model) == .rows)
        #expect(InstanceActivityModel.pageSize == 50)
    }

    @Test func showMoreAddsThePageAfterTheCursorUntilTheLastOne() async {
        daemon.set(ActivitySample.page(rows(4...6), next: "c1"))
        daemon.set(ActivitySample.page(rows(2...3), next: "c2"), for: "c1")
        daemon.set(ActivitySample.page(rows(1...1)), for: "c2")
        let model = await opened()
        #expect(model.next == "c1")
        await model.showMore()
        #expect(model.items.map(\.id) == [6, 5, 4, 3, 2].map(ActivitySample.id))
        #expect(model.next == "c2")
        await model.showMore()
        #expect(model.items.count == 6)
        #expect(model.next == nil)
        await model.showMore()
        #expect(daemon.cursors == [nil, "c1", "c2"])
    }

    @Test func aNewFirstPageKeepsTheRowsShowMoreLoadedWhereItMeetsThem() async {
        daemon.set(ActivitySample.page(rows(3...4), next: "c1"))
        daemon.set(ActivitySample.page(rows(1...2)), for: "c1")
        let model = await opened()
        await model.showMore()
        #expect(model.next == nil)

        daemon.set(ActivitySample.page(rows(4...5), next: "c3"))
        await model.reload()
        #expect(model.items.map(\.id) == [5, 4, 3, 2, 1].map(ActivitySample.id))
        #expect(model.next == nil)
    }

    @Test func aNewFirstPageThatDoesNotReachTheLoadedRowsReplacesThem() async {
        daemon.set(ActivitySample.page(rows(1...2), next: "c1"))
        let model = await opened()
        daemon.set(ActivitySample.page(rows(8...9), next: "c9"))
        await model.reload()
        #expect(model.items.map(\.id) == [9, 8].map(ActivitySample.id))
        #expect(model.next == "c9")
    }

    @Test func aFailedShowMoreKeepsTheRowsAndSaysWhy() async {
        daemon.set(ActivitySample.page(rows(2...3), next: "c1"))
        let model = await opened()
        daemon.fail(next: .rejected(.create("internal.unexpected", "the registry is busy")))
        await model.showMore()
        #expect(model.items.count == 2)
        #expect(model.next == "c1")
        #expect(model.moreError?.message == "the registry is busy")
    }

    // MARK: - States

    @Test func anInstanceWithoutRowsShowsTheEmptyPlaceholder() async {
        let model = await opened()
        #expect(InstanceActivityTab.display(of: model) == .empty)
        #expect(ActivityCopy.emptyTitle == "No Activity Yet")
    }

    @Test func aFirstLoadThatFailsShowsTheFailureAndRetryLoadsAgain() async {
        let refusal = ErrorBody.create("internal.unexpected", "the registry could not be read")
        daemon.fail(next: .rejected(refusal))
        let model = await opened()
        #expect(InstanceActivityTab.display(of: model) == .failed(refusal))
        #expect(ActivityCopy.failedTitle == "Couldn’t Load Activity")

        daemon.set(ActivitySample.page(rows(1...1)))
        await model.reload()
        #expect(InstanceActivityTab.display(of: model) == .rows)
        #expect(model.error == nil)
    }

    @Test func aLaterFailureKeepsTheRowsWithTheError() async {
        daemon.set(ActivitySample.page(rows(1...2)))
        let model = await opened()
        daemon.fail(next: .notSent(ClientErrorCatalog.notConnected()))
        await model.reload()
        #expect(InstanceActivityTab.display(of: model) == .rows)
        #expect(model.items.count == 2)
        #expect(model.error != nil)
    }

    @Test func anOlderDaemonWithoutAuditListShowsActivityUnavailable() async {
        daemon.fail(next: .rejected(.create("request.unknown_method", "unknown method")))
        let model = await opened()
        #expect(model.unsupported)
        #expect(model.error == nil)
        #expect(InstanceActivityTab.display(of: model) == .unsupported)
        #expect(ActivityCopy.unsupportedTitle == "Activity Unavailable")
    }

    // MARK: - Refreshing

    @Test func aStateChangeOfAnOperationOfTheInstanceLoadsAgainOnceForEveryChangeIn500ms()
        async
    {
        let model = await opened()
        model.receive(operation(1, state: .queued))
        model.receive(operation(1, state: .running))
        model.receive(operation(2, state: .running))
        await settle("the second load") { daemon.cursors.count == 2 }
        #expect(sleeps.all == [.milliseconds(500)])
        for _ in 0..<20 { await Task.yield() }
        #expect(daemon.cursors == [nil, nil])
    }

    @Test func aProgressEventOrAnotherInstanceDoesNotLoadAgain() async {
        let model = await opened()
        model.receive(operation(1, state: .running))
        await settle("the load after the first state") { daemon.cursors.count == 2 }
        model.receive(operation(1, state: .running, revision: 2))
        model.receive(
            operation(
                3, state: .running,
                instance: UUID(uuidString: "0192f3a5-0000-7000-8000-000000000999") ?? UUID()))
        for _ in 0..<20 { await Task.yield() }
        #expect(daemon.cursors.count == 2)
        #expect(sleeps.all.count == 1)
    }

    @Test func theChangesOfTheCenterReachTheModelUntilTheTabCloses() async {
        let (changes, continuation) = AsyncStream.makeStream(of: OperationCenterChange.self)
        let model = await opened(changes: changes)
        continuation.yield(.reloaded)
        continuation.yield(.operation(operation(1, state: .succeeded)))
        await settle("the load after the end") { daemon.cursors.count == 2 }

        model.close()
        continuation.yield(.operation(operation(2, state: .running)))
        for _ in 0..<20 { await Task.yield() }
        #expect(daemon.cursors.count == 2)
    }

    @Test func aSnapshotRoundLoadsAgainOnlyWhileTheTabShows() async {
        let model = await opened()
        model.reconnected()
        await settle("the load after the round") { daemon.cursors.count == 2 }
        model.close()
        model.reconnected()
        for _ in 0..<20 { await Task.yield() }
        #expect(daemon.cursors.count == 2)
    }

    @Test func aSecondViewerDoesNotLoadTwice() async {
        let model = await opened()
        model.open(changes: nil)
        for _ in 0..<20 { await Task.yield() }
        #expect(daemon.cursors.count == 1)
        model.close()
        model.reconnected()
        await settle("the load of the one left") { daemon.cursors.count == 2 }
    }

    // MARK: - Navigation

    @Test func aRowWithAnOperationOpensItAndASecretRowOpensNothing() async {
        let secret = ActivitySample.row(
            1, action: "secret.reveal", details: ["purpose": "show", "reference": "a/b"])
        daemon.set(ActivitySample.page([ActivitySample.row(2), secret]))
        let model = await opened()
        #expect(model.operation(for: [ActivitySample.id(2)]) == ActivitySample.operation(2))
        #expect(model.operation(for: [ActivitySample.id(1)]) == nil)
        #expect(model.operation(for: []) == nil)
    }
}
