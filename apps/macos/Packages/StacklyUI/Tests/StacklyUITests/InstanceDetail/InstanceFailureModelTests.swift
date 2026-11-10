import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

@testable import StacklyUI

/// The operations the failure banner reads (spec 0033, *Sumber data*): one
/// `operation.list { resource, limit: 10 }` on open, read again on a change of the summary or
/// an operation that ended, the center's entries in between, and the summary alone until a
/// first read succeeded.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceFailureModelTests {
    /// A scripted `operation.list`: it answers from values a test changes and records what it
    /// was asked.
    nonisolated final class Script: Sendable {
        private struct State {
            var answer: Result<[OperationSummary], ClientError>
            var calls: [(id: UUID, limit: UInt32)] = []
        }

        private let state: Mutex<State>

        init(_ operations: [OperationSummary]) {
            state = Mutex(State(answer: .success(operations)))
        }

        var calls: [(id: UUID, limit: UInt32)] { state.withLock { $0.calls } }

        func answer(_ result: Result<[OperationSummary], ClientError>) {
            state.withLock { $0.answer = result }
        }

        var read: @Sendable (UUID, UInt32) async throws(ClientError) -> [OperationSummary] {
            { [self] id, limit throws(ClientError) in
                try state.withLock { state in
                    state.calls.append((id, limit))
                    return state.answer
                }
                .get()
            }
        }
    }

    private let fixture = FixtureFailureBanner.portInUse

    private func model(_ script: Script) -> InstanceFailureModel {
        InstanceFailureModel(instanceId: fixture.id, read: script.read)
    }

    private func content(_ model: InstanceFailureModel, center: [OperationSummary] = [])
        -> FailureBannerContent?
    {
        model.content(
            for: fixture.instance, center: center, instanceNames: { _ in nil },
            now: FixtureClock.now)
    }

    private static let offline = ClientError.notSent(ClientErrorCatalog.notConnected())

    // MARK: - Reading

    @Test(
        .evidence(
            covers: ["AC-010"],
            expected:
                "The banner reads the ten newest operations of the instance once and rises from the summary to Use Port without changing its title"
        ))
    func openingReadsOnceAndTheBannerRises() async throws {
        let script = Script(fixture.operations)
        let model = model(script)
        let before = try #require(content(model))
        #expect(before.actions.first == .changePort)
        #expect(before.operationId == nil)

        await model.open()
        await model.open()
        #expect(script.calls.count == 1)
        #expect(script.calls.first?.id == fixture.id)
        #expect(script.calls.first?.limit == 10)

        let after = try #require(content(model))
        #expect(after.title == before.title)
        #expect(after.actions.first == .usePort(PortFix(endpoint: "database", port: 5439)))
        #expect(after.operationId != nil)
    }

    @Test(
        .evidence(
            covers: ["VAL-020"],
            expected:
                "While the first read fails or the daemon is offline the banner shows the summary alone, and a later failed read keeps what was read"
        ))
    func aFailedReadShowsTheSummaryAloneAndKeepsWhatWasRead() async throws {
        let script = Script([])
        script.answer(.failure(Self.offline))
        let model = model(script)
        await model.open()
        #expect(model.loaded == nil)
        #expect(model.readError?.code == ClientErrorCatalog.notConnected().code)
        #expect(content(model)?.operationId == nil)

        script.answer(.success(fixture.operations))
        await model.reload()
        #expect(model.readError == nil)
        #expect(content(model)?.operationId != nil)

        script.answer(.failure(Self.offline))
        await model.reload()
        #expect(model.readError != nil)
        #expect(content(model)?.operationId != nil)
    }

    // MARK: - Following

    @Test func aChangeOfRevisionOrStatusReadsAgain() async throws {
        let script = Script(fixture.operations)
        let model = model(script)
        await model.open()
        let summary = fixture.instance
        model.observe(summary)
        model.observe(summary)
        await model.settle()
        #expect(script.calls.count == 1)

        model.observe(summary.testing(statusSeq: summary.statusSeq + 1))
        await model.settle()
        #expect(script.calls.count == 2)

        model.observe(
            summary.testing(revision: summary.revision + 1, statusSeq: summary.statusSeq + 1))
        await model.settle()
        #expect(script.calls.count == 3)
    }

    @Test func aRunningInstanceDoesNotReadOnEveryStatusChange() async throws {
        let script = Script([])
        let model = model(script)
        await model.open()
        let running = fixture.instance.testing(lifecycle: .running)
        model.observe(running)
        model.observe(running.testing(statusSeq: running.statusSeq + 1))
        await model.settle()
        #expect(script.calls.count == 1)
    }

    @Test func aChangeBeforeTheBannerOpenedReadsNothing() async throws {
        let script = Script([])
        let model = model(script)
        model.observe(fixture.instance)
        model.observe(fixture.instance.testing(statusSeq: 9))
        await model.settle()
        #expect(script.calls.isEmpty)
    }

    @Test(
        .evidence(
            covers: ["VAL-020"],
            expected:
                "An operation of the instance that ends in the operation center makes the banner read again; one it already holds or of another instance does not"
        ))
    func anOperationThatEndedReadsAgain() async throws {
        let script = Script(fixture.operations)
        let model = model(script)
        await model.open()
        let held = try #require(fixture.operations.first)

        model.observe(center: [held])
        await model.settle()
        #expect(script.calls.count == 1)

        let other = FixtureFailureBanner.portConflict.operations[0]
        model.observe(center: [other])
        await model.settle()
        #expect(script.calls.count == 1)

        let stop = Self.operation(92, .stopInstance, .succeeded, on: fixture.id)
        model.observe(center: [stop.with(state: .running, revision: 1)])
        await model.settle()
        #expect(script.calls.count == 1)

        model.observe(center: [stop])
        await model.settle()
        #expect(script.calls.count == 2)
        model.observe(center: [stop])
        await model.settle()
        #expect(script.calls.count == 2)
    }

    @Test(
        .evidence(
            covers: ["VAL-020"],
            expected:
                "A newer operation of the instance in the operation center counts before the next read, so a stop hides a refused start at once"
        ))
    func theCenterReplacesWhatWasRead() async throws {
        let refused = FixtureFailureBanner.startRefused
        let script = Script(refused.operations)
        let model = InstanceFailureModel(instanceId: refused.id, read: script.read)
        await model.open()
        let names: (UUID) -> String? = { _ in nil }
        #expect(
            model.content(
                for: refused.instance, center: [], instanceNames: names, now: FixtureClock.now)?
                .rule == .startRefused)

        let stop = Self.operation(93, .stopInstance, .running, on: refused.id)
        #expect(
            model.content(
                for: refused.instance, center: [stop], instanceNames: names,
                now: FixtureClock.now) == nil)
        #expect(model.operations(center: [stop]).first?.id == stop.id)
    }

    @Test func aCenterEntryWithAHigherRevisionReplacesTheReadOne() async throws {
        let script = Script(fixture.operations)
        let model = model(script)
        await model.open()
        let read = try #require(fixture.operations.first)
        let newer = read.with(revision: read.revision + 1)
        #expect(model.operations(center: [newer]).map(\.revision) == [newer.revision])
        let older = read.with(revision: read.revision - 1)
        #expect(model.operations(center: [older]).map(\.revision) == [read.revision])
    }

    @Test func theRegistryKeepsOneModelPerInstanceAndDropsGoneOnes() {
        let models = InstanceDetailModels(service: .disconnected)
        let first = models.failureModel(for: fixture.id)
        #expect(models.failureModel(for: fixture.id) === first)
        models.keep([])
        #expect(models.failureModel(for: fixture.id) !== first)
    }

    // MARK: - Pieces

    private static func operation(
        _ number: UInt8, _ kind: OperationKind, _ state: OperationState, on id: UUID
    ) -> OperationSummary {
        let at = FixtureClock.secondsAgo(3)
        return OperationSummary(
            id: FixtureID.uuid(kind: FixtureID.operation, number: number), kind: kind,
            resource: ResourceRef(kind: .instance, id: id.uuidString.lowercased()), state: state,
            stage: nil, progress: .indeterminate, waitingOn: nil, cancelRequestedAt: nil,
            pointOfNoReturnAt: nil, failure: nil, recovery: nil, result: nil,
            trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: at,
            startedAt: at, updatedAt: at, revision: 2, cancel: nil)
    }
}
