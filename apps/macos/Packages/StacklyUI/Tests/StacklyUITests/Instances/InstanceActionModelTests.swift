import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// Start, Stop, and Retry (spec 0022 AC-8): the click, the key, the pending state, and the
/// feedback, with a scripted daemon.
@Suite(.timeLimit(.minutes(1)))
struct InstanceActionModelTests {
    /// What the daemon answers and what it was sent.
    final class Script {
        var sent: [(call: Call, key: String)] = []
        var outcomes: [MutationOutcome] = []
        var read: OperationSnapshot?
        var reads: [UUID] = []
        var streams: [UUID: AsyncStream<OperationSnapshot>.Continuation] = [:]
        var announcements: [String] = []
        var feedback: [Feedback] = []
        var clock = Date(timeIntervalSince1970: 1_000)
        var gate: CheckedContinuation<Void, Never>?
        var holdNextSend = false

        func send(_ call: Call, _ key: String) async -> MutationOutcome {
            sent.append((call, key))
            if holdNextSend {
                holdNextSend = false
                await withCheckedContinuation { gate = $0 }
            }
            let outcome =
                outcomes.isEmpty ? .rejected(Self.error("resource.busy")) : outcomes.removeFirst()
            // An unknown outcome carries the key that was sent, as the mutation path does.
            if case .outcomeUnknown(let body, key: "") = outcome {
                return .outcomeUnknown(body, key: key)
            }
            return outcome
        }

        func open() {
            gate?.resume()
            gate = nil
        }

        func updates(_ id: UUID) -> AsyncStream<OperationSnapshot> {
            let (stream, continuation) = AsyncStream.makeStream(of: OperationSnapshot.self)
            streams[id] = continuation
            return stream
        }

        static func error(_ code: String, message: String = "the instance is busy") -> ErrorBody {
            ErrorBody(
                code: code, category: .conflict, message: message, retryable: false,
                resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                items: nil)
        }
    }

    let script = Script()
    let instance = InstanceSummary.fixture(number: 1, name: "pg17-dev")

    func makeModel() -> InstanceActionModel {
        let script = script
        return InstanceActionModel(
            send: { await script.send($0, $1) },
            updates: { script.updates($0) },
            readOperation: { id in
                script.reads.append(id)
                return script.read
            },
            now: { script.clock },
            announce: { script.announcements.append($0) })
    }

    var context: InstanceActionContext {
        let script = script
        return InstanceActionContext(
            report: { script.feedback.append($0) },
            showOperations: {})
    }

    static func accepted(
        _ operation: OperationSummary, replayed: Bool = false, already: Bool = false
    )
        -> Reply
    {
        .instanceStart(
            InstanceActionAccepted(
                accepted: already
                    ? nil : OperationAccepted(operation: operation, replayed: replayed),
                alreadyInState: already))
    }

    func operation(_ state: OperationState, failure: OperationFailureInfo? = nil)
        -> OperationSummary
    {
        .fixture(
            number: 1, kind: .startInstance,
            resource: ResourceRef(kind: .instance, id: instance.id.uuidString.lowercased()),
            state: state, failure: failure)
    }

    // MARK: - The click

    @Test func aClickSendsInstanceStartWithAFreshKey() async throws {
        let model = makeModel()
        script.outcomes = [.completed(Self.accepted(operation(.succeeded)))]
        let task = try #require(model.perform(.start, for: instance, context: context))
        #expect(model.localStatus(for: instance.id) == .inFlight(.start))
        await task.value
        let sent = try #require(script.sent.first)
        guard case .instanceStart(let params) = sent.call else {
            Issue.record("not instance.start")
            return
        }
        #expect(params.instanceId == instance.id)
        #expect(UUID(uuidString: sent.key) != nil)
        #expect(sent.key == sent.key.lowercased())
    }

    @Test func stopSendsInstanceStopWithoutOverridingLeases() async throws {
        let model = makeModel()
        script.outcomes = [
            .completed(.instanceStop(InstanceActionAccepted(accepted: nil, alreadyInState: true)))
        ]
        let task = try #require(model.perform(.stop, for: instance, context: context))
        await task.value
        guard case .instanceStop(let params) = script.sent.first?.call else {
            Issue.record("not instance.stop")
            return
        }
        #expect(params.overrideLeases == false)
    }

    @Test func aSecondClickWhileTheFirstIsInFlightSendsNothing() async throws {
        let model = makeModel()
        script.holdNextSend = true
        script.outcomes = [.completed(Self.accepted(operation(.succeeded)))]
        let first = try #require(model.perform(.start, for: instance, context: context))
        #expect(model.perform(.start, for: instance, context: context) == nil)
        while script.gate == nil { await Task.yield() }
        script.open()
        await first.value
        #expect(script.sent.count == 1)
    }

    // MARK: - Pending and the end of the operation

    @Test func anAcceptedClickIsAnnouncedAndPendingUntilTheOperationEnds() async throws {
        let model = makeModel()
        let running = operation(.running)
        script.outcomes = [.completed(Self.accepted(running))]
        let task = try #require(model.perform(.start, for: instance, context: context))
        try await waitUntil {
            model.localStatus(for: instance.id) == .pending(.start, operationId: running.id)
        }
        #expect(script.announcements == ["Starting pg17-dev."])
        #expect(model.primaryAction(for: instance).isBusy)
        let continuation = try #require(script.streams[running.id])
        continuation.yield(operation(.succeeded))
        continuation.finish()
        await task.value
        #expect(model.localStatus(for: instance.id) == .none)
        #expect(script.feedback.isEmpty)
    }

    @Test func anOperationThatFailedShowsTheCardWithTheDaemonMessage() async throws {
        let model = makeModel()
        let running = operation(.running)
        script.outcomes = [.completed(Self.accepted(running))]
        let task = try #require(model.perform(.start, for: instance, context: context))
        try await waitUntil { script.streams[running.id] != nil }
        let failure = OperationFailureInfo(
            reason: "step_failed", retryable: true,
            error: Script.error("supervisor.endpoint_unavailable", message: "port 5432 is taken"))
        script.streams[running.id]?.yield(operation(.failed, failure: failure))
        script.streams[running.id]?.finish()
        await task.value
        let card = try #require(script.feedback.first)
        #expect(card.tone == .critical)
        #expect(card.message == "Couldn’t start “pg17-dev”.")
        #expect(card.detail == "Port 5432 is taken.")
        #expect(card.code == "supervisor.endpoint_unavailable")
        #expect(model.localStatus(for: instance.id) == .none)
    }

    @Test func aCancelledOperationShowsANeutralNote() async throws {
        let model = makeModel()
        let running = operation(.running)
        script.outcomes = [.completed(Self.accepted(running))]
        let task = try #require(model.perform(.stop, for: instance, context: context))
        try await waitUntil { script.streams[running.id] != nil }
        script.streams[running.id]?.yield(operation(.cancelled))
        script.streams[running.id]?.finish()
        await task.value
        let card = try #require(script.feedback.first)
        #expect(card.tone == .neutral)
        #expect(card.message == "“pg17-dev” didn’t stop: the operation was cancelled.")
    }

    @Test func aStreamThatEndsWithoutAFinalStateAsksTheDaemon() async throws {
        let model = makeModel()
        let running = operation(.running)
        script.outcomes = [.completed(Self.accepted(running))]
        script.read = operation(.succeeded)
        let task = try #require(model.perform(.start, for: instance, context: context))
        try await waitUntil { script.streams[running.id] != nil }
        script.streams[running.id]?.finish()
        await task.value
        #expect(script.reads == [running.id])
        #expect(script.feedback.isEmpty)
        #expect(model.localStatus(for: instance.id) == .none)
    }

    @Test func aResnapshotThatFindsTheOperationGoneEndsThePendingClick() async throws {
        let model = makeModel()
        let running = operation(.running)
        script.outcomes = [.completed(Self.accepted(running))]
        _ = try #require(model.perform(.start, for: instance, context: context))
        try await waitUntil { script.streams[running.id] != nil }
        script.read = operation(.cancelled)
        await model.reconcile(
            activeOperations: [], names: [instance.id: "pg17-dev"], context: context)
        #expect(script.reads == [running.id])
        #expect(model.localStatus(for: instance.id) == .none)
        #expect(script.feedback.first?.tone == .neutral)
    }

    @Test func aResnapshotThatStillHasTheOperationLeavesTheClickPending() async throws {
        let model = makeModel()
        let running = operation(.running)
        script.outcomes = [.completed(Self.accepted(running))]
        _ = try #require(model.perform(.start, for: instance, context: context))
        try await waitUntil { script.streams[running.id] != nil }
        await model.reconcile(
            activeOperations: [running.id], names: [:], context: context)
        #expect(script.reads.isEmpty)
        #expect(model.localStatus(for: instance.id) == .pending(.start, operationId: running.id))
    }

    // MARK: - Replies without an operation

    @Test func alreadyInTheAskedStateShowsNothing() async throws {
        let model = makeModel()
        script.outcomes = [.completed(Self.accepted(operation(.succeeded), already: true))]
        let task = try #require(model.perform(.start, for: instance, context: context))
        await task.value
        #expect(script.feedback.isEmpty)
        #expect(script.announcements.isEmpty)
        #expect(model.localStatus(for: instance.id) == .none)
    }

    @Test func aRefusedClickNamesTheInstanceAndKeepsTheDaemonWords() async throws {
        let model = makeModel()
        script.outcomes = [
            .rejected(
                Script.error("supervisor.data_in_use", message: "another process holds the data"))
        ]
        let task = try #require(model.perform(.start, for: instance, context: context))
        await task.value
        let card = try #require(script.feedback.first)
        #expect(card.message == "Couldn’t start “pg17-dev”.")
        #expect(card.detail == "Another process holds the data.")
        #expect(card.code == "supervisor.data_in_use")
        #expect(model.localStatus(for: instance.id) == .none)
    }

    @Test func aClickThatWasNotSentSaysNothingWasSent() async throws {
        let model = makeModel()
        script.outcomes = [.notSent(Script.error("ipc.not_connected"))]
        let task = try #require(model.perform(.start, for: instance, context: context))
        await task.value
        #expect(script.feedback.first?.message == MutationFeedback.notSent)
        #expect(model.localStatus(for: instance.id) == .none)
    }

    @Test func aClickWithAnUnknownOutcomeOffersShowOperations() async throws {
        let model = makeModel()
        script.outcomes = [.outcomeUnknown(Script.error("ipc.connection_closed"), key: "k1")]
        let task = try #require(model.perform(.start, for: instance, context: context))
        await task.value
        let card = try #require(script.feedback.first)
        #expect(card.message == MutationFeedback.outcomeUnknown)
        #expect(card.action?.title == "Show Operations")
    }

    // MARK: - Keys

    /// An unknown outcome; the script fills in the key that was sent.
    private func unknown(_ ignored: String = "") -> MutationOutcome {
        .outcomeUnknown(Script.error("ipc.connection_closed"), key: "")
    }

    @Test func theKeyOfAnUnknownOutcomeIsReusedForTheSameActionOnTheSameRevision() async throws {
        let model = makeModel()
        script.outcomes = [unknown("ignored"), .rejected(Script.error("resource.busy"))]
        await model.perform(.start, for: instance, context: context)?.value
        script.clock = script.clock.addingTimeInterval(120)
        await model.perform(.start, for: instance, context: context)?.value
        #expect(script.sent.count == 2)
        #expect(script.sent[1].key == script.sent[0].key)
    }

    @Test func theKeyIsDroppedAfterADefiniteReply() async throws {
        let model = makeModel()
        script.outcomes = [
            unknown("x"), .rejected(Script.error("resource.busy")),
            .rejected(Script.error("resource.busy")),
        ]
        await model.perform(.start, for: instance, context: context)?.value
        await model.perform(.start, for: instance, context: context)?.value
        await model.perform(.start, for: instance, context: context)?.value
        #expect(script.sent[1].key == script.sent[0].key)
        #expect(script.sent[2].key != script.sent[1].key)
    }

    @Test func aNewKeyComesWhenTheRevisionMovedOrFiveMinutesPassed() async throws {
        let model = makeModel()
        script.outcomes = [unknown("a"), unknown("b"), unknown("c")]
        await model.perform(.start, for: instance, context: context)?.value
        // The row changed: the earlier outcome says nothing about this click.
        let newer = InstanceSummary(
            id: instance.id, name: instance.name, displayName: nil, revision: instance.revision + 1,
            software: instance.software, provider: instance.provider, runtimeId: instance.runtimeId,
            runtimeVersion: instance.runtimeVersion, lifecycle: instance.lifecycle,
            desired: instance.desired, readiness: instance.readiness, health: instance.health,
            statusSeq: instance.statusSeq, process: nil, lastFailure: nil,
            restart: instance.restart,
            blocker: nil, endpoints: nil, leftoverProcesses: nil, logsUnavailable: false,
            ownership: .manual, initialized: true)
        await model.perform(.start, for: newer, context: context)?.value
        script.clock = script.clock.addingTimeInterval(301)
        await model.perform(.start, for: newer, context: context)?.value
        #expect(script.sent.count == 3)
        #expect(Set(script.sent.map(\.key)).count == 3)
    }

    @Test func aKeyIsKeptPerInstanceAndAction() async throws {
        let model = makeModel()
        script.outcomes = [unknown("a"), .rejected(Script.error("resource.busy"))]
        await model.perform(.start, for: instance, context: context)?.value
        await model.perform(.stop, for: instance, context: context)?.value
        #expect(script.sent[1].key != script.sent[0].key)
    }

    // MARK: - The StacklyAction

    @Test func theStacklyActionMirrorsTheMainAction() {
        let model = makeModel()
        let start = model.stacklyAction(for: instance, context: context)
        #expect(start.id == "start")
        #expect(start.title == "Start")
        #expect(start.mutates)
        #expect(start.disabledReason == nil)
        #expect(start.isBusy == false)

        let unknown = InstanceSummary.fixture(number: 2, name: "u", lifecycle: .unknown)
        let stop = model.stacklyAction(for: unknown, context: context)
        #expect(stop.id == "stop")
        #expect(stop.disabledReason == "Stackly can’t confirm this process.")

        let busy = InstanceSummary.fixture(number: 3, name: "b", lifecycle: .starting)
        let starting = model.stacklyAction(for: busy, context: context)
        #expect(starting.isBusy)
        #expect(starting.title == "Starting…")
        #expect(starting.id == "start")
    }
}
