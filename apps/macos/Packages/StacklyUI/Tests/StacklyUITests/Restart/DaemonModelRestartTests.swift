import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The restart model inside the daemon model (spec 0030 AC-16): the plan the handshake and
/// the events bring, and the two calls over the link with the key and the long limit.
@Suite(.timeLimit(.minutes(1)))
struct DaemonModelRestartTests {
    private typealias F = FixtureRestart

    private static func model() -> (DaemonModel, DaemonModelTests.FakeLink) {
        let link = DaemonModelTests.FakeLink()
        let model = DaemonModel(
            link: link.handle, signals: nil, endpoint: nil, launchedAt: DaemonModelTests.launch)
        return (model, link)
    }

    private static func connected(restart: RestartPlanView?, at seconds: TimeInterval = 10)
        -> ConnectionState
    {
        var daemon = DaemonModelTests.daemon(.ready, at: seconds)
        daemon.info = DaemonRestartModelTests.info(restart: restart)
        return .connected(daemon)
    }

    @Test func aNewConnectionTakesThePlanTheDaemonReports() {
        let (model, _) = Self.model()
        model.receive(Self.connected(restart: F.plan(.restoring, instances: F.mixedInstances)))
        #expect(model.restart.plan?.state == .restoring)
        #expect(model.restart.isRestarting)
    }

    @Test func aHeartbeatOnTheSameConnectionDoesNotWipeANewerPlan() {
        let (model, _) = Self.model()
        model.receive(Self.connected(restart: nil))
        model.receive(
            .event(
                Event(
                    subscriptionId: 1, sequence: 1, at: FixtureClock.now, resource: nil,
                    revision: nil,
                    kind: .daemonRestartPlan(F.plan(.handedOver, instances: F.mixedInstances)))))
        // The same connection again, with the info the handshake read before the plan existed.
        model.receive(Self.connected(restart: nil))
        #expect(model.restart.plan?.state == .handedOver)
    }

    @Test func aNewConnectionInRecoveryModeEndsThePlanTheOldDaemonLeft() throws {
        let (model, _) = Self.model()
        model.receive(Self.connected(restart: F.plan(.handedOver, instances: F.mixedInstances)))
        #expect(model.restart.isRestarting)
        guard case .connected(var daemon) = Self.connected(restart: nil, at: 20) else {
            Issue.record("the helper always connects")
            return
        }
        daemon.health = try DaemonModelTests.recoveryHealth()
        model.receive(.connected(daemon))
        #expect(model.restart.plan == nil)
        #expect(model.restart.isRestarting == false)
        #expect(model.restart.banner(now: FixtureClock.now) == nil)
    }

    @Test func aNewConnectionOutsideRecoveryModeKeepsTheOpenPlan() {
        let (model, _) = Self.model()
        model.receive(Self.connected(restart: F.plan(.handedOver, instances: F.mixedInstances)))
        model.receive(Self.connected(restart: nil, at: 20))
        #expect(model.restart.plan?.state == .handedOver)
    }

    @Test func aPlanEventReachesTheRestartModel() {
        let (model, _) = Self.model()
        model.receive(Self.connected(restart: nil))
        model.receive(
            .event(
                Event(
                    subscriptionId: 1, sequence: 1, at: FixtureClock.now, resource: nil,
                    revision: nil,
                    kind: .daemonRestartPlan(F.plan(.stopping, instances: F.mixedInstances)))))
        #expect(model.restart.plan?.state == .stopping)
        #expect(
            model.restart.markedInstances == [FixtureID.uuid(kind: FixtureID.instance, number: 3)])
    }

    /// Lets the tasks on the main actor run until `done` holds.
    private static func settle(_ done: () -> Bool) async {
        var turns = 0
        while done() == false, turns < 2000 {
            await Task.yield()
            turns += 1
        }
    }

    @Test func aFinishedSnapshotReadsTheInfoAgainAndFindsThePlanItMissed() async {
        let (model, link) = Self.model()
        model.receive(Self.connected(restart: nil))
        model.receive(
            .event(
                Event(
                    subscriptionId: 1, sequence: 1, at: FixtureClock.now, resource: nil,
                    revision: nil,
                    kind: .daemonRestartPlan(F.plan(.restoring, instances: F.mixedInstances)))))
        let finished = F.plan(.finished, instances: F.mixedInstances)
        link.result.withLock {
            $0 = .success(.daemonGetInfo(DaemonRestartModelTests.info(restart: finished)))
        }

        model.receive(.resnapshot(.gap(.cursorExpired)))
        await Self.settle { model.restart.plan?.state == .finished }

        #expect(model.restart.plan?.state == .finished)
        #expect(model.restart.isRestarting == false)
        #expect(link.calls.withLock { $0 }.contains { $0.0 == .daemonGetInfo })
    }

    @Test func aConnectionThatCaughtUpReadsTheInfoAgainToo() async {
        let (model, link) = Self.model()
        model.receive(Self.connected(restart: nil))
        let restoring = F.plan(.restoring, instances: F.mixedInstances)
        link.result.withLock {
            $0 = .success(.daemonGetInfo(DaemonRestartModelTests.info(restart: restoring)))
        }

        model.receive(.caughtUp)
        await Self.settle { model.restart.plan != nil }

        #expect(model.restart.plan?.state == .restoring)
    }

    @Test func anInfoThatCannotBeReadAfterASnapshotChangesNothing() async {
        let (model, link) = Self.model()
        model.receive(Self.connected(restart: F.plan(.restoring, instances: F.mixedInstances)))
        link.result.withLock { $0 = .failure(.notSent(ClientErrorCatalog.notConnected())) }

        model.receive(.caughtUp)
        await Self.settle { link.calls.withLock { $0 }.contains { $0.0 == .daemonGetInfo } }
        await Task.yield()

        #expect(model.restart.plan?.state == .restoring)
    }

    @Test func theSheetReadsThePreviewAndTheRestartCarriesTheDigestAndAKey() async throws {
        let (model, link) = Self.model()
        model.receive(Self.connected(restart: nil))
        link.result.withLock { $0 = .success(.daemonRestartPreview(F.preview(digest: "abc"))) }
        await model.restart.begin()
        #expect(model.restart.confirmation?.preview.digest == "abc")

        let accepted = ShutdownAccepted(
            serverInstanceId: UUID(), mode: .restart, planId: F.planID, exiting: true, stopAll: nil)
        link.result.withLock { $0 = .success(.daemonShutdown(accepted)) }
        await model.restart.confirm()

        let calls = link.calls.withLock { $0 }
        let last = try #require(calls.last)
        #expect(
            last.0
                == .daemonShutdown(
                    ShutdownParams(
                        mode: .restart, expectedPlanDigest: "abc", force: false, overrideLeases: nil
                    )))
        #expect(UUID(uuidString: try #require(last.1)) != nil)
        #expect(model.restart.confirmation == nil)
        #expect(model.restart.isAwaitingPlan)
    }

    @Test func aRefusalKeepsTheSheetOpenWithTheDaemonsReason() async throws {
        let (model, link) = Self.model()
        model.receive(Self.connected(restart: nil))
        link.result.withLock { $0 = .success(.daemonRestartPreview(F.preview())) }
        await model.restart.begin()
        let refusal = DaemonRestartModelTests.refusal("daemon.restart_pending")
        link.result.withLock { $0 = .failure(.rejected(refusal)) }
        await model.restart.confirm()
        #expect(model.restart.confirmation?.error == refusal)
        #expect(model.restart.isAwaitingPlan == false)
    }
}
