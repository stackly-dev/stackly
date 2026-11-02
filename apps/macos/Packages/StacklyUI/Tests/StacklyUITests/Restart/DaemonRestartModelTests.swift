import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The Restart Daemon model against a scripted daemon (spec 0030 AC-16): the sheet and its
/// refusals, the plan that arrives by event or by the handshake, the row marks, and the toast.
@Suite(.timeLimit(.minutes(1)))
struct DaemonRestartModelTests {
    private typealias F = FixtureRestart

    /// A scripted daemon: the preview it answers and the outcomes of the restarts, in order.
    final class Script: Sendable {
        let previews = Mutex<[Result<RestartPreview, ClientError>]>([])
        let outcomes = Mutex<[RestartOutcome]>([])
        let sent = Mutex<[(digest: String, force: Bool, key: String)]>([])

        init(previews: [Result<RestartPreview, ClientError>], outcomes: [RestartOutcome] = []) {
            self.previews.withLock { $0 = previews }
            self.outcomes.withLock { $0 = outcomes }
        }

        func model() -> DaemonRestartModel {
            DaemonRestartModel(
                preview: { () throws(ClientError) in
                    try self.previews.withLock { $0.count > 1 ? $0.removeFirst() : $0[0] }.get()
                },
                restart: { digest, force, key in
                    self.sent.withLock { $0.append((digest, force, key)) }
                    return self.outcomes.withLock { $0.removeFirst() }
                },
                now: { FixtureClock.now })
        }
    }

    private static let accepted = RestartOutcome.accepted(
        ShutdownAccepted(serverInstanceId: UUID(), mode: .restart, planId: F.planID))

    static func refusal(_ code: String) -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: "the plan changed", retryable: false,
            resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
            items: nil)
    }

    private static func event(_ plan: RestartPlanView, sequence: UInt64) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: FixtureClock.now, resource: nil,
            revision: nil, kind: .daemonRestartPlan(plan))
    }

    // MARK: - The sheet

    @Test func beginReadsThePlanAndOpensTheSheetOverIt() async {
        let model = Script(previews: [.success(F.preview())]).model()
        await model.begin()
        #expect(model.confirmation?.preview == F.preview())
        #expect(model.confirmation?.isWorking == false)
        #expect(model.previewError == nil)
    }

    @Test func aPreviewThatFailsLeavesTheSheetClosedAndKeepsTheError() async {
        let body = Self.refusal("daemon.shutting_down")
        let model = Script(previews: [.failure(.rejected(body))]).model()
        await model.begin()
        #expect(model.confirmation == nil)
        #expect(model.previewError == body)
        #expect(model.isLoadingPreview == false)
    }

    @Test func confirmingSendsTheShownDigestWithAFreshKeyAndClosesTheSheet() async throws {
        let script = Script(
            previews: [.success(F.preview(digest: "abc"))], outcomes: [Self.accepted])
        let model = script.model()
        await model.begin()
        await model.confirm()
        let sent = try #require(script.sent.withLock { $0 }.first)
        #expect(sent.digest == "abc")
        #expect(sent.force == false)
        #expect(sent.key == sent.key.lowercased() && UUID(uuidString: sent.key) != nil)
        #expect(model.confirmation == nil)
        #expect(model.isAwaitingPlan)
        #expect(model.isRestarting)
    }

    @Test func aChangedPlanReloadsThePreviewInTheSameSheet() async {
        let script = Script(
            previews: [.success(F.preview(digest: "old")), .success(F.preview(digest: "new"))],
            outcomes: [.rejected(Self.refusal("daemon.restart_plan_changed"))])
        let model = script.model()
        await model.begin()
        await model.confirm()
        #expect(model.confirmation?.preview.digest == "new")
        #expect(model.confirmation?.planChanged == true)
        #expect(model.confirmation?.isWorking == false)
        #expect(model.isAwaitingPlan == false)
    }

    @Test func aBusyDaemonKeepsTheSheetOpenWithTheBlockingOperations() async {
        let blocking = [
            BlockingOperation(operationId: UUID(), kind: .createInstance, resumable: false)
        ]
        let busy = Self.refusal("daemon.restart_busy")
        let script = Script(
            previews: [.success(F.preview()), .success(F.preview(blocking: blocking))],
            outcomes: [.rejected(busy)])
        let model = script.model()
        await model.begin()
        await model.confirm()
        #expect(model.confirmation?.error == busy)
        #expect(model.confirmation?.preview.blockingOperations == blocking)
        #expect(model.confirmation?.isWorking == false)
    }

    @Test func aLostAnswerKeepsTheSheetAndRetriesWithTheSameKey() async throws {
        let lost = Self.refusal("ipc.outcome_unknown")
        let script = Script(
            previews: [.success(F.preview(digest: "abc"))],
            outcomes: [.outcomeUnknown(lost), Self.accepted])
        let model = script.model()
        await model.begin()
        await model.confirm()
        #expect(model.confirmation?.error == lost)
        await model.confirm()
        let sent = script.sent.withLock { $0 }
        #expect(sent.count == 2)
        #expect(sent[0].key == sent[1].key)
        #expect(model.confirmation == nil)
    }

    @Test func nothingSentKeepsTheSheetWithTheReason() async {
        let down = Self.refusal("ipc.not_connected")
        let model = Script(previews: [.success(F.preview())], outcomes: [.notSent(down)]).model()
        await model.begin()
        await model.confirm()
        #expect(model.confirmation?.error == down)
    }

    @Test func cancelClosesTheSheetWithoutSendingAnything() async {
        let script = Script(previews: [.success(F.preview())])
        let model = script.model()
        await model.begin()
        model.cancel()
        #expect(model.confirmation == nil)
        #expect(script.sent.withLock { $0 }.isEmpty)
    }

    // MARK: - The plan

    @Test func aPlanEventDrivesTheBannerTheMarksAndTheButton() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.handedOver, instances: F.mixedInstances), sequence: 1))
        #expect(model.isRestarting)
        #expect(model.banner(now: FixtureClock.now)?.title == "stacklyd is restarting")
        #expect(
            model.markedInstances == [FixtureID.uuid(kind: FixtureID.instance, number: 3)])
    }

    @Test func aLateEventNeverMovesAPlanBack() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.restoring, instances: F.mixedInstances), sequence: 2))
        model.receive(
            event: Self.event(F.plan(.stopping, instances: F.mixedInstances), sequence: 1))
        #expect(model.plan?.state == .restoring)
    }

    @Test func aMarkLeavesWhenTheInstanceHasItsOutcome() {
        let model = Script(previews: [.success(F.preview())]).model()
        let restored = [
            F.instance(1, "pg17-dev", outcome: .adopted),
            F.instance(
                3, "valkey-cache", decision: .stopRestore, stopped: true, outcome: .restored),
        ]
        model.receive(event: Self.event(F.plan(.restoring, instances: restored), sequence: 1))
        #expect(model.markedInstances.isEmpty)
    }

    @Test func anInstanceThatHasNotStoppedYetIsNotMarked() {
        let model = Script(previews: [.success(F.preview())]).model()
        let waiting = [F.instance(3, "valkey-cache", decision: .stopRestore)]
        model.receive(event: Self.event(F.plan(.stopping, instances: waiting), sequence: 1))
        #expect(model.markedInstances.isEmpty)
    }

    // MARK: - The toast

    @Test func aRestartThatFinishesWhileWatchedLeavesOneToastWithTheCounts() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.stopping, instances: F.mixedInstances), sequence: 1))
        let done = [
            F.instance(1, "pg17-dev", outcome: .adopted),
            F.instance(2, "nats-local", outcome: .adopted),
            F.instance(
                3, "valkey-cache", decision: .stopRestore, stopped: true, outcome: .restored),
        ]
        model.receive(event: Self.event(F.plan(.finished, instances: done), sequence: 2))
        let toast = model.toast
        #expect(toast?.message == "Daemon restarted. 2 instances adopted, 1 started again.")
        if let toast { model.consumeToast(toast.id) }
        #expect(model.toast == nil)
        #expect(model.isRestarting == false)
    }

    @Test func aFinishedPlanTheAppLearnsFromTheHandshakeMakesNoToast() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(info: Self.info(restart: F.plan(.finished, instances: F.mixedInstances)))
        #expect(model.plan?.state == .finished)
        #expect(model.toast == nil)
    }

    @Test func aRestartThisWindowAskedForToastsEvenWhenItFirstSeesThePlanFinished() async {
        let script = Script(previews: [.success(F.preview())], outcomes: [Self.accepted])
        let model = script.model()
        await model.begin()
        await model.confirm()
        model.receive(info: Self.info(restart: F.plan(.finished, instances: F.mixedInstances)))
        #expect(model.toast != nil)
        #expect(model.isAwaitingPlan == false)
    }

    private static func reconciling(_ phase: ReconcilePhase, sequence: UInt64) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: FixtureClock.now, resource: nil,
            revision: nil,
            kind: .daemonReconciling(
                DaemonReconciling(
                    phase: phase, instances: 3, adopted: 2, unknown: 0, restarting: 0,
                    findings: nil)))
    }

    @Test func aNewDaemonThatDoesNotKnowThePlanYetKeepsTheOpenOne() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.handedOver, instances: F.mixedInstances), sequence: 1))
        model.receive(info: Self.info(restart: nil))
        #expect(model.plan?.state == .handedOver)
        #expect(model.banner(now: FixtureClock.now)?.title == RestartCopy.restartingTitle)
        #expect(model.markedInstances.isEmpty == false)
    }

    @Test func aRestartThatWaitsForItsPlanKeepsWaitingWhenTheFirstInfoHasNone() async {
        let script = Script(previews: [.success(F.preview())], outcomes: [Self.accepted])
        let model = script.model()
        await model.begin()
        await model.confirm()
        model.receive(info: Self.info(restart: nil))
        #expect(model.isAwaitingPlan)
        #expect(model.banner(now: FixtureClock.now) != nil)
    }

    @Test func aPlanTheNewDaemonTakesOverAfterAnEmptyInfoStillMakesTheToast() async {
        let script = Script(previews: [.success(F.preview())], outcomes: [Self.accepted])
        let model = script.model()
        await model.begin()
        await model.confirm()
        model.receive(
            event: Self.event(F.plan(.handedOver, instances: F.mixedInstances), sequence: 1))
        model.receive(info: Self.info(restart: nil))
        model.receive(
            event: Self.event(F.plan(.restoring, instances: F.mixedInstances), sequence: 2))
        model.receive(
            event: Self.event(F.plan(.finished, instances: F.mixedInstances), sequence: 3))
        #expect(model.toast != nil)
    }

    @Test func aReconciliationThatEndsWithoutThePlanClosesTheBannerOfTheOldOne() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.handedOver, instances: F.mixedInstances), sequence: 1))
        model.receive(event: Self.reconciling(.started, sequence: 2))
        #expect(model.plan?.state == .handedOver)
        model.receive(event: Self.reconciling(.finished, sequence: 3))
        #expect(model.plan == nil)
        #expect(model.banner(now: FixtureClock.now) == nil)
        #expect(model.isRestarting == false)
    }

    @Test func aReconciliationThatEndsLeavesAPlanTheNewDaemonTookOver() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.restoring, instances: F.mixedInstances), sequence: 1))
        model.receive(event: Self.reconciling(.finished, sequence: 2))
        #expect(model.plan?.state == .restoring)
        #expect(model.isRestarting)
    }

    @Test func aReconciliationThatEndsStopsTheWaitForAPlanThatNeverCame() async {
        let script = Script(previews: [.success(F.preview())], outcomes: [Self.accepted])
        let model = script.model()
        await model.begin()
        await model.confirm()
        model.receive(event: Self.reconciling(.finished, sequence: 1))
        #expect(model.isAwaitingPlan == false)
        #expect(model.banner(now: FixtureClock.now) == nil)
    }

    @Test func aHandshakeWithoutInfoChangesNothing() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.handedOver, instances: F.mixedInstances), sequence: 1))
        model.receive(info: nil)
        #expect(model.plan?.state == .handedOver)
    }

    @Test func aWarningBannerGivesWayToTheDaemonBannerAndComesBackWithoutIt() {
        let model = Script(previews: [.success(F.preview())]).model()
        let plan = F.plan(.expired, instances: [], closeReason: .bootChanged)
        model.receive(event: Self.event(plan, sequence: 1))
        #expect(model.banner(now: FixtureClock.now, daemonBannerShown: true) == nil)
        #expect(model.banner(now: FixtureClock.now, daemonBannerShown: false) != nil)
        model.dismissBanner()
        #expect(model.banner(now: FixtureClock.now) == nil)
    }

    @Test func aRunningRestartTakesTheBannerSlotFromTheDaemonBanner() {
        let model = Script(previews: [.success(F.preview())]).model()
        model.receive(
            event: Self.event(F.plan(.handedOver, instances: F.mixedInstances), sequence: 1))
        #expect(model.banner(now: FixtureClock.now, daemonBannerShown: true) != nil)
    }

    static func info(restart: RestartPlanView?, context: LaunchContextKind = .launchd) -> DaemonInfo
    {
        let daemon = DaemonModelTests.daemon(.ready)
        guard let info = daemon.info else { fatalError("the test daemon always has info") }
        return DaemonInfo(
            version: info.version, protocol: info.protocol, contractDigest: info.contractDigest,
            serverInstanceId: info.serverInstanceId, pid: info.pid, uid: info.uid,
            startedAt: info.startedAt, roots: info.roots, socketPath: info.socketPath,
            registrySchema: info.registrySchema, secretStore: info.secretStore,
            launch: DaemonLaunch(context: context, label: "l", bundlePath: nil),
            restart: restart, session: nil)
    }
}
