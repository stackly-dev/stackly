import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// Turn Off… against a scripted daemon (spec 0031, AC-19, AC-20): the agent is unregistered
/// only when the daemon says it exits, and a daemon that stays up leaves it registered with a
/// sentence that names what could not be stopped.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct TurnOffModelTests {
    private let held = UUID()

    final class Script: Sendable {
        let outcomes = Mutex<[TurnOffOutcome]>([])
        let keys = Mutex<[String]>([])
        let unregistered = Mutex<Int>(0)
        let preview: StopAllPreview

        init(preview: StopAllPreview, outcomes: [TurnOffOutcome]) {
            self.preview = preview
            self.outcomes.withLock { $0 = outcomes }
        }

        @MainActor func model(names: [UUID: String] = [:]) -> TurnOffModel {
            TurnOffModel(
                preview: { () throws(ClientError) in self.preview },
                atLoginCount: { 3 },
                names: { names },
                agentOff: { key in
                    self.keys.withLock { $0.append(key) }
                    return self.outcomes.withLock { $0.count > 1 ? $0.removeFirst() : $0[0] }
                },
                unregister: { self.unregistered.withLock { $0 += 1 } })
        }
    }

    private static func preview(skipped: [StopAllSkip] = []) -> StopAllPreview {
        StopAllPreview(
            items: [], alreadyStopping: [], skipped: skipped, atLoginCount: 0,
            affectedEnvironments: nil)
    }

    private static func accepted(
        exiting: Bool, result: StopAllResult = StopAllResult(stopped: [], failed: [], skipped: [])
    ) -> TurnOffOutcome {
        .accepted(
            ShutdownAccepted(
                serverInstanceId: UUID(), mode: .agentOff, planId: nil, exiting: exiting,
                stopAll: result))
    }

    private static func refusal() -> ErrorBody {
        ErrorBody(
            code: "daemon.restart_pending", category: .conflict, message: "a restart is pending",
            retryable: true, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
    }

    @Test func beginReadsThePreviewAndOpensThePlan() async {
        let model = Script(preview: Self.preview(), outcomes: [Self.accepted(exiting: true)])
            .model()
        await model.begin()
        #expect(model.plan?.canTurnOff == true)
        #expect(model.plan?.title == "Turn Off Background Agent?")
        #expect(model.phase == .idle)
    }

    @Test func theConfirmationBelongsToTheWindowThatAskedForIt() async {
        let model = Script(preview: Self.preview(), outcomes: [Self.accepted(exiting: true)])
            .model()
        await model.begin()
        #expect(model.origin == .diagnostics)
        model.cancel()
        await model.begin(from: .settings)
        #expect(model.origin == .settings)
        #expect(model.plan != nil)
    }

    @Test func aDaemonThatExitsIsFollowedByTheUnregister() async {
        let script = Script(preview: Self.preview(), outcomes: [Self.accepted(exiting: true)])
        let model = script.model()
        await model.begin()
        await model.confirm()
        #expect(script.keys.withLock { $0.count } == 1)
        #expect(script.unregistered.withLock { $0 } == 1)
        #expect(model.message == nil)
        #expect(model.plan == nil)
    }

    @Test func aDaemonThatStaysUpLeavesTheAgentAndNamesWhatBlocked() async {
        let result = StopAllResult(
            stopped: [UUID()], failed: [],
            skipped: [StopAllEntry(instanceId: held, code: "supervisor.stop_requires_override")])
        let script = Script(
            preview: Self.preview(), outcomes: [Self.accepted(exiting: false, result: result)])
        let model = script.model(names: [held: "held"])
        await model.begin()
        await model.confirm()
        #expect(script.unregistered.withLock { $0 } == 0)
        #expect(
            model.message
                == "Background agent is still on. Stopped 1 instances. These couldn’t be stopped: held · an environment uses it."
        )
    }

    @Test func aSkippedInstanceBlocksTheConfirmationBeforeAnythingIsSent() async {
        let skip = StopAllSkip(
            instanceId: held, name: "held", code: "supervisor.stop_requires_override",
            environments: nil)
        let script = Script(
            preview: Self.preview(skipped: [skip]), outcomes: [Self.accepted(exiting: true)])
        let model = script.model()
        await model.begin()
        #expect(model.plan?.canTurnOff == false)
        await model.confirm()
        #expect(script.keys.withLock { $0.isEmpty })
        #expect(script.unregistered.withLock { $0 } == 0)
    }

    @Test func aRefusalChangesNothingAndShowsTheReason() async {
        let script = Script(preview: Self.preview(), outcomes: [.rejected(Self.refusal())])
        let model = script.model()
        await model.begin()
        await model.confirm()
        #expect(model.message == "a restart is pending")
        #expect(script.unregistered.withLock { $0 } == 0)
    }

    @Test func aLostAnswerRetriesWithTheSameKey() async {
        let script = Script(
            preview: Self.preview(),
            outcomes: [.outcomeUnknown(Self.refusal()), Self.accepted(exiting: true)])
        let model = script.model()
        await model.begin()
        await model.confirm()
        await model.begin()
        await model.confirm()
        let keys = script.keys.withLock { $0 }
        #expect(keys.count == 2)
        #expect(keys[0] == keys[1])
        #expect(script.unregistered.withLock { $0 } == 1)
    }
}
